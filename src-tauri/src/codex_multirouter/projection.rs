use super::compiler::{compile_v2, CompiledCodexModel, CompiledCodexRoutingPlan};
use super::schema::{CodexRouteAuthSource, CodexRoutingDocument};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const PROJECTION_SETTING_PREFIX: &str = "codex_multirouter_projection:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionState {
    Ready,
    Pending,
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
}

impl ProjectionReadBack {
    pub fn verified(dependency_fingerprint: String) -> Self {
        Self {
            dependency_fingerprint,
            catalog_verified: true,
            config_verified: true,
            cache_verified: true,
        }
    }

    fn agrees_with(&self, expected: &str) -> bool {
        self.dependency_fingerprint == expected
            && self.catalog_verified
            && self.config_verified
            && self.cache_verified
    }
}

pub fn ensure_codex_multirouter_projection(
    db: &Database,
    router_provider_id: &str,
    force: bool,
) -> Result<CodexRoutingProjectionStatus, AppError> {
    let artifact = build_projection_artifact(db, router_provider_id)?;
    let provider_context = crate::codex_config::codex_provider_classification_context(db)?;
    let status = ensure_projection_with_publisher(db, router_provider_id, force, |artifact| {
        crate::codex_config::publish_codex_multirouter_projection_with_context(
            &artifact.projection_settings,
            Some(&provider_context),
        )
        .map_err(|error| error.to_string())
    })?;
    if status.state == ProjectionState::Ready {
        write_projected_catalog_to_router(db, router_provider_id, &artifact)?;
    }
    Ok(status)
}

/// MultiRouter 的聚合模型目录是 V2 子 Agent 编译的模型源。
///
/// 投影一直只写入 Live 文件会导致 V2 页面读取的 DB `settingsConfig.modelCatalog`
/// 与最终投影目录分叉：目标供应商新增的模型会出现在 Live catalog，却不会出现在
/// V2 后端列表。投影成功后把同一份聚合 catalog 回写 router DB 行，使 V2 编译、
/// 回读校验和 Live 投影共用同一模型源。
pub(crate) fn write_projected_catalog_to_router(
    db: &Database,
    router_provider_id: &str,
    artifact: &CodexRoutingProjectionArtifact,
) -> Result<(), AppError> {
    let Some(mut provider) = db.get_provider_by_id(router_provider_id, "codex")? else {
        return Ok(());
    };
    let Some(catalog) = artifact.projection_settings.get("modelCatalog") else {
        return Ok(());
    };
    provider.settings_config["modelCatalog"] = catalog.clone();
    db.update_provider_settings_config("codex", router_provider_id, &provider.settings_config)
}

