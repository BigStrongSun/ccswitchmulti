use super::compiler::{
    compile_v2, compile_v2_strict, CompiledCodexModel, CompiledCodexRoutingPlan,
};
use super::schema::{CodexRouteAuthSource, CodexRoutingDocument};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const PROJECTION_SETTING_PREFIX: &str = "codex_multirouter_projection:";

fn projection_publish_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionState {
    Ready,
    Pending,
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionCapabilitySources {
    pub context_window: String,
    pub input_modalities: String,
    pub reasoning: String,
    pub codex_cache: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionRouteDiagnostic {
    pub route_id: String,
    pub route_label: Option<String>,
    pub target_provider_id: String,
    pub target_provider_name: String,
    pub visible_model: String,
    pub canonical_model: String,
    pub upstream_model: String,
    pub api_format: String,
    pub api_format_source: String,
    pub auth_owner: String,
    pub capability_sources: ProjectionCapabilitySources,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRoutingProjectionStatus {
    pub schema_version: u32,
    pub router_provider_id: String,
    pub state: ProjectionState,
    pub dependency_fingerprint: String,
    pub generated_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<ProjectionRouteDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodexRoutingProjectionArtifact {
    pub router_provider_id: String,
    pub dependency_fingerprint: String,
    pub projection_settings: Value,
    pub compiled: CompiledCodexRoutingPlan,
    target_provider_names: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionReadBack {
    pub dependency_fingerprint: String,
    pub catalog_verified: bool,
    pub config_verified: bool,
    pub cache_verified: bool,
    pub agent_files_verified: bool,
}

impl ProjectionReadBack {
    pub fn verified(dependency_fingerprint: String) -> Self {
        Self {
            dependency_fingerprint,
            catalog_verified: true,
            config_verified: true,
            cache_verified: true,
            agent_files_verified: true,
        }
    }

    fn agrees_with(&self, expected: &str) -> bool {
        self.dependency_fingerprint == expected
            && self.catalog_verified
            && self.config_verified
            && self.cache_verified
            && self.agent_files_verified
    }
}

pub fn ensure_codex_multirouter_projection(
    db: &Database,
    router_provider_id: &str,
    force: bool,
) -> Result<CodexRoutingProjectionStatus, AppError> {
    if super::active_codex_router_id(db)?.as_deref() != Some(router_provider_id) {
        return Err(AppError::InvalidInput(format!(
            "codex_multirouter_projection_not_active: activate Router {router_provider_id} before publishing its shared live projection"
        )));
    }
    ensure_projection_with_publisher(db, router_provider_id, force, |artifact| {
        crate::codex_config::publish_codex_multirouter_projection_for_database(
            db,
            &artifact.projection_settings,
        )
        .map_err(|error| error.to_string())
    })
}

pub fn inspect_codex_multirouter_projection(
    db: &Database,
    router_provider_id: &str,
) -> Result<CodexRoutingProjectionStatus, AppError> {
    let artifact = build_projection_artifact(db, router_provider_id)?;
    if super::active_codex_router_id(db)?.as_deref() != Some(router_provider_id) {
        return Ok(projection_status(
            &artifact,
            ProjectionState::NotRequired,
            Some("projection_inactive"),
            Some("This Router is inactive; its shared live projection will be generated when activated"),
        ));
    }
    match crate::codex_config::read_back_codex_multirouter_projection_for_database(
        db,
        &artifact.projection_settings,
    ) {
        Ok(read_back) if read_back.agrees_with(&artifact.dependency_fingerprint) => Ok(
            projection_status(&artifact, ProjectionState::Ready, None, None),
        ),
        Ok(_) => Ok(projection_status(
            &artifact,
            ProjectionState::Pending,
            Some("projection_live_drift"),
            Some("Codex live catalog, config, cache, or managed Agent files differ from the current Provider-derived projection"),
        )),
        Err(_) => Ok(projection_status(
            &artifact,
            ProjectionState::Pending,
            Some("projection_readback_failed"),
            Some("Codex live projection could not be read back; retry is available"),
        )),
    }
}

pub fn inspect_active_codex_multirouter_projection(
    db: &Database,
) -> Result<Option<CodexRoutingProjectionStatus>, AppError> {
    let Some(provider_id) = super::active_codex_router_id(db)? else {
        return Ok(None);
    };
    let Some(provider) = db.get_provider_by_id(&provider_id, "codex")? else {
        return Ok(None);
    };
    let is_schema_v2_router = provider
        .settings_config
        .get("codexRouting")
        .and_then(|value| super::schema::CodexRoutingDocument::parse(value).ok())
        .is_some_and(|document| matches!(document, super::schema::CodexRoutingDocument::V2(_)));
    if !is_schema_v2_router {
        return Ok(None);
    }
    inspect_codex_multirouter_projection(db, &provider_id).map(Some)
}

pub fn ensure_projection_with_publisher<F>(
    db: &Database,
    router_provider_id: &str,
    force: bool,
    mut publish: F,
) -> Result<CodexRoutingProjectionStatus, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    // The catalog/config/cache files are one shared projection. Build, publish,
    // read back and persist status under the same process-wide boundary so an
    // older concurrent rebuild cannot finish after and overwrite newer state.
    let _publish_guard = projection_publish_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let artifact = build_projection_artifact(db, router_provider_id)?;
    if !force {
        if let Some(status) = read_projection_status(db, router_provider_id)? {
            if status.state == ProjectionState::Ready
                && status.dependency_fingerprint == artifact.dependency_fingerprint
            {
                return Ok(status);
            }
        }
    }

    let status = match publish(&artifact) {
        Ok(read_back) if read_back.agrees_with(&artifact.dependency_fingerprint) => {
            projection_status(&artifact, ProjectionState::Ready, None, None)
        }
        Ok(_) => projection_status(
            &artifact,
            ProjectionState::Pending,
            Some("projection_readback_mismatch"),
            Some("Codex MultiRouter projection read-back did not match the current Provider dependencies; retry is available"),
        ),
        Err(_) => projection_status(
            &artifact,
            ProjectionState::Pending,
            Some("projection_publish_failed"),
            Some("Codex MultiRouter projection publish failed; the database remains authoritative and retry is available"),
        ),
    };
    write_projection_status(db, &status)?;
    Ok(status)
}

pub fn read_projection_status(
    db: &Database,
    router_provider_id: &str,
) -> Result<Option<CodexRoutingProjectionStatus>, AppError> {
    let Some(serialized) = db.get_setting(&projection_setting_key(router_provider_id))? else {
        return Ok(None);
    };
    serde_json::from_str(&serialized)
        .map(Some)
        .map_err(|error| {
            AppError::Database(format!(
            "Failed to parse Codex MultiRouter projection status for {router_provider_id}: {error}"
        ))
        })
}

fn write_projection_status(
    db: &Database,
    status: &CodexRoutingProjectionStatus,
) -> Result<(), AppError> {
    let serialized = serde_json::to_string(status).map_err(|error| {
        AppError::Database(format!(
            "Failed to serialize Codex MultiRouter projection status: {error}"
        ))
    })?;
    db.set_setting(
        &projection_setting_key(&status.router_provider_id),
        &serialized,
    )
}

fn projection_setting_key(router_provider_id: &str) -> String {
    format!("{PROJECTION_SETTING_PREFIX}{router_provider_id}")
}

pub(crate) fn build_projection_artifact(
    db: &Database,
    router_provider_id: &str,
) -> Result<CodexRoutingProjectionArtifact, AppError> {
    let router = db
        .get_provider_by_id(router_provider_id, "codex")?
        .ok_or_else(|| {
            AppError::Message(format!(
                "Codex MultiRouter provider not found: {router_provider_id}"
            ))
        })?;
    let routing = router
        .settings_config
        .get("codexRouting")
        .ok_or_else(|| AppError::Message("Provider does not contain codexRouting".to_string()))?;
    let document = CodexRoutingDocument::parse(routing)
        .map_err(|error| AppError::Message(format!("{}: {}", error.code, error.message)))?;
    let CodexRoutingDocument::V2(plan) = document else {
        return Err(AppError::Message(
            "Codex MultiRouter projection requires schemaVersion 2".to_string(),
        ));
    };
    let providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let compiled = compile_v2(&plan, &providers)
        .map_err(|error| AppError::Message(format!("{}: {}", error.code, error.message)))?;
    let projection_settings = projection_settings(&router, &compiled);
    let target_provider_names = providers
        .iter()
        .map(|(id, provider)| (id.clone(), provider.name.clone()))
        .collect();
    Ok(CodexRoutingProjectionArtifact {
        router_provider_id: router.id,
        dependency_fingerprint: compiled.dependency_fingerprint.clone(),
        projection_settings,
        compiled,
        target_provider_names,
    })
}

/// Materialize Provider-owned model facts for schema-v2 consumers without persisting the
/// derived catalog back into the Router record.
pub(crate) fn effective_settings_for_candidate(
    db: &Database,
    candidate: &Provider,
    strict_router_references: bool,
) -> Result<Value, AppError> {
    let mut providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    providers.insert(candidate.id.clone(), candidate.clone());
    effective_settings_for_candidate_with_providers(candidate, strict_router_references, &providers)
}

/// Build the Router's effective settings against an already prepared Provider snapshot.
///
/// Provider mutations use this before opening the SQLite transaction so dependent V2
/// subagent profile changes can be committed atomically with the Provider facts that produced
/// them. Callers must include `candidate` and every referenced target Provider in `providers`.
pub(crate) fn effective_settings_for_candidate_with_providers(
    candidate: &Provider,
    strict_router_references: bool,
    providers: &HashMap<String, Provider>,
) -> Result<Value, AppError> {
    let Some(routing) = candidate.settings_config.get("codexRouting") else {
        return Ok(candidate.settings_config.clone());
    };
    let document = CodexRoutingDocument::parse(routing)
        .map_err(|error| AppError::InvalidInput(format!("{}: {}", error.code, error.message)))?;
    let CodexRoutingDocument::V2(plan) = document else {
        return Ok(candidate.settings_config.clone());
    };
    let compiled = if strict_router_references {
        compile_v2_strict(&plan, providers)
    } else {
        compile_v2(&plan, providers)
    }
    .map_err(|error| AppError::InvalidInput(format!("{}: {}", error.code, error.message)))?;
    Ok(projection_settings(candidate, &compiled))
}

fn projection_settings(router: &Provider, compiled: &CompiledCodexRoutingPlan) -> Value {
    let mut settings = router.settings_config.clone();
    let models = projected_model_entries(compiled);
    settings["modelCatalog"] = json!({
        "models": models,
        "spawnAgentModels": compiled.spawn_agent_models
    });
    settings["codexRoutingProjection"] = json!({
        "dependencyFingerprint": compiled.dependency_fingerprint
    });
    settings
}

/// Merge only fields owned by the compiled projection into effective/live settings.
/// Common config, authentication, and other user-owned fields must stay untouched.
pub(crate) fn apply_projection_owned_settings(target: &mut Value, projection: &Value) {
    for key in ["modelCatalog", "codexRoutingProjection"] {
        if let Some(value) = projection.get(key).cloned() {
            target[key] = value;
        }
    }
}

fn projected_model_entries(compiled: &CompiledCodexRoutingPlan) -> Vec<Value> {
    let has_source_custom_order = compiled
        .model_catalog
        .iter()
        .any(|model| model.sort_index.is_some());

    let mut indexed = compiled
        .model_catalog
        .iter()
        .enumerate()
        .collect::<Vec<_>>();
    if has_source_custom_order {
        indexed.sort_by_key(|(index, model)| (model.sort_index.unwrap_or(usize::MAX), *index));
    }

    indexed
        .into_iter()
        .enumerate()
        .map(|(sort_index, (_, model))| {
            projected_model_entry(model, has_source_custom_order.then_some(sort_index))
        })
        .collect()
}

fn projected_model_entry(model: &CompiledCodexModel, sort_index: Option<usize>) -> Value {
    let mut entry = Map::new();
    entry.insert(
        "model".to_string(),
        Value::String(model.visible_model.clone()),
    );
    if let Some(sort_index) = sort_index {
        entry.insert("sortIndex".to_string(), Value::from(sort_index));
    }
    entry.insert(
        "upstreamModel".to_string(),
        Value::String(model.upstream_model.clone()),
    );
    entry.insert(
        "displayName".to_string(),
        Value::String(model.display_name.clone()),
    );
    entry.insert(
        "apiFormat".to_string(),
        Value::String(model.api_format.clone()),
    );
    if let Some(context_window) = model.capability_summary.context_window {
        entry.insert("contextWindow".to_string(), Value::from(context_window));
    }
    if !model.capability_summary.input_modalities.is_empty() {
        entry.insert(
            "inputModalities".to_string(),
            Value::from(model.capability_summary.input_modalities.clone()),
        );
    }
    if let Some(reasoning) = model.capability_summary.reasoning.clone() {
        entry.insert("reasoning".to_string(), reasoning);
    }
    if let Some(cache) = model.capability_summary.codex_cache.clone() {
        entry.insert("codexCache".to_string(), cache);
    }
    if let Some(supports_parallel_tool_calls) =
        model.capability_summary.supports_parallel_tool_calls
    {
        entry.insert(
            "supportsParallelToolCalls".to_string(),
            Value::Bool(supports_parallel_tool_calls),
        );
    }
    if let Some(base_instructions) = model.capability_summary.base_instructions.clone() {
        entry.insert(
            "baseInstructions".to_string(),
            Value::String(base_instructions),
        );
    }
    if let Some(codex_ultra) = model.capability_summary.codex_ultra.clone() {
        entry.insert("codexUltra".to_string(), codex_ultra);
    }
    Value::Object(entry)
}

fn projection_status(
    artifact: &CodexRoutingProjectionArtifact,
    state: ProjectionState,
    error_code: Option<&str>,
    error: Option<&str>,
) -> CodexRoutingProjectionStatus {
    let providers = artifact
        .compiled
        .routes
        .iter()
        .map(|route| (route.id.as_str(), route))
        .collect::<HashMap<_, _>>();
    let routes = artifact
        .compiled
        .model_catalog
        .iter()
        .filter_map(|model| {
            let route = providers.get(model.route_id.as_str())?;
            Some(ProjectionRouteDiagnostic {
                route_id: model.route_id.clone(),
                route_label: route.label.clone(),
                target_provider_id: model.target_provider_id.clone(),
                target_provider_name: artifact
                    .target_provider_names
                    .get(&model.target_provider_id)
                    .cloned()
                    .unwrap_or_else(|| model.target_provider_id.clone()),
                visible_model: model.visible_model.clone(),
                canonical_model: model.canonical_model.clone(),
                upstream_model: model.upstream_model.clone(),
                api_format: model.api_format.clone(),
                api_format_source: model.api_format_source.clone(),
                auth_owner: auth_owner(route.auth_policy.source).to_string(),
                capability_sources: ProjectionCapabilitySources {
                    context_window: model.capability_summary.context_window_source.clone(),
                    input_modalities: model.capability_summary.input_modalities_source.clone(),
                    reasoning: model.capability_summary.reasoning_source.clone(),
                    codex_cache: model.capability_summary.codex_cache_source.clone(),
                },
            })
        })
        .collect();
    CodexRoutingProjectionStatus {
        schema_version: 1,
        router_provider_id: artifact.router_provider_id.clone(),
        state,
        dependency_fingerprint: artifact.dependency_fingerprint.clone(),
        generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        warnings: artifact
            .compiled
            .warnings
            .iter()
            .map(|warning| warning.message.clone())
            .collect(),
        routes,
        last_error_code: error_code.map(str::to_string),
        last_error: error.map(str::to_string),
    }
}

fn auth_owner(source: CodexRouteAuthSource) -> &'static str {
    match source {
        CodexRouteAuthSource::ProviderConfig => "provider_config",
        CodexRouteAuthSource::ManagedAccount => "managed_account",
        CodexRouteAuthSource::ManagedCodexOauth => "managed_codex_oauth",
        CodexRouteAuthSource::NativeCodexAuth => "native_codex_auth",
        CodexRouteAuthSource::AccountPool => "account_pool",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;
    use std::cell::Cell;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    fn target(api_format: &str) -> Provider {
        let mut provider = Provider::with_id(
            "qwen".to_string(),
            "Qwen".to_string(),
            json!({
                "base_url": "https://qwen.example/v1",
                "auth": {"OPENAI_API_KEY": "secret-must-not-leak"},
                "modelCatalog": {"models": [{
                    "model": "qwen3.8",
                    "inputModalities": ["text"],
                    "contextWindow": 262144
                }]}
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some(api_format.to_string()),
            ..Default::default()
        });
        provider
    }

    fn router() -> Provider {
        Provider::with_id(
            "router".to_string(),
            "MultiRouter".to_string(),
            json!({
                "codexRouting": {
                    "schemaVersion": 2,
                    "enabled": true,
                    "spawnAgentModels": ["qwen3.8", "removed"],
                    "defaultRouteId": "qwen",
                    "routes": [{
                        "id": "qwen",
                        "label": "Qwen route",
                        "enabled": true,
                        "targetProviderId": "qwen",
                        "modelSelection": {"mode": "all"},
                        "authPolicy": {"source": "provider_config"}
                    }]
                }
            }),
            None,
        )
    }

    #[test]
    fn effective_candidate_settings_compile_provider_catalog_without_router_snapshot() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_responses"))
            .expect("save target");
        let router = router();
        assert!(router.settings_config.get("modelCatalog").is_none());

        let effective = effective_settings_for_candidate(&db, &router, true)
            .expect("compile effective settings");

        assert_eq!(
            effective.pointer("/modelCatalog/models/0/model"),
            Some(&json!("qwen3.8"))
        );
        assert_eq!(
            effective.pointer("/modelCatalog/models/0/contextWindow"),
            Some(&json!(262144))
        );
        assert_eq!(
            effective.pointer("/modelCatalog/spawnAgentModels"),
            Some(&json!(["qwen3.8"]))
        );
    }

    fn save_fixture(db: &Database, api_format: &str) {
        db.save_provider("codex", &router()).expect("save router");
        db.save_provider("codex", &target(api_format))
            .expect("save target");
    }

    #[test]
    fn fingerprint_mismatch_rebuilds_projection_from_latest_provider() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");
        let calls = Cell::new(0);
        let first = ensure_projection_with_publisher(&db, "router", false, |artifact| {
            calls.set(calls.get() + 1);
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("first projection");
        assert_eq!(first.state, ProjectionState::Ready);
        assert_eq!(calls.get(), 1);

        let unchanged = ensure_projection_with_publisher(&db, "router", false, |_| {
            panic!("matching ready projection must not be republished")
        })
        .expect("unchanged projection");
        assert_eq!(
            unchanged.dependency_fingerprint,
            first.dependency_fingerprint
        );

        db.save_provider("codex", &target("openai_responses"))
            .expect("update target");
        let changed = ensure_projection_with_publisher(&db, "router", false, |artifact| {
            calls.set(calls.get() + 1);
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("changed projection");
        assert_eq!(changed.state, ProjectionState::Ready);
        assert_ne!(changed.dependency_fingerprint, first.dependency_fingerprint);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn projection_keeps_only_routable_spawn_agent_models() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");
        let artifact = build_projection_artifact(&db, "router").expect("projection artifact");

        assert_eq!(
            artifact.projection_settings["modelCatalog"]["spawnAgentModels"],
            json!(["qwen3.8"])
        );
    }

    #[test]
    fn projection_ignores_stale_router_catalog_order_and_uses_provider_order() {
        let db = Database::memory().expect("memory db");
        let mut router = router();
        router.settings_config["modelCatalog"] = json!({
            "models": [
                {"model": "qwen-b", "sortIndex": 0},
                {"model": "qwen-a", "sortIndex": 1}
            ],
            "spawnAgentModels": ["qwen-b"]
        });
        let target = Provider::with_id(
            "qwen".to_string(),
            "Qwen".to_string(),
            json!({
                "base_url": "https://qwen.example/v1",
                "modelCatalog": {"models": [
                    {"model": "qwen-a", "sortIndex": 0},
                    {"model": "qwen-b", "sortIndex": 1}
                ]}
            }),
            None,
        );
        db.save_provider("codex", &router)
            .expect("save router with custom order");
        db.save_provider("codex", &target)
            .expect("save target with custom order");

        let artifact = build_projection_artifact(&db, "router").expect("projection artifact");
        let models = artifact.projection_settings["modelCatalog"]["models"]
            .as_array()
            .expect("projected models");
        assert_eq!(
            models
                .iter()
                .map(|model| model["model"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["qwen-a", "qwen-b"]
        );
        assert_eq!(
            models
                .iter()
                .map(|model| model["sortIndex"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn projection_uses_source_sort_indexes_when_router_has_no_custom_order() {
        let db = Database::memory().expect("memory db");
        let router = router();
        let target = Provider::with_id(
            "qwen".to_string(),
            "Qwen".to_string(),
            json!({
                "base_url": "https://qwen.example/v1",
                "modelCatalog": {"models": [
                    {"model": "qwen-a", "sortIndex": 1},
                    {"model": "qwen-b", "sortIndex": 0}
                ]}
            }),
            None,
        );
        db.save_provider("codex", &router)
            .expect("save router without custom order");
        db.save_provider("codex", &target)
            .expect("save target with source order");

        let artifact = build_projection_artifact(&db, "router").expect("projection artifact");
        let models = artifact.projection_settings["modelCatalog"]["models"]
            .as_array()
            .expect("projected models");
        assert_eq!(
            models
                .iter()
                .map(|model| model["model"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["qwen-b", "qwen-a"]
        );
        assert_eq!(
            models
                .iter()
                .map(|model| model["sortIndex"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn projection_drops_sort_indexes_when_custom_order_is_reset() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");
        let artifact = build_projection_artifact(&db, "router").expect("projection artifact");
        let models = artifact.projection_settings["modelCatalog"]["models"]
            .as_array()
            .expect("projected models");

        assert!(models.iter().all(|model| {
            model
                .as_object()
                .is_some_and(|entry| !entry.contains_key("sortIndex"))
        }));
    }

    #[test]
    fn publish_failure_persists_pending_and_retry_recovers() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");

        let pending = ensure_projection_with_publisher(&db, "router", false, |_| {
            Err("injected catalog write failure".to_string())
        })
        .expect("pending status is diagnostic, not lost");
        assert_eq!(pending.state, ProjectionState::Pending);
        assert_eq!(
            pending.last_error_code.as_deref(),
            Some("projection_publish_failed")
        );
        assert!(!pending
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("secret"));
        assert_eq!(
            read_projection_status(&db, "router")
                .expect("read status")
                .expect("stored pending")
                .state,
            ProjectionState::Pending
        );

        let ready = ensure_projection_with_publisher(&db, "router", true, |artifact| {
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("retry projection");
        assert_eq!(ready.state, ProjectionState::Ready);
        assert!(ready.last_error.is_none());
    }

    #[test]
    fn readback_fingerprint_mismatch_is_pending_not_ready() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");

        let status = ensure_projection_with_publisher(&db, "router", false, |_| {
            Ok(ProjectionReadBack::verified(
                "stale-fingerprint".to_string(),
            ))
        })
        .expect("mismatch status");

        assert_eq!(status.state, ProjectionState::Pending);
        assert_eq!(
            status.last_error_code.as_deref(),
            Some("projection_readback_mismatch")
        );
    }

    #[test]
    fn diagnostics_are_secret_free_and_explain_effective_sources() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");
        let status = ensure_projection_with_publisher(&db, "router", false, |artifact| {
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("projection");
        let serialized = serde_json::to_string(&status).expect("serialize diagnostics");

        assert!(!serialized.contains("secret-must-not-leak"));
        assert!(!serialized.to_ascii_lowercase().contains("api_key"));
        assert_eq!(status.routes[0].target_provider_id, "qwen");
        assert_eq!(status.routes[0].target_provider_name, "Qwen");
        assert_eq!(status.routes[0].canonical_model, "qwen3.8");
        assert_eq!(status.routes[0].api_format, "openai_chat");
        assert_eq!(status.routes[0].api_format_source, "provider");
        assert_eq!(status.routes[0].auth_owner, "provider_config");
        assert_eq!(
            status.routes[0].capability_sources.context_window,
            "provider_model"
        );
    }

    #[test]
    fn inspect_reports_inactive_router_as_not_requiring_shared_projection() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");

        let inactive = inspect_codex_multirouter_projection(&db, "router")
            .expect("inspect inactive projection");
        assert_eq!(inactive.state, ProjectionState::NotRequired);
        assert_eq!(
            inactive.last_error_code.as_deref(),
            Some("projection_inactive")
        );
        assert!(read_projection_status(&db, "router")
            .expect("read status")
            .is_none());
    }

    #[test]
    fn inspect_active_projection_returns_none_for_a_direct_provider() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");
        db.set_current_provider("codex", "qwen")
            .expect("make direct provider current");

        assert!(inspect_active_codex_multirouter_projection(&db)
            .expect("inspect current provider")
            .is_none());
    }

    #[test]
    fn public_projection_publish_rejects_an_inactive_router() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");
        db.set_current_provider("codex", "qwen")
            .expect("make target provider current instead of router");

        let error = ensure_codex_multirouter_projection(&db, "router", true)
            .expect_err("inactive router must not overwrite the shared live projection");

        assert!(error
            .to_string()
            .contains("codex_multirouter_projection_not_active"));
    }

    #[test]
    fn concurrent_projection_rebuilds_publish_in_database_order() {
        let db = Arc::new(Database::memory().expect("memory db"));
        save_fixture(&db, "openai_chat");
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();

        let first_db = db.clone();
        let first = std::thread::spawn(move || {
            ensure_projection_with_publisher(&first_db, "router", true, |artifact| {
                first_entered_tx
                    .send(artifact.dependency_fingerprint.clone())
                    .expect("signal first publisher");
                release_first_rx.recv().expect("release first publisher");
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            })
            .expect("first projection")
        });

        let first_fingerprint = first_entered_rx.recv().expect("first publisher entered");
        db.save_provider("codex", &target("openai_responses"))
            .expect("update target");

        let second_db = db.clone();
        let second = std::thread::spawn(move || {
            ensure_projection_with_publisher(&second_db, "router", true, |artifact| {
                second_entered_tx
                    .send(artifact.dependency_fingerprint.clone())
                    .expect("signal second publisher");
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            })
            .expect("second projection")
        });

        let second_published_before_first_released = second_entered_rx
            .recv_timeout(Duration::from_millis(150))
            .ok();
        release_first_tx.send(()).expect("release first publisher");
        let first_status = first.join().expect("join first publisher");
        let second_status = second.join().expect("join second publisher");

        assert!(
            second_published_before_first_released.is_none(),
            "a newer projection must wait until the older publisher has completed"
        );
        assert_eq!(first_status.dependency_fingerprint, first_fingerprint);
        assert_ne!(
            second_status.dependency_fingerprint,
            first_status.dependency_fingerprint
        );
        assert_eq!(
            read_projection_status(&db, "router")
                .expect("read projection status")
                .expect("projection status")
                .dependency_fingerprint,
            second_status.dependency_fingerprint
        );
    }

    #[test]
    fn projected_models_preserve_display_name_from_source_catalog() {
        let db = Database::memory().expect("memory db");
        let router = router();
        let mut target = Provider::with_id(
            "qwen".to_string(),
            "Qwen".to_string(),
            json!({
                "base_url": "https://qwen.example/v1",
                "modelCatalog": {"models": [{
                    "model": "qwen3.8",
                    "displayName": "Qwen 3.8",
                    "inputModalities": ["text"],
                    "contextWindow": 262144,
                    "reasoning": {
                        "schemaVersion": 2,
                        "supportStatus": "confirmed_supported",
                        "controlKind": "graded",
                        "supportedEfforts": ["low", "high"],
                        "defaultEffort": "high",
                        "disableAllowed": false,
                        "upstream": {"format": "string", "parameter": "reasoning_effort"}
                    },
                    "codexCache": {"cacheMode": "qwen_context_cache"},
                    "supportsParallelToolCalls": true,
                    "baseInstructions": "Use Qwen tools.",
                    "codexUltra": {"enabled": true, "providerEffort": "high"}
                }]}
            }),
            None,
        );
        target.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        db.save_provider("codex", &router).expect("save router");
        db.save_provider("codex", &target).expect("save target");

        let artifact = build_projection_artifact(&db, "router").expect("projection artifact");
        let models = artifact.projection_settings["modelCatalog"]["models"]
            .as_array()
            .expect("projected models");
        assert_eq!(models[0]["displayName"].as_str(), Some("Qwen 3.8"));
        assert_eq!(models[0]["contextWindow"].as_u64(), Some(262_144));
        assert_eq!(models[0]["inputModalities"], json!(["text"]));
        assert_eq!(models[0]["reasoning"]["defaultEffort"], "high");
        assert_eq!(models[0]["codexCache"]["cacheMode"], "qwen_context_cache");
        assert_eq!(models[0]["supportsParallelToolCalls"], true);
        assert_eq!(models[0]["baseInstructions"], "Use Qwen tools.");
        assert_eq!(models[0]["codexUltra"]["providerEffort"], "high");
    }

    #[test]
    fn projection_owned_merge_keeps_effective_settings_and_carries_fingerprint() {
        let mut target = json!({
            "auth": {"type": "api_key"},
            "config": "model_provider = \"router\"",
            "modelCatalog": {"models": [{"model": "stale"}]},
            "customUserField": true
        });
        let projection = json!({
            "auth": {"type": "must-not-overwrite"},
            "modelCatalog": {"models": [{"model": "projected", "sortIndex": 0}]},
            "codexRoutingProjection": {"dependencyFingerprint": "fingerprint-v2"}
        });

        apply_projection_owned_settings(&mut target, &projection);

        assert_eq!(target["auth"]["type"], "api_key");
        assert_eq!(target["config"], "model_provider = \"router\"");
        assert_eq!(target["customUserField"], true);
        assert_eq!(target["modelCatalog"], projection["modelCatalog"]);
        assert_eq!(
            target["codexRoutingProjection"],
            projection["codexRoutingProjection"]
        );
    }
}
