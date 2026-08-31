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
use std::time::{SystemTime, UNIX_EPOCH};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexConfigRuntimeActivationState {
    NotRunning,
    Current,
    RestartRequired,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexConfigRuntimeActivation {
    pub state: CodexConfigRuntimeActivationState,
    pub app_server_started_at: Option<String>,
    pub config_modified_at: Option<String>,
    pub reason: Option<String>,
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
    pub runtime_activation: CodexConfigRuntimeActivation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexConfigConsistencyAction {
    ApplyCcsm,
    KeepCodex,
    Later,
}

fn canonicalize_toml_value(value: &toml::Value) -> JsonValue {
    match value {
        toml::Value::String(value) => JsonValue::String(value.clone()),
        toml::Value::Integer(value) => JsonValue::Number((*value).into()),
        toml::Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(JsonValue::Number)
            .unwrap_or_else(|| JsonValue::String(value.to_string())),
        toml::Value::Boolean(value) => JsonValue::Bool(*value),
        toml::Value::Datetime(value) => JsonValue::String(value.to_string()),
        toml::Value::Array(values) => {
            JsonValue::Array(values.iter().map(canonicalize_toml_value).collect())
        }
        toml::Value::Table(values) => {
            let mut object = JsonMap::new();
            for (name, value) in values {
                object.insert(name.clone(), canonicalize_toml_value(value));
            }
            JsonValue::Object(object)
        }
    }
}

#[cfg(test)]
fn canonicalize_toml(text: &str) -> Result<JsonValue, AppError> {
    let value = text.parse::<toml::Value>().map_err(|error| {
        AppError::Config(format!("Codex config.toml semantic parse failed: {error}"))
    })?;
    Ok(canonicalize_toml_value(&value))
}

#[cfg(test)]
pub(crate) fn fingerprint_toml(text: &str) -> Result<String, AppError> {
    let canonical = canonicalize_toml(text)?;
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| AppError::JsonSerialize { source: error })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

const CCSM_MANAGED_TOP_LEVEL_KEYS: &[&str] = &[
    "model",
    "model_provider",
    "model_catalog_json",
    "openai_base_url",
    "experimental_bearer_token",
];

fn active_model_provider_id(value: &toml::Value) -> Option<String> {
    value
        .get("model_provider")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn ccsm_owned_projection(
    text: &str,
    provider_id_hint: Option<&str>,
) -> Result<JsonValue, AppError> {
    let value = text.parse::<toml::Value>().map_err(|error| {
        AppError::Config(format!("Codex config.toml semantic parse failed: {error}"))
    })?;
    let root = value
        .as_table()
        .ok_or_else(|| AppError::Config("Codex config.toml root must be a table".to_string()))?;
    let mut managed = toml::map::Map::new();

    for key in CCSM_MANAGED_TOP_LEVEL_KEYS {
        if let Some(value) = root.get(*key) {
            managed.insert((*key).to_string(), value.clone());
        }
    }

    if root
        .get(crate::codex_config::CODEX_WEB_SEARCH_FIELD)
        .and_then(toml::Value::as_str)
        == Some(crate::codex_config::CODEX_WEB_SEARCH_DISABLED)
    {
        managed.insert(
            crate::codex_config::CODEX_WEB_SEARCH_FIELD.to_string(),
            root[crate::codex_config::CODEX_WEB_SEARCH_FIELD].clone(),
        );
    }

    let provider_id = provider_id_hint
        .map(str::to_string)
        .or_else(|| active_model_provider_id(&value));
    if provider_id.as_deref() == Some(crate::codex_config::CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID)
    {
        for key in ["model_context_window", "model_auto_compact_token_limit"] {
            if let Some(value) = root.get(key) {
                managed.insert(key.to_string(), value.clone());
            }
        }
    }

    if let Some(provider_id) = provider_id {
        if let Some(provider) = root
            .get("model_providers")
            .and_then(toml::Value::as_table)
            .and_then(|providers| providers.get(&provider_id))
        {
            let mut providers = toml::map::Map::new();
            providers.insert(provider_id, provider.clone());
            managed.insert("model_providers".to_string(), toml::Value::Table(providers));
        }
    }

    Ok(canonicalize_toml_value(&toml::Value::Table(managed)))
}

fn fingerprint_json(value: &JsonValue) -> Result<String, AppError> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| AppError::JsonSerialize { source: error })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn managed_fingerprint(text: &str, provider_id_hint: Option<&str>) -> Result<String, AppError> {
    fingerprint_json(&ccsm_owned_projection(text, provider_id_hint)?)
}

fn active_model_provider_id_from_text(text: &str) -> Result<Option<String>, AppError> {
    let value = text.parse::<toml::Value>().map_err(|error| {
        AppError::Config(format!("Codex config.toml semantic parse failed: {error}"))
    })?;
    Ok(active_model_provider_id(&value))
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

#[cfg(test)]
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

fn changed_managed_key_paths(
    expected: &str,
    actual: &str,
    provider_id_hint: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let expected = ccsm_owned_projection(expected, provider_id_hint)?;
    let actual = ccsm_owned_projection(actual, provider_id_hint)?;
    let mut expected_paths = BTreeMap::new();
    let mut actual_paths = BTreeMap::new();
    flatten_json(&expected, "", &mut expected_paths);
    flatten_json(&actual, "", &mut actual_paths);
    Ok(expected_paths
        .keys()
        .chain(actual_paths.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| expected_paths.get(*key) != actual_paths.get(*key))
        .take(64)
        .cloned()
        .collect())
}

fn copy_or_remove_top_level_item(
    target: &mut toml_edit::DocumentMut,
    expected: &toml_edit::DocumentMut,
    key: &str,
) {
    match expected.get(key).cloned() {
        Some(item) => {
            target.as_table_mut().insert(key, item);
        }
        None => {
            target.as_table_mut().remove(key);
        }
    }
}

fn merge_ccsm_owned_projection(current: &str, expected: &str) -> Result<String, AppError> {
    let mut target = current
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| AppError::Config(format!("Invalid live Codex config.toml: {error}")))?;
    let expected = expected
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| {
            AppError::Config(format!("Invalid expected Codex config.toml: {error}"))
        })?;

    for key in CCSM_MANAGED_TOP_LEVEL_KEYS {
        copy_or_remove_top_level_item(&mut target, &expected, key);
    }

    match expected
        .get(crate::codex_config::CODEX_WEB_SEARCH_FIELD)
        .and_then(toml_edit::Item::as_str)
    {
        Some(crate::codex_config::CODEX_WEB_SEARCH_DISABLED) => copy_or_remove_top_level_item(
            &mut target,
            &expected,
            crate::codex_config::CODEX_WEB_SEARCH_FIELD,
        ),
        _ => {
            if target
                .get(crate::codex_config::CODEX_WEB_SEARCH_FIELD)
                .and_then(toml_edit::Item::as_str)
                == Some(crate::codex_config::CODEX_WEB_SEARCH_DISABLED)
            {
                target
                    .as_table_mut()
                    .remove(crate::codex_config::CODEX_WEB_SEARCH_FIELD);
            }
        }
    }

    let expected_provider_id = expected
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if expected_provider_id.as_deref()
        == Some(crate::codex_config::CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID)
    {
        for key in ["model_context_window", "model_auto_compact_token_limit"] {
            copy_or_remove_top_level_item(&mut target, &expected, key);
        }
    }

    if let Some(provider_id) = expected_provider_id {
        let expected_provider = expected
            .get("model_providers")
            .and_then(toml_edit::Item::as_table_like)
            .and_then(|providers| providers.get(&provider_id))
            .cloned();

        if target.get("model_providers").is_none() {
            target["model_providers"] = toml_edit::table();
        }
        let providers = target
            .get_mut("model_providers")
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or_else(|| AppError::Config("Codex model_providers must be a table".to_string()))?;
        match expected_provider {
            Some(provider) => {
                providers.insert(&provider_id, provider);
            }
            None => {
                providers.remove(&provider_id);
            }
        }
        if providers.is_empty() {
            target.as_table_mut().remove("model_providers");
        }
    }

    Ok(target.to_string())
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
        runtime_activation: runtime_activation_from_system(),
    }
}

