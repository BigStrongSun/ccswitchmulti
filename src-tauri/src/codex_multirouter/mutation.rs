use super::compiler::compile_v2;
use super::projection::{
    ensure_projection_with_publisher, CodexRoutingProjectionArtifact, CodexRoutingProjectionStatus,
    ProjectionReadBack,
};
use super::schema::CodexRoutingDocument;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CodexProviderMutationOutcome {
    pub projections: Vec<CodexRoutingProjectionStatus>,
}

pub fn apply_codex_provider_mutation(
    db: &Database,
    provider: Provider,
) -> Result<CodexProviderMutationOutcome, AppError> {
    apply_codex_provider_mutation_with_publisher(db, provider, |artifact| {
        crate::codex_config::publish_codex_multirouter_projection(&artifact.projection_settings)
            .map_err(|error| error.to_string())
    })
}

pub fn apply_codex_provider_mutation_with_publisher<F>(
    db: &Database,
    provider: Provider,
    mut publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    let affected_router_ids = validate_and_collect_affected_router_ids(db, &provider)?;
    db.save_provider("codex", &provider)?;

    let mut projections = Vec::with_capacity(affected_router_ids.len());
    for router_id in affected_router_ids {
        projections.push(ensure_projection_with_publisher(
            db,
            &router_id,
            false,
            |artifact| publish(artifact),
        )?);
    }
    Ok(CodexProviderMutationOutcome { projections })
}

fn validate_and_collect_affected_router_ids(
    db: &Database,
    candidate: &Provider,
) -> Result<Vec<String>, AppError> {
    let mut providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    providers.insert(candidate.id.clone(), candidate.clone());

    let mut affected = Vec::new();
    for router in providers.values() {
        let Some(routing) = router.settings_config.get("codexRouting") else {
            continue;
        };
        let declares_candidate_dependency = router.id == candidate.id
            || routing
                .get("routes")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|routes| {
                    routes.iter().any(|route| {
                        route
                            .get("targetProviderId")
                            .and_then(serde_json::Value::as_str)
                            == Some(candidate.id.as_str())
                    })
                });
        if !declares_candidate_dependency {
            continue;
        }
        let document = CodexRoutingDocument::parse(routing).map_err(|error| {
            AppError::InvalidInput(format!("{}: {}", error.code, error.message))
        })?;
        let CodexRoutingDocument::V2(plan) = document else {
            continue;
        };
        compile_v2(&plan, &providers).map_err(|error| {
            AppError::InvalidInput(format!("{}: {}", error.code, error.message))
        })?;
        affected.push(router.id.clone());
    }
    affected.sort();
    Ok(affected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_multirouter::projection::{ProjectionReadBack, ProjectionState};
    use crate::database::Database;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;
    use std::cell::RefCell;

    fn target(api_format: &str) -> Provider {
        let mut provider = Provider::with_id(
            "qwen".to_string(),
            "Qwen".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "secret"},
                "modelCatalog": {"models": [{"model": "qwen3.8"}]}
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some(api_format.to_string()),
            ..Default::default()
        });
        provider
    }

    fn router(id: &str, target_provider_id: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            format!("Router {id}"),
            json!({
                "auth": {},
                "codexRouting": {
                    "schemaVersion": 2,
                    "enabled": true,
                    "routes": [{
                        "id": "route-qwen",
                        "enabled": true,
                        "targetProviderId": target_provider_id,
                        "modelSelection": {"mode": "all"},
                        "authPolicy": {"source": "provider_config"}
                    }]
                }
            }),
            None,
        )
    }

    #[test]
    fn provider_update_rebuilds_only_affected_v2_projections_without_rewriting_routes() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed affected router");
        db.save_provider("codex", &router("router-b", "other"))
            .expect("seed unrelated router");
        let original_route = db
            .get_provider_by_id("router-a", "codex")
            .expect("read router")
            .expect("router exists")
            .settings_config["codexRouting"]["routes"][0]
            .clone();
        let published = RefCell::new(Vec::new());

        let outcome = apply_codex_provider_mutation_with_publisher(
            &db,
            target("openai_responses"),
            |artifact| {
                published.borrow_mut().push((
                    artifact.router_provider_id.clone(),
                    artifact.compiled.model_catalog[0].api_format.clone(),
                ));
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            },
        )
        .expect("apply provider mutation");

        assert_eq!(
            published.into_inner(),
            vec![("router-a".to_string(), "openai_responses".to_string())]
        );
        assert_eq!(outcome.projections.len(), 1);
        assert_eq!(outcome.projections[0].state, ProjectionState::Ready);
        let saved_route = db
            .get_provider_by_id("router-a", "codex")
            .expect("read router")
            .expect("router exists")
            .settings_config["codexRouting"]["routes"][0]
            .clone();
        assert_eq!(
            saved_route, original_route,
            "Route declaration must not be synchronized with Provider fields"
        );
    }

    #[test]
    fn unrelated_invalid_router_does_not_block_provider_mutation() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        let unrelated = Provider::with_id(
            "legacy-broken".to_string(),
            "Unrelated broken router".to_string(),
            json!({
                "auth": {},
                "codexRouting": {
                    "schemaVersion": 2,
                    "routes": [{
                        "id": "other",
                        "targetProviderId": "other-provider",
                        "modelSelection": {"mode": "all"},
                        "upstream": {"apiFormat": "openai_chat"}
                    }]
                }
            }),
            None,
        );
        db.save_provider("codex", &unrelated)
            .expect("seed unrelated invalid router");

        apply_codex_provider_mutation_with_publisher(&db, target("openai_responses"), |_| {
            panic!("unrelated router must not publish")
        })
        .expect("unrelated invalid router must not block target update");

        assert_eq!(
            db.get_provider_by_id("qwen", "codex")
                .expect("read qwen")
                .expect("qwen exists")
                .meta
                .and_then(|meta| meta.api_format)
                .as_deref(),
            Some("openai_responses")
        );
    }
}