pub fn inspect_codex_multirouter_projection(
    db: &Database,
    router_provider_id: &str,
) -> Result<CodexRoutingProjectionStatus, AppError> {
    let artifact = build_projection_artifact(db, router_provider_id)?;
    match read_projection_status(db, router_provider_id)? {
        Some(status) if status.dependency_fingerprint == artifact.dependency_fingerprint => {
            Ok(status)
        }
        Some(_) => Ok(projection_status(
            &artifact,
            ProjectionState::Pending,
            Some("projection_stale"),
            Some("Codex MultiRouter projection dependencies changed and regeneration is required"),
        )),
        None => Ok(projection_status(
            &artifact,
            ProjectionState::Pending,
            Some("projection_missing"),
            Some("Codex MultiRouter projection has not been generated yet"),
        )),
    }
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

fn projection_settings(router: &Provider, compiled: &CompiledCodexRoutingPlan) -> Value {
    let mut settings = router.settings_config.clone();
    let models = projected_model_entries(router, compiled);
    settings["modelCatalog"] = json!({
        "models": models,
        "spawnAgentModels": compiled.spawn_agent_models
    });
    settings["codexRoutingProjection"] = json!({
        "dependencyFingerprint": compiled.dependency_fingerprint
    });
    settings
}

fn projected_model_entries(router: &Provider, compiled: &CompiledCodexRoutingPlan) -> Vec<Value> {
    let previous_models = router
        .settings_config
        .get("modelCatalog")
        .or_else(|| router.settings_config.get("model_catalog"))
        .and_then(|catalog| catalog.get("models"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let previous_rank = model_ranks(&previous_models);
    let has_previous_custom_order = previous_models
        .iter()
        .any(|model| model_sort_index_value(model).is_some());
    let has_source_custom_order = compiled
        .model_catalog
        .iter()
        .any(|model| model.sort_index.is_some());
    let use_custom_order = has_previous_custom_order || has_source_custom_order;

    let mut indexed = compiled
        .model_catalog
        .iter()
        .enumerate()
        .collect::<Vec<_>>();
    if has_previous_custom_order {
        indexed.sort_by_key(|(index, model)| {
            (
                previous_rank
                    .get(&model.visible_model.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(usize::MAX),
                *index,
            )
        });
    } else if has_source_custom_order {
        indexed.sort_by_key(|(index, model)| (model.sort_index.unwrap_or(usize::MAX), *index));
    }

    indexed
        .into_iter()
        .enumerate()
        .map(|(sort_index, (_, model))| {
            projected_model_entry(model, use_custom_order.then_some(sort_index))
        })
        .collect()
}

fn model_ranks(models: &[Value]) -> HashMap<String, usize> {
    let mut ranked = models
        .iter()
        .enumerate()
        .filter_map(|(index, model)| {
            let name = model
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())?;
            Some((
                name.to_ascii_lowercase(),
                (model_sort_index_value(model), index),
            ))
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(_, (sort_index, index))| (sort_index.unwrap_or(usize::MAX), *index));

    let mut ranks = HashMap::new();
    for (name, _) in ranked {
        let next_rank = ranks.len();
        ranks.entry(name).or_insert(next_rank);
    }
    ranks
}

fn model_sort_index_value(model: &Value) -> Option<usize> {
    model
        .get("sortIndex")
        .or_else(|| model.get("sort_index"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
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
    fn projection_preserves_custom_model_order_and_sort_indexes() {
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
                    {"model": "qwen-a", "sortIndex": 1},
                    {"model": "qwen-b", "sortIndex": 0}
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
    fn inspect_is_read_only_and_reports_missing_or_stale_projection() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_chat");

        let missing = inspect_codex_multirouter_projection(&db, "router")
            .expect("inspect missing projection");
        assert_eq!(missing.state, ProjectionState::Pending);
        assert_eq!(
            missing.last_error_code.as_deref(),
            Some("projection_missing")
        );
        assert!(read_projection_status(&db, "router")
            .expect("read status")
            .is_none());

        ensure_projection_with_publisher(&db, "router", false, |artifact| {
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("seed ready projection");
        db.save_provider("codex", &target("openai_responses"))
            .expect("change dependency");

        let stale =
            inspect_codex_multirouter_projection(&db, "router").expect("inspect stale projection");
        assert_eq!(stale.state, ProjectionState::Pending);
        assert_eq!(stale.last_error_code.as_deref(), Some("projection_stale"));
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
                    "displayName": "Qwen 3.8 大模型",
                    "inputModalities": ["text"],
                    "contextWindow": 262144
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
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["model"].as_str(), Some("qwen3.8"));
        assert_eq!(
            models[0]["displayName"].as_str(),
            Some("Qwen 3.8 大模型"),
            "projected model entry must carry displayName from the source catalog"
        );
    }

    #[test]
    fn projection_persists_projected_catalog_to_router_database() {
        let db = Database::memory().expect("memory db");
        save_fixture(&db, "openai_responses");

        let artifact = build_projection_artifact(&db, "router").expect("projection artifact");
        write_projected_catalog_to_router(&db, "router", &artifact).expect("writeback catalog");

        let router = db
            .get_provider_by_id("router", "codex")
            .expect("read router")
            .expect("router exists");
        let models = router.settings_config["modelCatalog"]["models"]
            .as_array()
            .expect("persisted projected models");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["model"].as_str(), Some("qwen3.8"));
    }
}