fn runtime_activation_from_evidence(
    app_server_started_at_ms: Option<i64>,
    config_modified_at_ms: Option<i64>,
    app_server_started_at: Option<String>,
    config_modified_at: Option<String>,
    detection_error: Option<String>,
) -> CodexConfigRuntimeActivation {
    let (state, reason) = if detection_error.is_some() {
        (
            CodexConfigRuntimeActivationState::Unknown,
            Some("app_server_detection_failed".to_string()),
        )
    } else if app_server_started_at_ms.is_none() {
        (CodexConfigRuntimeActivationState::NotRunning, None)
    } else if config_modified_at_ms.is_none() {
        (
            CodexConfigRuntimeActivationState::Unknown,
            Some("managed_config_timestamp_unavailable".to_string()),
        )
    } else if app_server_started_at_ms < config_modified_at_ms {
        (
            CodexConfigRuntimeActivationState::RestartRequired,
            Some("app_server_started_before_managed_config".to_string()),
        )
    } else {
        (CodexConfigRuntimeActivationState::Current, None)
    };

    CodexConfigRuntimeActivation {
        state,
        app_server_started_at,
        config_modified_at,
        reason,
    }
}

fn system_time_to_millis(time: SystemTime) -> Option<i64> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_millis()).ok()
}

fn runtime_activation_from_system() -> CodexConfigRuntimeActivation {
    let (processes, detection_error) = crate::commands::query_codex_processes();
    let newest_app_server = processes
        .iter()
        .filter(|process| process.is_app_server)
        .filter_map(|process| {
            let text = process.started_at.as_deref()?;
            let parsed = chrono::DateTime::parse_from_rfc3339(text).ok()?;
            Some((parsed.timestamp_millis(), text.to_string()))
        })
        .max_by_key(|(timestamp, _)| *timestamp);
    let config_modified = fs::metadata(codex_config::get_codex_config_path())
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| {
            let timestamp = system_time_to_millis(time)?;
            let datetime: chrono::DateTime<chrono::Local> = time.into();
            Some((timestamp, datetime.to_rfc3339()))
        });

    runtime_activation_from_evidence(
        newest_app_server.as_ref().map(|(timestamp, _)| *timestamp),
        config_modified.as_ref().map(|(timestamp, _)| *timestamp),
        newest_app_server.map(|(_, text)| text),
        config_modified.map(|(_, text)| text),
        detection_error,
    )
}

