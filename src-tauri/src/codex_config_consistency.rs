use crate::app_config::AppType;
use crate::codex_config;
use crate::error::AppError;
use crate::services::provider::build_codex_live_config_for_provider;
use crate::store::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use tauri::{Emitter, Manager, State};

const LAST_ACTION_KEY: &str = "codex_config_consistency:last_action";
const LAST_ACTUAL_FINGERPRINT_KEY: &str = "codex_config_consistency:last_actual_fingerprint";
const LAST_PROVIDER_ID_KEY: &str = "codex_config_consistency:last_provider_id";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexConfigConsistencyState {
    Consistent,
    ExternalDrift,
    NotApplicable,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexConfigConsistencyReport {
    pub state: CodexConfigConsistencyState,
    pub provider_id: Option<String>,
    pub expected_fingerprint: Option<String>,
    pub actual_fingerprint: Option<String>,
    pub changed_keys: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexConfigConsistencyAction {
    ApplyCcsm,
    KeepCodex,
    Later,
}

fn canonicalize_toml_value(value: &toml::Value, key: Option<&str>) -> JsonValue {
    if key == Some("model_catalog_json") {
        return JsonValue::String("<ccsm-managed-model-catalog>".to_string());
    }

    match value {
        toml::Value::String(value) => JsonValue::String(value.clone()),
        toml::Value::Integer(value) => JsonValue::Number((*value).into()),
        toml::Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(JsonValue::Number)
            .unwrap_or_else(|| JsonValue::String(value.to_string())),
        toml::Value::Boolean(value) => JsonValue::Bool(*value),
        toml::Value::Datetime(value) => JsonValue::String(value.to_string()),
        toml::Value::Array(values) => JsonValue::Array(
            values
                .iter()
                .map(|value| canonicalize_toml_value(value, None))
                .collect(),
        ),
        toml::Value::Table(values) => {
            let mut object = JsonMap::new();
            for (name, value) in values {
                object.insert(
                    name.clone(),
                    canonicalize_toml_value(value, Some(name.as_str())),
                );
            }
            JsonValue::Object(object)
        }
    }
}

fn canonicalize_toml(text: &str) -> Result<JsonValue, AppError> {
    let value = text.parse::<toml::Value>().map_err(|error| {
        AppError::Config(format!("Codex config.toml semantic parse failed: {error}"))
    })?;
    Ok(canonicalize_toml_value(&value, None))
}

pub(crate) fn fingerprint_toml(text: &str) -> Result<String, AppError> {
    let canonical = canonicalize_toml(text)?;
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| AppError::JsonSerialize { source: error })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn flatten_json(value: &JsonValue, prefix: &str, output: &mut BTreeMap<String, String>) {
    match value {
        JsonValue::Object(object) if object.is_empty() && !prefix.is_empty() => {
            output.insert(prefix.to_string(), "{}".to_string());
        }
        JsonValue::Object(object) => {
            for (key, child) in object {
                let child_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_json(child, &child_prefix, output);
            }
        }
        JsonValue::Array(array) if array.is_empty() => {
            output.insert(prefix.to_string(), "[]".to_string());
        }
        JsonValue::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                flatten_json(child, &format!("{prefix}[{index}]"), output);
            }
        }
        other => {
            output.insert(prefix.to_string(), other.to_string());
        }
    }
}

pub(crate) fn changed_key_paths(before: &str, after: &str) -> Result<Vec<String>, AppError> {
    let before = canonicalize_toml(before)?;
    let after = canonicalize_toml(after)?;
    let mut before_paths = BTreeMap::new();
    let mut after_paths = BTreeMap::new();
    flatten_json(&before, "", &mut before_paths);
    flatten_json(&after, "", &mut after_paths);
    Ok(before_paths
        .keys()
        .chain(after_paths.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| before_paths.get(*key) != after_paths.get(*key))
        .take(64)
        .cloned()
        .collect())
}

