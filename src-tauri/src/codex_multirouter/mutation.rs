use super::active_codex_router_id;
use super::compiler::{compile_v2, compile_v2_strict};
use super::projection::{
    build_projection_artifact, ensure_projection_with_publisher, CodexRoutingProjectionArtifact,
    CodexRoutingProjectionStatus, ProjectionReadBack,
};
use super::schema::CodexRoutingDocument;
use crate::database::Database;
use crate::error::AppError;
use crate::protocol_compatibility::ProtocolCompatibilityRecord;
use crate::provider::Provider;
use rusqlite::params;
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct CodexProviderMutationOutcome {
    pub projections: Vec<CodexRoutingProjectionStatus>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderDeleteOutcome {
    pub deleted_provider_id: String,
    pub affected_plan_ids: Vec<String>,
    pub disabled_plan_ids: Vec<String>,
    pub removed_candidates: Vec<String>,
    pub projections: Vec<CodexRoutingProjectionStatus>,
}

struct PreparedRouterDeletion {
    router_id: String,
    settings_config: serde_json::Value,
    disabled: bool,
}

struct AffectedRouterIds {
    projection: Vec<String>,
    subagent_profiles: Vec<String>,
}

pub fn apply_codex_provider_mutation(
    db: &Database,
    provider: Provider,
) -> Result<CodexProviderMutationOutcome, AppError> {
    apply_codex_provider_mutation_with_profiles_and_publisher(db, provider, &[], |artifact| {
        crate::codex_config::publish_codex_multirouter_projection(&artifact.projection_settings)
            .map_err(|error| error.to_string())
    })
}

pub fn apply_codex_provider_mutation_with_profile(
    db: &Database,
    provider: Provider,
    profile: &ProtocolCompatibilityRecord,
) -> Result<CodexProviderMutationOutcome, AppError> {
    apply_codex_provider_mutation_with_profiles(db, provider, std::slice::from_ref(profile))
}

pub fn apply_codex_provider_mutation_with_profiles(
    db: &Database,
    provider: Provider,
    profiles: &[ProtocolCompatibilityRecord],
) -> Result<CodexProviderMutationOutcome, AppError> {
    apply_codex_provider_mutation_with_profiles_and_publisher(db, provider, profiles, |artifact| {
        crate::codex_config::publish_codex_multirouter_projection(&artifact.projection_settings)
            .map_err(|error| error.to_string())
    })
}

pub fn apply_codex_provider_mutation_with_publisher<F>(
    db: &Database,
    provider: Provider,
    publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    apply_codex_provider_mutation_with_profiles_and_publisher(db, provider, &[], publish)
}

fn apply_codex_provider_mutation_with_profiles_and_publisher<F>(
    db: &Database,
    provider: Provider,
    profiles: &[ProtocolCompatibilityRecord],
    mut publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    let mut provider = provider;
    remove_schema_v2_router_derived_catalog(&mut provider);
    let affected_router_ids = validate_and_collect_affected_router_ids(db, &provider)?;
    if profiles.is_empty() {
        db.save_provider("codex", &provider)?;
    } else {
        db.save_provider_with_protocol_profiles("codex", &provider, profiles)?;
    }

    // Router 的 V2 profile 是用户可编辑策略，但候选成员来自 Provider 目录。
    // Provider 新增可路由第三方模型时自动补一个关闭的 profile，保留已有问卷和覆盖；
    // 模型移除时不静默删除用户配置，仍由 unroutable 状态显式呈现。
    for router_id in &affected_router_ids.subagent_profiles {
        sync_router_subagent_profiles_from_provider_catalog(db, router_id)?;
    }

    let active_router_id = active_codex_router_id(db)?;
    let publish_without_active_router =
        active_router_id.is_none() && affected_router_ids.projection.len() == 1;
    let mut projections = Vec::with_capacity(affected_router_ids.projection.len());
    for router_id in affected_router_ids.projection {
        let owns_shared_projection = active_router_id.as_deref() == Some(router_id.as_str())
            || publish_without_active_router;
        if !owns_shared_projection {
            continue;
        }
        projections.push(ensure_projection_with_publisher(
            db,
            &router_id,
            false,
            |artifact| publish(artifact),
        )?);
    }
    Ok(CodexProviderMutationOutcome { projections })
}

fn sync_router_subagent_profiles_from_provider_catalog(
    db: &Database,
    router_id: &str,
) -> Result<(), AppError> {
    let mut router = db.get_provider_by_id(router_id, "codex")?.ok_or_else(|| {
        AppError::Message(format!("Codex MultiRouter provider not found: {router_id}"))
    })?;
    let Some(current) = router
        .settings_config
        .pointer("/codexRouting/subagentV2")
        .cloned()
    else {
        return Ok(());
    };
    let effective = build_projection_artifact(db, router_id)?.projection_settings;
    let provider_context = crate::codex_config::codex_provider_classification_context(db)?;
    let reconciled = crate::codex_config::reconcile_codex_subagent_v2_for_candidate(
        &effective,
        crate::codex_config::CodexSubagentV2ReconcileAction::SyncCatalog,
        Some(&current),
        Some(&provider_context),
    )?;
    if reconciled == current {
        return Ok(());
    }
    router.settings_config["codexRouting"]["subagentV2"] = reconciled;
    db.save_provider("codex", &router)
}

fn remove_schema_v2_router_derived_catalog(provider: &mut Provider) {
    let is_schema_v2_router = provider
        .settings_config
        .pointer("/codexRouting/schemaVersion")
        .and_then(serde_json::Value::as_u64)
        == Some(2);
    if !is_schema_v2_router {
        return;
    }
    remove_derived_catalog_fields(&mut provider.settings_config);
}

fn remove_derived_catalog_fields(settings_config: &mut serde_json::Value) {
    if let Some(settings) = settings_config.as_object_mut() {
        settings.remove("modelCatalog");
        settings.remove("model_catalog");
    }
}

fn validate_and_collect_affected_router_ids(
    db: &Database,
    candidate: &Provider,
) -> Result<AffectedRouterIds, AppError> {
    let mut providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    providers.insert(candidate.id.clone(), candidate.clone());

    let mut projection = Vec::new();
    let mut subagent_profiles = Vec::new();
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
        if routing.get("subagentV2").is_some() {
            subagent_profiles.push(router.id.clone());
        }
        if !plan.enabled {
            continue;
        }
        let compile_result = if router.id == candidate.id {
            compile_v2_strict(&plan, &providers)
        } else {
            compile_v2(&plan, &providers)
        };
        compile_result.map_err(|error| {
            AppError::InvalidInput(format!("{}: {}", error.code, error.message))
        })?;
        projection.push(router.id.clone());
    }
    projection.sort();
    subagent_profiles.sort();
    Ok(AffectedRouterIds {
        projection,
        subagent_profiles,
    })
}