pub(crate) fn should_emit_startup_event(state: CodexConfigConsistencyState) -> bool {
    state == CodexConfigConsistencyState::ExternalDrift
}

fn should_emit_report(report: &CodexConfigConsistencyReport) -> bool {
    should_emit_startup_event(report.state)
        || report.runtime_activation.state == CodexConfigRuntimeActivationState::RestartRequired
}

fn repair_current_profile_provider_before_inspection(state: &AppState) -> Result<(), AppError> {
    let Some(provider_id) =
        crate::settings::get_effective_current_provider(&state.db, &AppType::Codex)?
    else {
        return Ok(());
    };
    crate::services::provider::ProviderService::sync_active_profile_provider_snapshot(
        state,
        &AppType::Codex,
        &provider_id,
    )
}

/// Run the one-shot live-config reconciliation after startup writers and proxy
/// recovery have settled. Emitting an event is deliberately best-effort for
/// startup: a UI listener can query the same report as a race-free fallback.
pub(crate) async fn reconcile_after_startup(app: &tauri::AppHandle) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    if repair_current_profile_provider_before_inspection(&state).is_err() {
        log::warn!(
            "Codex 当前 Provider 已恢复，但活跃项目快照仍无法同步，error_kind=profile_provider_snapshot_sync_failed"
        );
    }
    let report = inspect(&state)?;
    if should_emit_report(&report) {
        app.emit("codex-config-consistency", &report)
            .map_err(|error| {
                AppError::Message(format!("Codex config consistency event failed: {error}"))
            })?;
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

    let live_takeover_active = state
        .proxy_service
        .detect_takeover_in_live_config_for_app(&AppType::Codex);
    let takeover_expected =
        futures::executor::block_on(state.db.get_proxy_config_for_app(AppType::Codex.as_str()))
            .map(|config| config.enabled)
            .unwrap_or(false);
    if live_takeover_active {
        return Ok(report(
            CodexConfigConsistencyState::NotApplicable,
            Some(provider_id),
            None,
            None,
            Vec::new(),
            Some("proxy_takeover_active"),
        ));
    }

    if takeover_expected {
        let live_path = codex_config::get_codex_config_path();
        let actual_text = match fs::read_to_string(&live_path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(report(
                    CodexConfigConsistencyState::Unavailable,
                    Some(provider_id),
                    None,
                    None,
                    Vec::new(),
                    Some("live_config_missing"),
                ));
            }
            Err(error) => return Err(AppError::io(&live_path, error)),
        };
        let actual_provider_id = active_model_provider_id_from_text(&actual_text)?;
        let actual_fingerprint = managed_fingerprint(&actual_text, actual_provider_id.as_deref())?;
        return Ok(report(
            CodexConfigConsistencyState::ExternalDrift,
            Some(provider_id),
            None,
            Some(actual_fingerprint),
            vec![
                "model_provider".to_string(),
                format!(
                    "model_providers.{}",
                    crate::codex_config::CC_SWITCH_CODEX_ROUTER_MODEL_PROVIDER_ID
                ),
            ],
            Some("takeover_projection_drift"),
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
    let expected_provider_id = active_model_provider_id_from_text(&expected_text)?;
    let expected_fingerprint =
        managed_fingerprint(&expected_text, expected_provider_id.as_deref())?;

    let live_path = codex_config::get_codex_config_path();
    let actual_text = match fs::read_to_string(&live_path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(report(
                CodexConfigConsistencyState::Unavailable,
                Some(provider_id),
                Some(expected_fingerprint),
                None,
                Vec::new(),
                Some("live_config_missing"),
            ));
        }
        Err(error) => return Err(AppError::io(&live_path, error)),
    };

    let actual_fingerprint =
        match managed_fingerprint(&actual_text, expected_provider_id.as_deref()) {
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
        changed_managed_key_paths(
            &expected_text,
            &actual_text,
            expected_provider_id.as_deref(),
        )?,
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
            if current.reason.as_deref() == Some("takeover_projection_drift") {
                return Err(AppError::InvalidInput(
                    "cannot_keep_codex_changes_while_takeover_enabled".to_string(),
                ));
            }
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
                let candidate = build_codex_live_config_for_provider(&state.db, &provider)?;
                let provider_id_hint = active_model_provider_id_from_text(&candidate)?;
                let observed = managed_fingerprint(before, provider_id_hint.as_deref())?;
                if observed != expected_fingerprint {
                    return Err(AppError::InvalidInput(
                        "codex_config_consistency_stale_fingerprint".to_string(),
                    ));
                }
                if !backup_created {
                    fs::write(&backup_path, before.as_bytes())
                        .map_err(|error| AppError::io(&backup_path, error))?;
                    backup_created = true;
                }
                merge_ccsm_owned_projection(before, &candidate)
            })?;
            inspect(state)
        }
        CodexConfigConsistencyAction::Later => Ok(current),
    }
}