fn report(
    state: CodexConfigConsistencyState,
    provider_id: Option<String>,
    expected_fingerprint: Option<String>,
    actual_fingerprint: Option<String>,
    changed_keys: Vec<String>,
    reason: Option<&str>,
) -> CodexConfigConsistencyReport {
    CodexConfigConsistencyReport {
        state,
        provider_id,
        expected_fingerprint,
        actual_fingerprint,
        changed_keys,
        reason: reason.map(str::to_string),
    }
}

pub(crate) fn should_emit_startup_event(state: CodexConfigConsistencyState) -> bool {
    state == CodexConfigConsistencyState::ExternalDrift
}

/// Run the one-shot live-config reconciliation after startup writers and proxy
/// recovery have settled. Emitting an event is deliberately best-effort for
/// startup: a UI listener can query the same report as a race-free fallback.
pub(crate) async fn reconcile_after_startup(
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let report = inspect(&state)?;
    if should_emit_startup_event(report.state) {
        app.emit("codex-config-consistency", &report)
            .map_err(|error| AppError::Message(format!("Codex config consistency event failed: {error}")))?;
    }
    Ok(())
}

pub fn inspect(state: &AppState) -> Result<CodexConfigConsistencyReport, AppError> {
    let provider_id = crate::settings::get_effective_current_provider(&state.db, &AppType::Codex)?;
    let Some(provider_id) = provider_id else {
        return Ok(report(
            CodexConfigConsistencyState::NotApplicable,
            None,
            None,
            None,
            Vec::new(),
            Some("no_current_provider"),
        ));
    };

    if state
        .proxy_service
        .detect_takeover_in_live_config_for_app(&AppType::Codex)
    {
        return Ok(report(
            CodexConfigConsistencyState::NotApplicable,
            Some(provider_id),
            None,
            None,
            Vec::new(),
            Some("proxy_takeover_active"),
        ));
    }

    let provider = state
        .db
        .get_provider_by_id(&provider_id, AppType::Codex.as_str())?
        .ok_or_else(|| AppError::Config("Codex current provider is missing".to_string()))?;
    let expected_text = match build_codex_live_config_for_provider(&state.db, &provider) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("Codex config consistency expected config build failed: {error}");
            return Ok(report(
                CodexConfigConsistencyState::Unavailable,
                Some(provider_id),
                None,
                None,
                Vec::new(),
                Some("expected_config_unavailable"),
            ));
        }
    };

    let live_path = codex_config::get_codex_config_path();
    let actual_text = match fs::read_to_string(&live_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(report(
                CodexConfigConsistencyState::Unavailable,
                Some(provider_id),
                Some(fingerprint_toml(&expected_text)?),
                None,
                Vec::new(),
                Some("live_config_missing"),
            ));
        }
        Err(error) => return Err(AppError::io(&live_path, error)),
    };

    let expected_fingerprint = fingerprint_toml(&expected_text)?;
    let actual_fingerprint = match fingerprint_toml(&actual_text) {
        Ok(fingerprint) => fingerprint,
        Err(error) => {
            log::warn!("Codex config consistency live TOML parse failed: {error}");
            return Ok(report(
                CodexConfigConsistencyState::Unavailable,
                Some(provider_id),
                Some(expected_fingerprint),
                None,
                Vec::new(),
                Some("invalid_toml"),
            ));
        }
    };

    if expected_fingerprint == actual_fingerprint {
        return Ok(report(
            CodexConfigConsistencyState::Consistent,
            Some(provider_id),
            Some(expected_fingerprint),
            Some(actual_fingerprint),
            Vec::new(),
            None,
        ));
    }

    Ok(report(
        CodexConfigConsistencyState::ExternalDrift,
        Some(provider_id),
        Some(expected_fingerprint),
        Some(actual_fingerprint),
        changed_key_paths(&expected_text, &actual_text)?,
        Some("live_config_changed"),
    ))
}

