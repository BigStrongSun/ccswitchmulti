use super::compiler::{compile_v2, CompiledCodexModel, CompiledCodexRoutingPlan};
use super::schema::{CodexRouteAuthSource, CodexRoutingDocument};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
    ensure_projection_with_publisher(db, router_provider_id, force, |artifact| {
        crate::codex_config::publish_codex_multirouter_projection(&artifact.projection_settings)
            .map_err(|error| error.to_string())
    })
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

fn build_projection_artifact(
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
    settings["modelCatalog"] = json!({
        "models": compiled.model_catalog.iter().map(projected_model_entry).collect::<Vec<_>>()
    });
    settings["codexRoutingProjection"] = json!({
        "dependencyFingerprint": compiled.dependency_fingerprint
    });
    settings
}

fn projected_model_entry(model: &CompiledCodexModel) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert(
        "model".to_string(),
        Value::String(model.visible_model.clone()),
    );
    entry.insert(
        "upstreamModel".to_string(),
        Value::String(model.upstream_model.clone()),
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
}