async fn resolve_for_command(
    state: &AppState,
    expected_fingerprint: String,
    action: CodexConfigConsistencyAction,
) -> Result<CodexConfigConsistencyReport, AppError> {
    let current = inspect(state)?;
    if current.reason.as_deref() != Some("takeover_projection_drift") {
        return resolve(state, expected_fingerprint, action);
    }
    if action == CodexConfigConsistencyAction::Later {
        return Ok(current);
    }
    if action == CodexConfigConsistencyAction::KeepCodex {
        return Err(AppError::InvalidInput(
            "cannot_keep_codex_changes_while_takeover_enabled".to_string(),
        ));
    }
    let actual_fingerprint = current.actual_fingerprint.as_deref().ok_or_else(|| {
        AppError::Config("Codex live configuration fingerprint is missing".to_string())
    })?;
    if actual_fingerprint != expected_fingerprint {
        return Err(AppError::InvalidInput(
            "codex_config_consistency_stale_fingerprint".to_string(),
        ));
    }

    state
        .proxy_service
        .set_takeover_for_app(AppType::Codex.as_str(), true)
        .await
        .map_err(AppError::Message)?;
    inspect(state)
}

#[tauri::command]
pub fn inspect_codex_config_consistency(
    state: State<'_, AppState>,
) -> Result<CodexConfigConsistencyReport, String> {
    inspect(&state).map_err(String::from)
}