pub fn resolve(
    state: &AppState,
    expected_fingerprint: String,
    action: CodexConfigConsistencyAction,
) -> Result<CodexConfigConsistencyReport, AppError> {
    let current = inspect(state)?;
    if action == CodexConfigConsistencyAction::Later {
        return Ok(current);
    }

    let Some(actual_fingerprint) = current.actual_fingerprint.clone() else {
        return Ok(current);
    };

    match action {
        CodexConfigConsistencyAction::KeepCodex => {
            state.db.set_setting(LAST_ACTION_KEY, "keep_codex")?;
            state
                .db
                .set_setting(LAST_ACTUAL_FINGERPRINT_KEY, &actual_fingerprint)?;
            state.db.set_setting(
                LAST_PROVIDER_ID_KEY,
                current.provider_id.as_deref().unwrap_or_default(),
            )?;
            Ok(current)
        }
        CodexConfigConsistencyAction::ApplyCcsm => {
            if expected_fingerprint != actual_fingerprint {
                return Err(AppError::InvalidInput(
                    "codex_config_consistency_stale_fingerprint".to_string(),
                ));
            }
            let provider_id = current
                .provider_id
                .clone()
                .ok_or_else(|| AppError::Config("Codex current provider is missing".to_string()))?;
            let provider = state
                .db
                .get_provider_by_id(&provider_id, AppType::Codex.as_str())?
                .ok_or_else(|| AppError::Config("Codex current provider is missing".to_string()))?;
            let live_path = codex_config::get_codex_config_path();
            let backup_path = live_path.with_file_name(format!(
                "config.toml.ccsm-drift-{}.bak",
                chrono::Utc::now().format("%Y%m%dT%H%M%S%.fZ")
            ));
            let mut backup_created = false;
            codex_config::reconcile_codex_live_config_atomic(|before| {
                let observed = fingerprint_toml(before)?;
                if observed != expected_fingerprint {
                    return Err(AppError::InvalidInput(
                        "codex_config_consistency_stale_fingerprint".to_string(),
                    ));
                }
                let candidate = build_codex_live_config_for_provider(&state.db, &provider)?;
                if !backup_created {
                    fs::write(&backup_path, before.as_bytes())
                        .map_err(|error| AppError::io(&backup_path, error))?;
                    backup_created = true;
                }
                Ok(candidate)
            })?;
            inspect(state)
        }
        CodexConfigConsistencyAction::Later => Ok(current),
    }
}

#[tauri::command]
pub fn inspect_codex_config_consistency(
    state: State<'_, AppState>,
) -> Result<CodexConfigConsistencyReport, String> {
    inspect(&state).map_err(String::from)
}