pub fn apply_codex_provider_delete_with_hooks<R, F>(
    db: &Database,
    provider_id: &str,
    current_provider_id: Option<&str>,
    restore_official: R,
    mut publish: F,
) -> Result<CodexProviderDeleteOutcome, AppError>
where
    R: FnOnce() -> Result<(), AppError>,
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    let providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    if !providers.contains_key(provider_id) {
        return Err(AppError::InvalidInput(format!(
            "Codex provider does not exist: {provider_id}"
        )));
    }

    let mut prepared = Vec::new();
    let mut removed_candidates = BTreeSet::new();
    for router in providers.values() {
        if router.id == provider_id {
            continue;
        }
        let Some(routing) = router.settings_config.get("codexRouting") else {
            continue;
        };
        let references_deleted_provider = routing
            .get("routes")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|routes| {
                routes.iter().any(|route| {
                    route
                        .get("targetProviderId")
                        .and_then(serde_json::Value::as_str)
                        == Some(provider_id)
                })
            });
        if !references_deleted_provider {
            continue;
        }

        let document = CodexRoutingDocument::parse(routing).map_err(|error| {
            AppError::InvalidInput(format!("{}: {}", error.code, error.message))
        })?;
        let CodexRoutingDocument::V2(mut plan) = document else {
            return Err(AppError::InvalidInput(format!(
                "legacy_route_requires_migration: Provider {provider_id} is referenced by legacy MultiRouter {}",
                router.id
            )));
        };
        let compiled = compile_v2(&plan, &providers).map_err(|error| {
            AppError::InvalidInput(format!("{}: {}", error.code, error.message))
        })?;
        removed_candidates.extend(
            compiled
                .model_catalog
                .iter()
                .filter(|model| model.target_provider_id == provider_id)
                .map(|model| model.visible_model.clone()),
        );
        plan.routes
            .retain(|route| route.target_provider_id != provider_id);
        let disabled = plan.routes.is_empty();
        if disabled {
            plan.enabled = false;
            plan.default_route_id = None;
        } else if plan.default_route_id.as_deref().is_some_and(|default_id| {
            !plan
                .routes
                .iter()
                .any(|route| route.id.eq_ignore_ascii_case(default_id))
        }) {
            plan.default_route_id = plan.routes.first().map(|route| route.id.clone());
        }
        let mut settings_config = router.settings_config.clone();
        settings_config["codexRouting"] = serde_json::to_value(plan).map_err(|error| {
            AppError::Database(format!(
                "Failed to serialize cascaded Codex routes: {error}"
            ))
        })?;
        remove_derived_catalog_fields(&mut settings_config);
        prepared.push(PreparedRouterDeletion {
            router_id: router.id.clone(),
            settings_config,
            disabled,
        });
    }
    prepared.sort_by(|left, right| left.router_id.cmp(&right.router_id));

    let active_router_id = active_codex_router_id(db)?;

    let must_restore_official = current_provider_id.is_some_and(|current| {
        prepared
            .iter()
            .any(|router| router.disabled && router.router_id == current)
    });
    if must_restore_official {
        restore_official()?;
    }

    {
        let mut conn = crate::database::lock_conn!(db.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        for router in &prepared {
            tx.execute(
                "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = 'codex'",
                params![
                    serde_json::to_string(&router.settings_config).map_err(|error| {
                        AppError::Database(format!(
                            "Failed to serialize cascaded Provider settings: {error}"
                        ))
                    })?,
                    router.router_id
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        }
        let deleted = tx
            .execute(
                "DELETE FROM providers WHERE id = ?1 AND app_type = 'codex'",
                params![provider_id],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        if deleted != 1 {
            return Err(AppError::Database(format!(
                "Codex provider deletion changed {deleted} rows instead of 1"
            )));
        }
        tx.execute(
            "DELETE FROM settings WHERE key = ?1",
            params![format!("codex_multirouter_projection:{provider_id}")],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
    }

    let publish_without_active_router = active_router_id.is_none() && prepared.len() == 1;
    let mut projections = Vec::with_capacity(prepared.len());
    for router in &prepared {
        let owns_shared_projection = active_router_id.as_deref() == Some(router.router_id.as_str())
            || publish_without_active_router;
        if !owns_shared_projection || (active_router_id.is_some() && router.disabled) {
            continue;
        }
        projections.push(ensure_projection_with_publisher(
            db,
            &router.router_id,
            false,
            |artifact| publish(artifact),
        )?);
    }

    Ok(CodexProviderDeleteOutcome {
        deleted_provider_id: provider_id.to_string(),
        affected_plan_ids: prepared
            .iter()
            .map(|router| router.router_id.clone())
            .collect(),
        disabled_plan_ids: prepared
            .iter()
            .filter(|router| router.disabled)
            .map(|router| router.router_id.clone())
            .collect(),
        removed_candidates: removed_candidates.into_iter().collect(),
        projections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_multirouter::projection::{ProjectionReadBack, ProjectionState};
    use crate::database::Database;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;
    use std::cell::{Cell, RefCell};

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
    fn schema_v2_router_save_removes_legacy_derived_catalog_storage() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        let mut updated_router = router("router-a", "qwen");
        updated_router.settings_config["modelCatalog"] = json!({
            "models": [{"model": "stale-qwen"}],
            "spawnAgentModels": ["stale-qwen"]
        });

        apply_codex_provider_mutation_with_publisher(&db, updated_router, |artifact| {
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("save schema v2 router");

        let saved = db
            .get_provider_by_id("router-a", "codex")
            .expect("read router")
            .expect("router exists");
        assert!(saved.settings_config.get("modelCatalog").is_none());
        assert!(saved.settings_config.get("model_catalog").is_none());
    }

    #[test]
    fn provider_model_capability_update_rebuilds_the_active_router_projection() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");
        db.set_current_provider("codex", "router-a")
            .expect("activate router");

        let mut updated = target("openai_responses");
        updated.settings_config["modelCatalog"]["models"] = json!([{
            "model": "qwen3.8",
            "displayName": "Qwen 3.8 Updated",
            "contextWindow": 524288,
            "inputModalities": ["text", "image"],
            "reasoning": {
                "schemaVersion": 2,
                "supportStatus": "confirmed_supported",
                "controlKind": "graded",
                "supportedEfforts": ["low", "high", "max"],
                "defaultEffort": "max",
                "disableAllowed": false,
                "upstream": {
                    "format": "string",
                    "parameter": "reasoning_effort",
                    "effortMap": {"low": "low", "high": "high", "max": "max"}
                }
            },
            "codexUltra": {"enabled": true, "providerEffort": "max"}
        }]);
        let published_model = RefCell::new(None);

        apply_codex_provider_mutation_with_publisher(&db, updated, |artifact| {
            *published_model.borrow_mut() = artifact
                .projection_settings
                .pointer("/modelCatalog/models/0")
                .cloned();
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("apply capability mutation");

        let model = published_model
            .into_inner()
            .expect("active router projection model");
        assert_eq!(model["displayName"], "Qwen 3.8 Updated");
        assert_eq!(model["contextWindow"], 524_288);
        assert_eq!(model["inputModalities"], json!(["text", "image"]));
        assert_eq!(
            model["reasoning"]["supportedEfforts"],
            json!(["low", "high", "max"])
        );
        assert_eq!(model["reasoning"]["defaultEffort"], "max");
        assert_eq!(model["codexUltra"]["providerEffort"], "max");
        assert_eq!(model["apiFormat"], "openai_responses");
    }

    #[test]
    fn provider_model_addition_automatically_adds_a_disabled_v2_subagent_profile() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_responses"))
            .expect("seed target");
        let mut router = router("router-a", "qwen");
        router.settings_config["codexRouting"]["subagentVersion"] = json!("v2");
        router.settings_config["codexRouting"]["subagentV2"] = json!({
            "schemaVersion": 2,
            "selectionPolicy": "balanced",
            "profiles": {
                "qwen3.8": {
                    "model": "qwen3.8",
                    "enabled": false,
                    "questionnaire": {
                        "taskStrengths": ["repository_exploration"],
                        "optimization": "balanced",
                        "writeScope": "read_only",
                        "preference": "eligible"
                    },
                    "reasoning": {"policy": "delegated"}
                }
            }
        });
        db.save_provider("codex", &router).expect("seed router");

        let mut updated = target("openai_responses");
        updated.settings_config["modelCatalog"]["models"] =
            json!([{"model": "qwen3.8"}, {"model": "qwen3.9"}]);
        apply_codex_provider_mutation_with_publisher(&db, updated, |artifact| {
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("apply Provider model addition");

        let saved = db
            .get_provider_by_id("router-a", "codex")
            .expect("read router")
            .expect("router exists");
        assert_eq!(
            saved.settings_config["codexRouting"]["subagentV2"]["profiles"]["qwen3.9"]["enabled"],
            false
        );
    }

    #[test]
    fn provider_model_addition_syncs_profiles_for_a_disabled_router_without_publishing() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_responses"))
            .expect("seed target");
        let mut router = router("router-disabled", "qwen");
        router.settings_config["codexRouting"]["enabled"] = json!(false);
        router.settings_config["codexRouting"]["subagentVersion"] = json!("v2");
        router.settings_config["codexRouting"]["subagentV2"] = json!({
            "schemaVersion": 2,
            "selectionPolicy": "balanced",
            "profiles": {}
        });
        db.save_provider("codex", &router)
            .expect("seed disabled router");

        let mut updated = target("openai_responses");
        updated.settings_config["modelCatalog"]["models"] =
            json!([{"model": "qwen3.8"}, {"model": "qwen3.9"}]);
        apply_codex_provider_mutation_with_publisher(&db, updated, |_| {
            panic!("a disabled Router must not publish a live projection")
        })
        .expect("sync disabled Router profiles");

        let saved = db
            .get_provider_by_id("router-disabled", "codex")
            .expect("read router")
            .expect("router exists");
        assert_eq!(
            saved.settings_config["codexRouting"]["subagentV2"]["profiles"]["qwen3.9"]["enabled"],
            false
        );
    }

    #[test]
    fn provider_model_removal_preserves_include_policy_and_publishes_current_intersection() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_responses"))
            .expect("seed target");
        let mut router = router("router-a", "qwen");
        router.settings_config["codexRouting"]["routes"][0]["modelSelection"] =
            json!({"mode": "include", "models": ["qwen3.8"]});
        db.save_provider("codex", &router).expect("seed router");
        db.set_current_provider("codex", "router-a")
            .expect("activate router");
        let mut updated = target("openai_responses");
        updated.settings_config["modelCatalog"]["models"] = json!([]);
        let warning_seen = Cell::new(false);

        apply_codex_provider_mutation_with_publisher(&db, updated, |artifact| {
            assert!(artifact.compiled.model_catalog.is_empty());
            warning_seen.set(
                artifact
                    .compiled
                    .warnings
                    .iter()
                    .any(|warning| warning.code == "selected_model_unavailable"),
            );
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("Provider fact update must not be blocked by stale include policy");

        assert!(warning_seen.get());
        let saved_router = db
            .get_provider_by_id("router-a", "codex")
            .expect("read router")
            .expect("router remains");
        assert_eq!(
            saved_router.settings_config["codexRouting"]["routes"][0]["modelSelection"],
            json!({"mode": "include", "models": ["qwen3.8"]})
        );
    }

    #[test]
    fn router_save_strictly_rejects_new_unavailable_include_references() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_responses"))
            .expect("seed target");
        let mut router = router("router-a", "qwen");
        router.settings_config["codexRouting"]["routes"][0]["modelSelection"] =
            json!({"mode": "include", "models": ["missing-model"]});

        let error = apply_codex_provider_mutation_with_publisher(&db, router, |_| {
            panic!("invalid Router must not publish")
        })
        .expect_err("new invalid Router policy must fail strict validation");

        assert!(error.to_string().contains("selected_model_missing"));
        assert!(db
            .get_provider_by_id("router-a", "codex")
            .expect("read rejected router")
            .is_none());
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

    #[test]
    fn deleting_target_cascades_routes_and_disables_an_empty_current_plan() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");
        let restore_calls = Cell::new(0);
        let published = RefCell::new(Vec::new());

        let outcome = apply_codex_provider_delete_with_hooks(
            &db,
            "qwen",
            Some("router-a"),
            || {
                restore_calls.set(restore_calls.get() + 1);
                Ok(())
            },
            |artifact| {
                published
                    .borrow_mut()
                    .push(artifact.router_provider_id.clone());
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            },
        )
        .expect("delete target");

        assert_eq!(restore_calls.get(), 1);
        assert!(db
            .get_provider_by_id("qwen", "codex")
            .expect("read target")
            .is_none());
        let saved_router = db
            .get_provider_by_id("router-a", "codex")
            .expect("read router")
            .expect("router remains");
        assert_eq!(
            saved_router.settings_config["codexRouting"]["enabled"],
            false
        );
        assert_eq!(
            saved_router.settings_config["codexRouting"]["routes"],
            json!([])
        );
        assert!(saved_router.settings_config["codexRouting"]
            .get("defaultRouteId")
            .is_none());
        assert!(saved_router.settings_config.get("modelCatalog").is_none());
        assert!(saved_router.settings_config.get("model_catalog").is_none());
        assert_eq!(outcome.affected_plan_ids, vec!["router-a"]);
        assert_eq!(outcome.disabled_plan_ids, vec!["router-a"]);
        assert_eq!(outcome.removed_candidates, vec!["qwen3.8"]);
        assert_eq!(published.into_inner(), vec!["router-a"]);
    }

    #[test]
    fn failed_official_restore_leaves_provider_and_routes_unchanged() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");

        let error = apply_codex_provider_delete_with_hooks(
            &db,
            "qwen",
            Some("router-a"),
            || Err(AppError::Message("restore failed".to_string())),
            |_| panic!("failed restore must not publish"),
        )
        .expect_err("restore failure must abort deletion");

        assert!(error.to_string().contains("restore failed"));
        assert!(db
            .get_provider_by_id("qwen", "codex")
            .expect("read target")
            .is_some());
        assert_eq!(
            db.get_provider_by_id("router-a", "codex")
                .expect("read router")
                .expect("router exists")
                .settings_config["codexRouting"]["routes"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn deleting_provider_referenced_by_legacy_route_requires_explicit_migration() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        let mut legacy_router = router("legacy-router", "qwen");
        legacy_router.settings_config["codexRouting"]
            .as_object_mut()
            .expect("routing object")
            .remove("schemaVersion");
        db.save_provider("codex", &legacy_router)
            .expect("seed legacy router");

        let error = apply_codex_provider_delete_with_hooks(
            &db,
            "qwen",
            None,
            || panic!("legacy dependency must block before restore"),
            |_| panic!("legacy dependency must block before publish"),
        )
        .expect_err("legacy route requires explicit migration");

        assert!(error
            .to_string()
            .contains("legacy_route_requires_migration"));
        assert!(db
            .get_provider_by_id("qwen", "codex")
            .expect("read target")
            .is_some());
    }

    #[test]
    fn shared_provider_mutation_publishes_only_the_active_router() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed shared target");
        db.save_provider("codex", &router("router-personal", "qwen"))
            .expect("seed personal router");
        db.save_provider("codex", &router("router-company", "qwen"))
            .expect("seed company router");

        let profile_id = "profile-personal".to_string();
        db.save_profile(&crate::database::Profile {
            id: profile_id.clone(),
            name: "Personal".to_string(),
            payload: r#"{"providers":{"codex":"router-personal"}}"#.to_string(),
            sort_order: None,
            created_at: Some(1),
            updated_at: Some(1),
        })
        .expect("seed profile");
        db.set_current_profile_id("codex", Some(&profile_id))
            .expect("set active profile");

        let published = RefCell::new(Vec::new());
        apply_codex_provider_mutation_with_publisher(&db, target("openai_responses"), |artifact| {
            published
                .borrow_mut()
                .push(artifact.router_provider_id.clone());
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("apply shared provider mutation");

        assert_eq!(published.into_inner(), vec!["router-personal".to_string()]);
    }

    #[test]
    fn shared_provider_mutation_without_active_router_does_not_publish_ambiguous_projection() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed shared target");
        db.save_provider("codex", &router("router-personal", "qwen"))
            .expect("seed personal router");
        db.save_provider("codex", &router("router-company", "qwen"))
            .expect("seed company router");

        let published = RefCell::new(Vec::new());
        let outcome = apply_codex_provider_mutation_with_publisher(
            &db,
            target("openai_responses"),
            |artifact| {
                published
                    .borrow_mut()
                    .push(artifact.router_provider_id.clone());
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            },
        )
        .expect("persist shared target without choosing a router");

        assert!(published.into_inner().is_empty());
        assert!(outcome.projections.is_empty());
    }

    #[test]
    fn device_local_router_selection_overrides_stale_database_current_provider() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &router("router-personal", "qwen"))
            .expect("save personal router");
        db.save_provider("codex", &router("router-company", "qwen"))
            .expect("save company router");
        db.set_current_provider("codex", "router-company")
            .expect("seed stale database current provider");

        assert_eq!(
            super::super::active_codex_router_id_with_local(&db, Some("router-personal"))
                .expect("resolve active router"),
            Some("router-personal".to_string())
        );
    }

    #[test]
    fn shared_provider_delete_publishes_only_the_active_remaining_router() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed shared target");

        let mut personal = Provider::with_id(
            "router-personal".to_string(),
            "Personal".to_string(),
            json!({
                "auth": {},
                "codexRouting": {
                    "schemaVersion": 2,
                    "enabled": true,
                    "routes": [
                        {
                            "id": "shared",
                            "enabled": true,
                            "targetProviderId": "qwen",
                            "modelSelection": {"mode": "all"},
                            "authPolicy": {"source": "provider_config"}
                        },
                        {
                            "id": "personal-fallback",
                            "enabled": true,
                            "targetProviderId": "personal-only",
                            "modelSelection": {"mode": "all"},
                            "authPolicy": {"source": "provider_config"}
                        }
                    ]
                }
            }),
            None,
        );
        personal.settings_config["modelCatalog"] = json!({"models": [{"model": "stale-personal"}]});
        let mut company = Provider::with_id(
            "router-company".to_string(),
            "Company".to_string(),
            json!({
                "auth": {},
                "codexRouting": {
                    "schemaVersion": 2,
                    "enabled": true,
                    "routes": [
                        {
                            "id": "shared",
                            "enabled": true,
                            "targetProviderId": "qwen",
                            "modelSelection": {"mode": "all"},
                            "authPolicy": {"source": "provider_config"}
                        },
                        {
                            "id": "company-fallback",
                            "enabled": true,
                            "targetProviderId": "company-only",
                            "modelSelection": {"mode": "all"},
                            "authPolicy": {"source": "provider_config"}
                        }
                    ]
                }
            }),
            None,
        );
        company.settings_config["model_catalog"] = json!({"models": [{"model": "stale-company"}]});
        db.save_provider("codex", &personal)
            .expect("save personal router");
        db.save_provider("codex", &company)
            .expect("save company router");
        let personal_target = Provider::with_id(
            "personal-only".to_string(),
            "Personal only".to_string(),
            json!({"modelCatalog": {"models": [{"model": "personal-model"}]}}),
            None,
        );
        let company_target = Provider::with_id(
            "company-only".to_string(),
            "Company only".to_string(),
            json!({"modelCatalog": {"models": [{"model": "company-model"}]}}),
            None,
        );
        db.save_provider("codex", &personal_target)
            .expect("save personal fallback target");
        db.save_provider("codex", &company_target)
            .expect("save company fallback target");
        db.set_current_provider("codex", "router-personal")
            .expect("set active router");

        let published = RefCell::new(Vec::new());
        apply_codex_provider_delete_with_hooks(
            &db,
            "qwen",
            Some("router-personal"),
            || Ok(()),
            |artifact| {
                published
                    .borrow_mut()
                    .push(artifact.router_provider_id.clone());
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            },
        )
        .expect("delete shared target");

        assert_eq!(published.into_inner(), vec!["router-personal".to_string()]);
        for router_id in ["router-personal", "router-company"] {
            let saved = db
                .get_provider_by_id(router_id, "codex")
                .expect("read cascaded router")
                .expect("cascaded router remains");
            assert!(saved.settings_config.get("modelCatalog").is_none());
            assert!(saved.settings_config.get("model_catalog").is_none());
        }
    }
}