#[tauri::command]
pub async fn resolve_codex_config_consistency(
    state: State<'_, AppState>,
    expected_fingerprint: String,
    action: CodexConfigConsistencyAction,
) -> Result<CodexConfigConsistencyReport, String> {
    resolve_for_command(&state, expected_fingerprint, action)
        .await
        .map_err(String::from)
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
                "config": "model = \"gpt-5.5\"\nmodel_provider = \"ccsm-provider\"\nmodel_reasoning_effort = \"medium\"\n\n[model_providers.ccsm-provider]\nname = \"CCSM Provider\"\nbase_url = \"https://example.test/v1\"\nwire_api = \"responses\"\n",
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
    fn runtime_activation_requires_restart_when_app_server_predates_managed_config() {
        let activation = runtime_activation_from_evidence(
            Some(1_788_186_514_000),
            Some(1_788_189_533_000),
            Some("2026-08-31T21:48:34+08:00".to_string()),
            Some("2026-08-31T22:38:53+08:00".to_string()),
            None,
        );

        assert_eq!(
            activation.state,
            CodexConfigRuntimeActivationState::RestartRequired
        );
        assert_eq!(
            activation.reason.as_deref(),
            Some("app_server_started_before_managed_config")
        );
    }

    #[test]
    fn runtime_activation_is_current_after_app_server_restart() {
        let activation = runtime_activation_from_evidence(
            Some(1_788_190_000_000),
            Some(1_788_189_533_000),
            Some("2026-08-31T22:46:40+08:00".to_string()),
            Some("2026-08-31T22:38:53+08:00".to_string()),
            None,
        );

        assert_eq!(activation.state, CodexConfigRuntimeActivationState::Current);
        assert!(activation.reason.is_none());
    }

    #[test]
    #[serial]
    fn inspect_reports_only_ccsm_owned_route_drift() {
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
        live["model_providers"]["ccsm-provider"]["wire_api"] = toml_edit::value("chat");
        live["agents"]["max_threads"] = toml_edit::value(16);
        live["desktop"]["composerPlainTextMode"] = toml_edit::value(true);
        live["projects"][r"c:\users\sunda\documents\new-project"]["trust_level"] =
            toml_edit::value("trusted");
        codex_config::write_codex_live_config_atomic(Some(&live.to_string()))
            .expect("write drifted live config");

        let result = inspect(&state).expect("inspect config consistency");

        assert_eq!(result.state, CodexConfigConsistencyState::ExternalDrift);
        assert_eq!(result.provider_id.as_deref(), Some("consistency-provider"));
        assert_eq!(
            result.changed_keys,
            vec!["model_providers.ccsm-provider.wire_api"]
        );
    }

    #[test]
    #[serial]
    fn inspect_ignores_codex_desktop_runtime_and_user_owned_changes() {
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
        live["developer_instructions"] = toml_edit::value("updated by the user");
        live["agents"]["max_threads"] = toml_edit::value(16);
        live["desktop"]["composerPlainTextMode"] = toml_edit::value(true);
        live["mcp_servers"]["node_repl"]["command"] =
            toml_edit::value(r"C:\Program Files\Codex\node_repl.exe");
        live["mcp_servers"]["cua_repl"]["command"] =
            toml_edit::value(r"C:\Program Files\Codex\cua_repl.exe");
        live["mcp_servers"]["node_repl"]["env"]["BROWSER_USE_CODEX_APP_VERSION"] =
            toml_edit::value("26.825.6671.0");
        live["mcp_servers"]["node_repl"]["env"]["BROWSER_USE_TINYSKY_ENABLED"] =
            toml_edit::value("true");
        live["mcp_servers"]["node_repl"]["env"]["CODEX_CLI_PATH"] =
            toml_edit::value(r"C:\Program Files\Codex\codex.exe");
        live["mcp_servers"]["node_repl"]["env"]["NODE_REPL_NODE_MODULE_DIRS"] =
            toml_edit::value(r"C:\Program Files\Codex\node_modules");
        live["mcp_servers"]["node_repl"]["env"]["NODE_REPL_NODE_PATH"] =
            toml_edit::value(r"C:\Program Files\Codex\node.exe");
        live["mcp_servers"]["node_repl"]["env"]["NODE_REPL_TRUSTED_CODE_PATHS"] =
            toml_edit::value(r"C:\Program Files\Codex");
        live["mcp_servers"]["node_repl"]["env"]["NODE_REPL_TRUSTED_SERVICES"] =
            toml_edit::value("codex-app");
        live["mcp_servers"]["node_repl"]["env"]["SKY_CUA_NATIVE_PIPE_DIRECTORY"] =
            toml_edit::value(r"C:\Users\sunda\AppData\Local\Temp\codex-cua");
        live["mcp_servers"]["cua_repl"]["enabled"] = toml_edit::value(true);
        live["plugins"]["codex-app-tools@openai-bundled"]["enabled"] = toml_edit::value(true);
        for project in [
            r"c:\users\sunda\appdata\local\temp\ccsm-qwen-coalesce-canary-20260817-1355",
            r"c:\users\sunda\documents\chatgpt\acps比赛",
            r"c:\users\sunda\documents\codex\2026-08-24\w",
            r"c:\users\sunda\documents\llm-api-protocol-bridge",
            r"c:\users\sunda\documents\wit和codex与acps adapter",
        ] {
            live["projects"][project]["trust_level"] = toml_edit::value("trusted");
        }
        live["wire_api"] = toml_edit::value("responses");
        codex_config::write_codex_live_config_atomic(Some(&live.to_string()))
            .expect("write Codex-owned changes");

        let result = inspect(&state).expect("inspect config consistency");

        assert_eq!(result.state, CodexConfigConsistencyState::Consistent);
        assert!(result.changed_keys.is_empty());
    }

    #[test]
    #[serial]
    fn inspect_reports_when_qwen_is_left_on_the_chatgpt_account_provider() {
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
        live["model_provider"] = toml_edit::value("openai");
        live["agents"]["max_threads"] = toml_edit::value(16);
        codex_config::write_codex_live_config_atomic(Some(&live.to_string()))
            .expect("write provider drift");

        let result = inspect(&state).expect("inspect config consistency");

        assert_eq!(result.state, CodexConfigConsistencyState::ExternalDrift);
        assert_eq!(result.changed_keys, vec!["model_provider"]);
    }

    #[test]
    #[serial]
    fn inspect_treats_missing_live_router_as_takeover_projection_drift() {
        let _home = TestHomeGuard::new();
        crate::settings::reload_settings().expect("reload settings");
        let (state, _) = seed_provider();
        let provider = state
            .db
            .get_provider_by_id("consistency-provider", AppType::Codex.as_str())
            .expect("read provider")
            .expect("provider exists");
        let direct_config = build_codex_live_config_for_provider(&state.db, &provider)
            .expect("build direct provider config");
        codex_config::write_codex_live_config_atomic(Some(&direct_config))
            .expect("write non-router live config");
        let mut proxy_config =
            futures::executor::block_on(state.db.get_proxy_config_for_app(AppType::Codex.as_str()))
                .expect("read proxy config");
        proxy_config.enabled = true;
        futures::executor::block_on(state.db.update_proxy_config_for_app(proxy_config))
            .expect("mark takeover enabled");

        let result = inspect(&state).expect("inspect takeover projection");

        assert_eq!(result.state, CodexConfigConsistencyState::ExternalDrift);
        assert_eq!(result.reason.as_deref(), Some("takeover_projection_drift"));
        assert!(result
            .changed_keys
            .iter()
            .any(|key| key == "model_provider"));
        let actual = result.actual_fingerprint.expect("actual fingerprint");
        let error = resolve(&state, actual, CodexConfigConsistencyAction::KeepCodex)
            .expect_err("takeover drift cannot be accepted as an external edit");
        assert!(error
            .to_string()
            .contains("cannot_keep_codex_changes_while_takeover_enabled"));
    }

    #[test]
    #[serial]
    fn startup_inspection_repairs_a_legacy_stale_profile_provider() {
        let _home = TestHomeGuard::new();
        crate::settings::reload_settings().expect("reload settings");
        let (state, _) = seed_provider();
        state
            .db
            .save_provider(
                AppType::Codex.as_str(),
                &Provider::with_id(
                    "old-provider".to_string(),
                    "Old provider".to_string(),
                    json!({}),
                    None,
                ),
            )
            .expect("save old provider");
        state
            .db
            .save_profile(&crate::database::Profile {
                id: "workspace".to_string(),
                name: "Workspace".to_string(),
                payload: json!({
                    "providers": {"codex": "old-provider"},
                    "mcp": {"codex": ["matrix"]}
                })
                .to_string(),
                sort_order: None,
                created_at: Some(1),
                updated_at: Some(1),
            })
            .expect("save stale profile");
        state
            .db
            .set_current_profile_id("codex", Some("workspace"))
            .expect("activate stale profile");

        repair_current_profile_provider_before_inspection(&state)
            .expect("repair startup provider ownership");

        let profile = state
            .db
            .get_profile("workspace")
            .expect("read profile")
            .expect("profile exists");
        let payload: serde_json::Value =
            serde_json::from_str(&profile.payload).expect("parse payload");
        assert_eq!(payload["providers"]["codex"], "consistency-provider");
        assert_eq!(payload["mcp"]["codex"][0], "matrix");
    }

    #[test]
    #[serial]
    fn inspect_reports_a_changed_ccsm_model_catalog_pointer() {
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
        live["model_catalog_json"] = toml_edit::value(r"C:\other\catalog.json");
        codex_config::write_codex_live_config_atomic(Some(&live.to_string()))
            .expect("write changed catalog pointer");

        let result = inspect(&state).expect("inspect config consistency");

        assert_eq!(result.state, CodexConfigConsistencyState::ExternalDrift);
        assert_eq!(result.changed_keys, vec!["model_catalog_json"]);
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
        live["model_providers"]["ccsm-provider"]["wire_api"] = toml_edit::value("chat");
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
        let live_path = codex_config::get_codex_config_path();
        let mut live = fs::read_to_string(&live_path)
            .expect("read drifted live config")
            .parse::<toml_edit::DocumentMut>()
            .expect("parse drifted live config");
        live["desktop"]["composerPlainTextMode"] = toml_edit::value(true);
        live["mcp_servers"]["node_repl"]["env"]["BROWSER_USE_CODEX_APP_VERSION"] =
            toml_edit::value("26.825.6671.0");
        live["projects"][r"c:\users\sunda\documents\new-project"]["trust_level"] =
            toml_edit::value("trusted");
        codex_config::write_codex_live_config_atomic(Some(&live.to_string()))
            .expect("write unmanaged live fields");
        let before = inspect(&state).expect("inspect drift");
        let actual = before
            .actual_fingerprint
            .clone()
            .expect("actual fingerprint");

        let after = resolve(&state, actual, CodexConfigConsistencyAction::ApplyCcsm)
            .expect("apply CCSM config");

        assert_eq!(after.state, CodexConfigConsistencyState::Consistent);
        let applied = fs::read_to_string(&live_path)
            .expect("read applied live config")
            .parse::<toml::Value>()
            .expect("parse applied live config");
        assert_eq!(
            applied
                .get("desktop")
                .and_then(|desktop| desktop.get("composerPlainTextMode"))
                .and_then(toml::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            applied
                .get("mcp_servers")
                .and_then(|servers| servers.get("node_repl"))
                .and_then(|server| server.get("env"))
                .and_then(|env| env.get("BROWSER_USE_CODEX_APP_VERSION"))
                .and_then(toml::Value::as_str),
            Some("26.825.6671.0")
        );
        assert_eq!(
            applied
                .get("projects")
                .and_then(|projects| projects.get(r"c:\users\sunda\documents\new-project"))
                .and_then(|project| project.get("trust_level"))
                .and_then(toml::Value::as_str),
            Some("trusted")
        );
        assert_eq!(
            applied
                .get("model_providers")
                .and_then(|providers| providers.get("ccsm-provider"))
                .and_then(|provider| provider.get("wire_api"))
                .and_then(toml::Value::as_str),
            Some("responses")
        );
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
    fn startup_reconciliation_emits_for_disk_drift_or_stale_runtime() {
        let states = [
            CodexConfigConsistencyState::Consistent,
            CodexConfigConsistencyState::NotApplicable,
            CodexConfigConsistencyState::Unavailable,
        ];
        assert!(states
            .iter()
            .all(|state| !should_emit_startup_event(*state)));
        assert!(should_emit_startup_event(
            CodexConfigConsistencyState::ExternalDrift
        ));
        let runtime_only = CodexConfigConsistencyReport {
            state: CodexConfigConsistencyState::Consistent,
            provider_id: Some("router".to_string()),
            expected_fingerprint: Some("same".to_string()),
            actual_fingerprint: Some("same".to_string()),
            changed_keys: Vec::new(),
            reason: None,
            runtime_activation: CodexConfigRuntimeActivation {
                state: CodexConfigRuntimeActivationState::RestartRequired,
                app_server_started_at: Some("2026-08-31T21:48:34+08:00".to_string()),
                config_modified_at: Some("2026-08-31T22:38:53+08:00".to_string()),
                reason: Some("app_server_started_before_managed_config".to_string()),
            },
        };
        assert!(should_emit_report(&runtime_only));
    }
}