#[tauri::command]
pub fn resolve_codex_config_consistency(
    state: State<'_, AppState>,
    expected_fingerprint: String,
    action: CodexConfigConsistencyAction,
) -> Result<CodexConfigConsistencyReport, String> {
    resolve(&state, expected_fingerprint, action).map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::Provider;
    use serde_json::json;
    use serial_test::serial;

    struct TestHomeGuard {
        _dir: tempfile::TempDir,
        original: Option<String>,
    }

    impl TestHomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("create temp home");
            let original = std::env::var("CC_SWITCH_TEST_HOME").ok();
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                _dir: dir,
                original,
            }
        }
    }

    impl Drop for TestHomeGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn seed_provider() -> (crate::store::AppState, String) {
        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let provider = Provider::with_id(
            "consistency-provider".to_string(),
            "Consistency Provider".to_string(),
            json!({
                "auth": {},
                "config": "model = \"gpt-5.5\"\nmodel_reasoning_effort = \"medium\"\n",
                "modelCatalog": {"models": [{"model": "gpt-5.5"}]}
            }),
            None,
        );
        db.save_provider(AppType::Codex.as_str(), &provider)
            .expect("save provider");
        db.set_current_provider(AppType::Codex.as_str(), &provider.id)
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some(&provider.id))
            .expect("set local current provider");
        (crate::store::AppState::new(db), provider.id)
    }

    #[test]
    fn semantic_fingerprint_ignores_comments_whitespace_and_key_order() {
        let first = "# user note\nmodel = \"gpt-5.5\"\nmodel_reasoning_effort = \"medium\"\n";
        let second = "model_reasoning_effort = \"medium\"\nmodel = \"gpt-5.5\" # inline note\n";

        assert_eq!(
            fingerprint_toml(first).expect("first fingerprint"),
            fingerprint_toml(second).expect("second fingerprint")
        );
        assert!(changed_key_paths(first, second)
            .expect("semantic diff")
            .is_empty());
    }

    #[test]
    fn semantic_diff_reports_changed_paths_without_values() {
        let before = "model = \"gpt-5.5\"\n[features]\nweb_search = true\n";
        let after = "model = \"gpt-5.5\"\n[features]\nweb_search = false\n";

        let changed = changed_key_paths(before, after).expect("semantic diff");

        assert_eq!(changed, vec!["features.web_search"]);
        let serialized = serde_json::to_string(&changed).expect("serialize paths");
        assert!(!serialized.contains("true"));
        assert!(!serialized.contains("false"));
    }

    #[test]
    #[serial]
    fn inspect_reports_external_drift_with_changed_paths_only() {
        let _home = TestHomeGuard::new();
        crate::settings::reload_settings().expect("reload settings");
        let (state, _) = seed_provider();
        let provider = state
            .db
            .get_provider_by_id("consistency-provider", AppType::Codex.as_str())
            .expect("read provider")
            .expect("provider exists");
        let expected = build_codex_live_config_for_provider(&state.db, &provider)
            .expect("build expected live config");
        let mut live = expected
            .parse::<toml_edit::DocumentMut>()
            .expect("parse expected config");
        live["model_reasoning_effort"] = toml_edit::value("high");
        codex_config::write_codex_live_config_atomic(Some(&live.to_string()))
            .expect("write drifted live config");

        let result = inspect(&state).expect("inspect config consistency");

        assert_eq!(result.state, CodexConfigConsistencyState::ExternalDrift);
        assert_eq!(result.provider_id.as_deref(), Some("consistency-provider"));
        assert!(result
            .changed_keys
            .contains(&"model_reasoning_effort".to_string()));
        assert!(result.changed_keys.iter().all(|key| !key.contains("high")));
    }

    #[test]
    #[serial]
    fn inspect_marks_invalid_live_toml_unavailable() {
        let _home = TestHomeGuard::new();
        crate::settings::reload_settings().expect("reload settings");
        let (state, _) = seed_provider();
        codex_config::write_codex_live_config_atomic(Some("model = [\n"))
            .expect_err("invalid TOML must not be written by the atomic writer");
        let path = codex_config::get_codex_config_path();
        std::fs::create_dir_all(path.parent().expect("config parent")).expect("create parent");
        std::fs::write(&path, "model = [\n").expect("seed invalid external TOML");

        let result = inspect(&state).expect("inspect invalid config");

        assert_eq!(result.state, CodexConfigConsistencyState::Unavailable);
        assert_eq!(result.reason.as_deref(), Some("invalid_toml"));
        assert!(result.actual_fingerprint.is_none());
    }

    fn seed_drifted_state() -> (TestHomeGuard, crate::store::AppState, String) {
        let home = TestHomeGuard::new();
        crate::settings::reload_settings().expect("reload settings");
        let (state, provider_id) = seed_provider();
        let provider = state
            .db
            .get_provider_by_id(&provider_id, AppType::Codex.as_str())
            .expect("read provider")
            .expect("provider exists");
        let expected = build_codex_live_config_for_provider(&state.db, &provider)
            .expect("build expected live config");
        let mut live = expected
            .parse::<toml_edit::DocumentMut>()
            .expect("parse expected config");
        live["model_reasoning_effort"] = toml_edit::value("high");
        codex_config::write_codex_live_config_atomic(Some(&live.to_string()))
            .expect("write drifted live config");
        (home, state, provider_id)
    }

    #[test]
    #[serial]
    fn keep_codex_records_only_fingerprint_acknowledgement() {
        let (_home, state, provider_id) = seed_drifted_state();
        let before = inspect(&state).expect("inspect drift");
        let actual = before
            .actual_fingerprint
            .clone()
            .expect("actual fingerprint");

        let after = resolve(
            &state,
            actual.clone(),
            CodexConfigConsistencyAction::KeepCodex,
        )
        .expect("keep Codex changes");

        assert_eq!(after.state, CodexConfigConsistencyState::ExternalDrift);
        assert_eq!(
            state.db.get_setting(LAST_ACTION_KEY).unwrap().as_deref(),
            Some("keep_codex")
        );
        assert_eq!(
            state
                .db
                .get_setting(LAST_ACTUAL_FINGERPRINT_KEY)
                .unwrap()
                .as_deref(),
            Some(actual.as_str())
        );
        assert_eq!(
            state
                .db
                .get_setting(LAST_PROVIDER_ID_KEY)
                .unwrap()
                .as_deref(),
            Some(provider_id.as_str())
        );
    }

    #[test]
    #[serial]
    fn later_does_not_persist_an_acknowledgement() {
        let (_home, state, _) = seed_drifted_state();
        let before = inspect(&state).expect("inspect drift");
        let actual = before
            .actual_fingerprint
            .clone()
            .expect("actual fingerprint");

        resolve(&state, actual, CodexConfigConsistencyAction::Later).expect("defer Codex changes");

        assert!(state.db.get_setting(LAST_ACTION_KEY).unwrap().is_none());
        assert!(state
            .db
            .get_setting(LAST_ACTUAL_FINGERPRINT_KEY)
            .unwrap()
            .is_none());
        assert!(state
            .db
            .get_setting(LAST_PROVIDER_ID_KEY)
            .unwrap()
            .is_none());
    }

    #[test]
    #[serial]
    fn apply_ccsm_uses_compare_and_swap_and_creates_a_drift_backup() {
        let (_home, state, _) = seed_drifted_state();
        let before = inspect(&state).expect("inspect drift");
        let actual = before
            .actual_fingerprint
            .clone()
            .expect("actual fingerprint");

        let after = resolve(&state, actual, CodexConfigConsistencyAction::ApplyCcsm)
            .expect("apply CCSM config");

        assert_eq!(after.state, CodexConfigConsistencyState::Consistent);
        let backups = std::fs::read_dir(codex_config::get_codex_config_dir())
            .expect("read Codex config directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.ccsm-drift-")
            })
            .count();
        assert_eq!(backups, 1, "apply must create one recoverable drift backup");
    }

    #[test]
    #[serial]
    fn apply_ccsm_rejects_a_stale_live_fingerprint_before_writing() {
        let (_home, state, _) = seed_drifted_state();
        let error = resolve(
            &state,
            "stale-fingerprint".to_string(),
            CodexConfigConsistencyAction::ApplyCcsm,
        )
        .expect_err("stale fingerprint must not overwrite Codex changes");

        assert!(error
            .to_string()
            .contains("codex_config_consistency_stale_fingerprint"));
    }

    #[test]
    fn startup_reconciliation_emits_only_for_external_drift() {
        let states = [
            CodexConfigConsistencyState::Consistent,
            CodexConfigConsistencyState::NotApplicable,
            CodexConfigConsistencyState::Unavailable,
        ];
        assert!(states.iter().all(|state| !should_emit_startup_event(*state)));
        assert!(should_emit_startup_event(
            CodexConfigConsistencyState::ExternalDrift
        ));
    }
}
