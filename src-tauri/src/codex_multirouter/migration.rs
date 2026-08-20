use super::compiler::compile_v2;
use super::schema::{
    CodexModelSelection, CodexRouteAuthPolicy, CodexRouteAuthSource, CodexRoutingConfigV2,
    CodexRoutingDocument, CodexRoutingRouteV2, CODEX_ROUTING_SCHEMA_V2,
};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use rusqlite::params;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MIGRATION_TOKEN_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationDiff {
    pub removed_route_fields: Vec<String>,
    pub created_provider_ids: Vec<String>,
    pub changed_route_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedProviderSummary {
    pub id: String,
    pub name: String,
    pub migration_generated: bool,
    pub source_provider_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMultiRouterMigrationPreview {
    pub schema_version: u32,
    pub provider_id: String,
    pub expected_revision: String,
    pub plan_token: String,
    pub diff: MigrationDiff,
    pub warnings: Vec<String>,
    pub generated_providers: Vec<GeneratedProviderSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexMultiRouterMigrationApplyOutcome {
    pub provider_id: String,
    pub revision: String,
    pub created_provider_ids: Vec<String>,
    pub already_applied: bool,
}

#[derive(Clone)]
struct StoredMigration {
    expires_at: Instant,
    provider_id: String,
    expected_revision: String,
    migrated_router: Provider,
    generated_providers: Vec<Provider>,
    updated_providers: Vec<Provider>,
    result_revision: Option<String>,
}

struct BuiltMigration {
    migrated_router: Provider,
    generated_providers: Vec<Provider>,
    updated_providers: Vec<Provider>,
    summaries: Vec<GeneratedProviderSummary>,
    diff: MigrationDiff,
    warnings: Vec<String>,
}

fn migration_tokens() -> &'static Mutex<HashMap<String, StoredMigration>> {
    static TOKENS: OnceLock<Mutex<HashMap<String, StoredMigration>>> = OnceLock::new();
    TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn codex_multirouter_revision(db: &Database, provider_id: &str) -> Result<String, AppError> {
    let provider = db
        .get_provider_by_id(provider_id, "codex")?
        .ok_or_else(|| {
            AppError::InvalidInput("Codex MultiRouter provider does not exist".into())
        })?;
    provider_revision(&provider)
}

fn provider_revision(provider: &Provider) -> Result<String, AppError> {
    let bytes = serde_json::to_vec(provider).map_err(|error| {
        AppError::Database(format!("Failed to hash Provider revision: {error}"))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub fn preview_codex_multirouter_migration(
    db: &Database,
    provider_id: &str,
    expected_revision: &str,
) -> Result<CodexMultiRouterMigrationPreview, AppError> {
    let router = db
        .get_provider_by_id(provider_id, "codex")?
        .ok_or_else(|| {
            AppError::InvalidInput("Codex MultiRouter provider does not exist".into())
        })?;
    let actual_revision = provider_revision(&router)?;
    if actual_revision != expected_revision {
        return Err(AppError::InvalidInput("migration_revision_conflict".into()));
    }
    let routing = router
        .settings_config
        .get("codexRouting")
        .ok_or_else(|| AppError::InvalidInput("codexRouting is missing".into()))?;
    if matches!(
        CodexRoutingDocument::parse(routing),
        Ok(CodexRoutingDocument::V2(_))
    ) {
        return Err(AppError::InvalidInput("migration_already_v2".into()));
    }
    let providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let built = build_migration(&router, &providers)?;
    let mut candidate_providers = providers.clone();
    for provider in built
        .generated_providers
        .iter()
        .chain(built.updated_providers.iter())
    {
        candidate_providers.insert(provider.id.clone(), provider.clone());
    }
    candidate_providers.insert(provider_id.to_string(), built.migrated_router.clone());
    let migrated_routing = built
        .migrated_router
        .settings_config
        .get("codexRouting")
        .expect("migration builder always writes codexRouting");
    let CodexRoutingDocument::V2(migrated_plan) = CodexRoutingDocument::parse(migrated_routing)
        .map_err(|error| AppError::InvalidInput(format!("{}: {}", error.code, error.message)))?
    else {
        return Err(AppError::InvalidInput(
            "migration_did_not_produce_v2".into(),
        ));
    };
    compile_v2(&migrated_plan, &candidate_providers)
        .map_err(|error| AppError::InvalidInput(format!("{}: {}", error.code, error.message)))?;
    let token = format!("cmr_{}", uuid::Uuid::new_v4().simple());
    let stored = StoredMigration {
        expires_at: Instant::now() + MIGRATION_TOKEN_TTL,
        provider_id: provider_id.to_string(),
        expected_revision: expected_revision.to_string(),
        migrated_router: built.migrated_router.clone(),
        generated_providers: built.generated_providers.clone(),
        updated_providers: built.updated_providers.clone(),
        result_revision: None,
    };
    migration_tokens()
        .lock()
        .map_err(|_| AppError::Message("migration_token_store_poisoned".into()))?
        .insert(token.clone(), stored);
    Ok(CodexMultiRouterMigrationPreview {
        schema_version: CODEX_ROUTING_SCHEMA_V2,
        provider_id: provider_id.to_string(),
        expected_revision: expected_revision.to_string(),
        plan_token: token,
        diff: built.diff,
        warnings: built.warnings,
        generated_providers: built.summaries,
    })
}

pub fn apply_codex_multirouter_migration(
    db: &Database,
    provider_id: &str,
    expected_revision: &str,
    plan_token: &str,
) -> Result<CodexMultiRouterMigrationApplyOutcome, AppError> {
    let stored = {
        let tokens = migration_tokens()
            .lock()
            .map_err(|_| AppError::Message("migration_token_store_poisoned".into()))?;
        tokens
            .get(plan_token)
            .cloned()
            .ok_or_else(|| AppError::InvalidInput("migration_plan_token_invalid".into()))?
    };
    if stored.expires_at < Instant::now() {
        return Err(AppError::InvalidInput(
            "migration_plan_token_expired".into(),
        ));
    }
    if stored.provider_id != provider_id || stored.expected_revision != expected_revision {
        return Err(AppError::InvalidInput(
            "migration_plan_token_mismatch".into(),
        ));
    }
    let actual_revision = codex_multirouter_revision(db, provider_id)?;
    if let Some(result_revision) = stored.result_revision.as_deref() {
        if actual_revision == result_revision {
            return Ok(CodexMultiRouterMigrationApplyOutcome {
                provider_id: provider_id.to_string(),
                revision: result_revision.to_string(),
                created_provider_ids: stored
                    .generated_providers
                    .iter()
                    .map(|provider| provider.id.clone())
                    .collect(),
                already_applied: true,
            });
        }
    }
    if actual_revision != expected_revision {
        return Err(AppError::InvalidInput("migration_revision_conflict".into()));
    }

    {
        let mut conn = crate::database::lock_conn!(db.conn);
        let tx = conn
            .transaction()
            .map_err(|error| AppError::Database(error.to_string()))?;
        for provider in &stored.generated_providers {
            insert_migration_provider(&tx, provider)?;
        }
        for provider in &stored.updated_providers {
            tx.execute(
                "UPDATE providers SET settings_config = ?1, meta = ?2 WHERE id = ?3 AND app_type = 'codex'",
                params![
                    serde_json::to_string(&provider.settings_config)
                        .map_err(|error| AppError::Database(error.to_string()))?,
                    serde_json::to_string(&provider.meta.clone().unwrap_or_default())
                        .map_err(|error| AppError::Database(error.to_string()))?,
                    provider.id
                ],
            )
            .map_err(|error| AppError::Database(error.to_string()))?;
        }
        tx.execute(
            "UPDATE providers SET settings_config = ?1 WHERE id = ?2 AND app_type = 'codex'",
            params![
                serde_json::to_string(&stored.migrated_router.settings_config).map_err(
                    |error| {
                        AppError::Database(format!("Failed to serialize migrated router: {error}"))
                    }
                )?,
                provider_id
            ],
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
        tx.commit()
            .map_err(|error| AppError::Database(error.to_string()))?;
    }
    let result_revision = codex_multirouter_revision(db, provider_id)?;
    if let Ok(mut tokens) = migration_tokens().lock() {
        if let Some(entry) = tokens.get_mut(plan_token) {
            entry.result_revision = Some(result_revision.clone());
        }
    }
    Ok(CodexMultiRouterMigrationApplyOutcome {
        provider_id: provider_id.to_string(),
        revision: result_revision,
        created_provider_ids: stored
            .generated_providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect(),
        already_applied: false,
    })
}

fn insert_migration_provider(
    tx: &rusqlite::Transaction<'_>,
    provider: &Provider,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO providers (id, app_type, name, settings_config, website_url, category, created_at, sort_index, notes, icon, icon_color, meta, is_current, in_failover_queue)
         VALUES (?1, 'codex', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, 0)",
        params![
            provider.id,
            provider.name,
            serde_json::to_string(&provider.settings_config).map_err(|error| AppError::Database(error.to_string()))?,
            provider.website_url,
            provider.category,
            provider.created_at,
            provider.sort_index,
            provider.notes,
            provider.icon,
            provider.icon_color,
            serde_json::to_string(&provider.meta.clone().unwrap_or_default()).map_err(|error| AppError::Database(error.to_string()))?
        ],
    )
    .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(())
}

fn build_migration(
    router: &Provider,
    providers: &HashMap<String, Provider>,
) -> Result<BuiltMigration, AppError> {
    let legacy = router
        .settings_config
        .get("codexRouting")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("legacy codexRouting must be an object".into()))?;
    let routes = legacy
        .get("routes")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::InvalidInput("legacy routes are missing".into()))?;
    let mut generated_providers = Vec::new();
    let mut updated_providers = HashMap::<String, Provider>::new();
    let mut summaries = Vec::new();
    let mut v2_routes = Vec::new();
    let mut removed_fields = BTreeSet::new();
    let mut warnings = Vec::new();

    for (index, route) in routes.iter().enumerate() {
        let route_id = route
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("route-{index}"));
        let mut target_id = route
            .get("targetProviderId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| infer_official_target(route))
            .ok_or_else(|| {
                AppError::InvalidInput(format!("migration_target_ambiguous:{route_id}"))
            })?;
        let target = providers.get(&target_id).ok_or_else(|| {
            AppError::InvalidInput(format!("migration_target_missing:{target_id}"))
        })?;
        let upstream = route.get("upstream").and_then(Value::as_object);
        let inline_base_url = upstream
            .and_then(|value| value.get("baseUrl").or_else(|| value.get("base_url")))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty());
        let inline_key = upstream
            .and_then(|value| value.get("apiKey").or_else(|| value.get("api_key")))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let api_format = upstream
            .and_then(|value| value.get("apiFormat"))
            .and_then(Value::as_str);
        let api_format_source = upstream
            .and_then(|value| value.get("apiFormatSource"))
            .and_then(Value::as_str);
        let route_canonical_models = legacy_route_canonical_models(route, upstream);
        let protocol_override = api_format_source != Some("provider") && api_format.is_some();
        let protocol_conflict = protocol_override
            && api_format.is_some_and(|format| {
                legacy_protocol_conflicts(routes, &target_id, &route_canonical_models, format)
            });
        let legacy_capabilities = route.get("capabilities");
        let capability_conflict = legacy_capabilities.is_some_and(|capabilities| {
            legacy_capability_conflicts(routes, &target_id, &route_canonical_models, capabilities)
        });
        let requires_clone = inline_base_url.is_some()
            || inline_key.is_some()
            || protocol_conflict
            || capability_conflict;
        if requires_clone {
            let clone_id = unique_clone_id(&target_id, &route_id, providers, &generated_providers);
            let mut clone = target.clone();
            clone.id = clone_id.clone();
            clone.name = format!("{}（迁移生成：{}）", target.name, route_id);
            clone.settings_config["migrationGenerated"] = Value::Bool(true);
            clone.settings_config["migrationSourceProviderId"] = Value::String(target_id.clone());
            if let Some(base_url) = inline_base_url {
                clone.settings_config["base_url"] = Value::String(base_url.to_string());
            }
            if let Some(api_key) = inline_key {
                clone.settings_config["auth"] = json!({"OPENAI_API_KEY": api_key});
            }
            if let Some(format) = api_format {
                clone.meta.get_or_insert_with(Default::default).api_format =
                    Some(format.to_string());
            }
            if let Some(capabilities) = legacy_capabilities {
                for canonical in &route_canonical_models {
                    apply_model_capabilities(&mut clone, canonical, capabilities)?;
                }
            }
            summaries.push(GeneratedProviderSummary {
                id: clone_id.clone(),
                name: clone.name.clone(),
                migration_generated: true,
                source_provider_id: target_id.clone(),
            });
            generated_providers.push(clone);
            target_id = clone_id;
            warnings.push(format!(
                "route {route_id} uses a migration-generated Provider to preserve legacy overrides"
            ));
        } else if protocol_override {
            let format = api_format.expect("protocol override has a format");
            let provider = updated_providers
                .entry(target_id.clone())
                .or_insert_with(|| target.clone());
            for canonical in &route_canonical_models {
                apply_model_api_format(provider, canonical, format)?;
            }
        }
        if !requires_clone {
            if let Some(capabilities) = legacy_capabilities {
                let provider = updated_providers
                    .entry(target_id.clone())
                    .or_insert_with(|| target.clone());
                for canonical in &route_canonical_models {
                    apply_model_capabilities(provider, canonical, capabilities)?;
                }
            }
        }

        let model_map = upstream
            .and_then(|value| value.get("modelMap"))
            .and_then(Value::as_object);
        let canonical_models = route_canonical_models;
        let target_catalog = provider_catalog_models(target);
        let model_selection = if !target_catalog.is_empty() && canonical_models == target_catalog {
            CodexModelSelection::All
        } else {
            CodexModelSelection::Include {
                models: canonical_models.iter().cloned().collect(),
            }
        };
        let aliases = model_map
            .map(|map| {
                map.iter()
                    .filter_map(|(visible, canonical)| {
                        let canonical = canonical.as_str()?;
                        (visible != canonical).then(|| (visible.clone(), canonical.to_string()))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let match_prefixes = route
            .pointer("/match/prefixes")
            .and_then(Value::as_array)
            .map(|prefixes| {
                prefixes
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let auth_policy = legacy_auth_policy(upstream.and_then(|value| value.get("auth")))?;
        v2_routes.push(CodexRoutingRouteV2 {
            id: route_id,
            label: route
                .get("label")
                .and_then(Value::as_str)
                .map(str::to_string),
            enabled: route
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            target_provider_id: target_id,
            model_selection,
            match_prefixes,
            aliases,
            auth_policy,
        });
        for field in [
            "upstream.baseUrl",
            "upstream.apiKey",
            "upstream.apiFormat",
            "upstream.modelMap",
            "capabilities",
        ] {
            removed_fields.insert(field.to_string());
        }
    }

    let spawn_agent_models = router
        .settings_config
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("spawnAgentModels"))
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let plan = CodexRoutingConfigV2 {
        schema_version: CODEX_ROUTING_SCHEMA_V2,
        enabled: legacy
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        default_route_id: legacy
            .get("defaultRouteId")
            .and_then(Value::as_str)
            .map(str::to_string),
        routes: v2_routes,
        subagent_version: legacy
            .get("subagentVersion")
            .and_then(Value::as_str)
            .map(str::to_string),
        subagent_v2: legacy.get("subagentV2").cloned(),
        spawn_agent_models,
        extensions: BTreeMap::new(),
    };
    let mut migrated_router = router.clone();
    migrated_router.settings_config["codexRouting"] =
        serde_json::to_value(&plan).map_err(|error| AppError::Database(error.to_string()))?;
    if let Some(settings) = migrated_router.settings_config.as_object_mut() {
        settings.remove("modelCatalog");
    }
    let created_provider_ids = generated_providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect();
    Ok(BuiltMigration {
        migrated_router,
        generated_providers,
        updated_providers: updated_providers.into_values().collect(),
        summaries,
        diff: MigrationDiff {
            removed_route_fields: removed_fields.into_iter().collect(),
            created_provider_ids,
            changed_route_ids: plan.routes.iter().map(|route| route.id.clone()).collect(),
        },
        warnings,
    })
}

fn provider_catalog_models(provider: &Provider) -> BTreeSet<String> {
    provider
        .settings_config
        .pointer("/modelCatalog/models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            model
                .get("upstreamModel")
                .or_else(|| model.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn legacy_route_canonical_models(
    route: &Value,
    upstream: Option<&serde_json::Map<String, Value>>,
) -> BTreeSet<String> {
    let model_map = upstream
        .and_then(|value| value.get("modelMap"))
        .and_then(Value::as_object);
    route
        .pointer("/match/models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|visible| {
            model_map
                .and_then(|map| map.get(visible))
                .and_then(Value::as_str)
                .unwrap_or(visible)
                .to_string()
        })
        .collect()
}

fn legacy_protocol_conflicts(
    routes: &[Value],
    target_provider_id: &str,
    canonical_models: &BTreeSet<String>,
    api_format: &str,
) -> bool {
    routes.iter().any(|route| {
        if route.get("targetProviderId").and_then(Value::as_str) != Some(target_provider_id) {
            return false;
        }
        let upstream = route.get("upstream").and_then(Value::as_object);
        if upstream
            .and_then(|value| value.get("apiFormatSource"))
            .and_then(Value::as_str)
            == Some("provider")
        {
            return false;
        }
        let Some(other_format) = upstream
            .and_then(|value| value.get("apiFormat"))
            .and_then(Value::as_str)
        else {
            return false;
        };
        other_format != api_format
            && !legacy_route_canonical_models(route, upstream).is_disjoint(canonical_models)
    })
}

fn legacy_capability_conflicts(
    routes: &[Value],
    target_provider_id: &str,
    canonical_models: &BTreeSet<String>,
    capabilities: &Value,
) -> bool {
    routes.iter().any(|route| {
        if route.get("targetProviderId").and_then(Value::as_str) != Some(target_provider_id) {
            return false;
        }
        let Some(other_capabilities) = route.get("capabilities") else {
            return false;
        };
        other_capabilities != capabilities
            && !legacy_route_canonical_models(
                route,
                route.get("upstream").and_then(Value::as_object),
            )
            .is_disjoint(canonical_models)
    })
}

fn apply_model_api_format(
    provider: &mut Provider,
    canonical_model: &str,
    api_format: &str,
) -> Result<(), AppError> {
    let models = provider
        .settings_config
        .pointer_mut("/modelCatalog/models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            AppError::InvalidInput(format!("migration_model_catalog_missing:{}", provider.id))
        })?;
    let model = models
        .iter_mut()
        .find(|model| {
            model
                .get("upstreamModel")
                .or_else(|| model.get("model"))
                .and_then(Value::as_str)
                == Some(canonical_model)
        })
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "migration_model_missing:{}:{canonical_model}",
                provider.id
            ))
        })?;
    model["apiFormat"] = Value::String(api_format.to_string());
    Ok(())
}

fn apply_model_capabilities(
    provider: &mut Provider,
    canonical_model: &str,
    capabilities: &Value,
) -> Result<(), AppError> {
    let provider_id = provider.id.clone();
    let models = provider
        .settings_config
        .pointer_mut("/modelCatalog/models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            AppError::InvalidInput(format!("migration_model_catalog_missing:{provider_id}"))
        })?;
    let model = models
        .iter_mut()
        .find(|model| {
            model
                .get("upstreamModel")
                .or_else(|| model.get("model"))
                .and_then(Value::as_str)
                == Some(canonical_model)
        })
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "migration_model_missing:{provider_id}:{canonical_model}"
            ))
        })?;
    if let Some(input_modalities) = capabilities.get("inputModalities") {
        model["inputModalities"] = input_modalities.clone();
    } else if capabilities.get("textOnly").and_then(Value::as_bool) == Some(true) {
        model["inputModalities"] = json!(["text"]);
    }
    if let Some(supports_reasoning) = capabilities
        .get("supportsReasoning")
        .and_then(Value::as_bool)
    {
        model["reasoning"] = json!({"supported": supports_reasoning});
    }
    if let Some(cache) = capabilities.get("codexCache") {
        model["codexCache"] = cache.clone();
    }
    Ok(())
}

fn infer_official_target(route: &Value) -> Option<String> {
    let source = route.pointer("/upstream/auth/source")?.as_str()?;
    matches!(
        source,
        "managed_codex_oauth" | "native_codex_auth" | "account_pool"
    )
    .then(|| crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string())
}

fn legacy_auth_policy(value: Option<&Value>) -> Result<CodexRouteAuthPolicy, AppError> {
    let source = value
        .and_then(|value| value.get("source"))
        .and_then(Value::as_str)
        .unwrap_or("provider_config");
    let source = match source {
        "provider_config" => CodexRouteAuthSource::ProviderConfig,
        "managed_account" => CodexRouteAuthSource::ManagedAccount,
        "managed_codex_oauth" => CodexRouteAuthSource::ManagedCodexOauth,
        "native_codex_auth" => CodexRouteAuthSource::NativeCodexAuth,
        "account_pool" => CodexRouteAuthSource::AccountPool,
        _ => {
            return Err(AppError::InvalidInput(format!(
                "migration_auth_source_unknown:{source}"
            )))
        }
    };
    Ok(CodexRouteAuthPolicy {
        source,
        account_id: value
            .and_then(|value| value.get("accountId"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn unique_clone_id(
    target_id: &str,
    route_id: &str,
    providers: &HashMap<String, Provider>,
    generated: &[Provider],
) -> String {
    let suffix = route_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    let base = format!("{target_id}-migration-{suffix}");
    let mut candidate = base.clone();
    let mut index = 2;
    while providers.contains_key(&candidate)
        || generated.iter().any(|provider| provider.id == candidate)
    {
        candidate = format!("{base}-{index}");
        index += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    fn target() -> Provider {
        let mut provider = Provider::with_id(
            "qwen".to_string(),
            "Qwen".to_string(),
            json!({
                "base_url": "https://qwen.example/v1",
                "auth": {"OPENAI_API_KEY": "provider-secret"},
                "modelCatalog": {"models": [{"model": "qwen3.8"}]}
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        provider
    }

    fn legacy_router(route: serde_json::Value) -> Provider {
        Provider::with_id(
            "router".to_string(),
            "Legacy Router".to_string(),
            json!({
                "auth": {},
                "modelCatalog": {
                    "models": [{"model": "qwen3.8"}],
                    "spawnAgentModels": ["qwen3.8"]
                },
                "codexRouting": {
                    "enabled": true,
                    "defaultRouteId": "qwen-route",
                    "routes": [route]
                }
            }),
            None,
        )
    }

    fn legacy_route(upstream: serde_json::Value) -> serde_json::Value {
        json!({
            "id": "qwen-route",
            "enabled": true,
            "targetProviderId": "qwen",
            "match": {"models": ["qwen3.8"], "prefixes": ["qwen-"]},
            "upstream": upstream
        })
    }

    #[test]
    fn preview_inherits_stale_provider_snapshot_and_is_secret_free() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target()).expect("save target");
        db.save_provider(
            "codex",
            &legacy_router(legacy_route(json!({
                "apiFormat": "openai_chat",
                "apiFormatSource": "provider",
                "auth": {"source": "provider_config"},
                "modelMap": {"qwen3.8": "qwen3.8"}
            }))),
        )
        .expect("save router");
        let revision = codex_multirouter_revision(&db, "router").expect("revision");

        let preview = preview_codex_multirouter_migration(&db, "router", &revision)
            .expect("preview migration");
        let serialized = serde_json::to_string(&preview).expect("serialize preview");

        assert_eq!(preview.schema_version, 2);
        assert!(preview.generated_providers.is_empty());
        assert!(!serialized.contains("provider-secret"));
        assert!(!serialized.contains("OPENAI_API_KEY"));
        assert!(preview
            .diff
            .removed_route_fields
            .contains(&"upstream.apiFormat".to_string()));
        apply_codex_multirouter_migration(&db, "router", &revision, &preview.plan_token)
            .expect("apply migration");
        let migrated = db
            .get_provider_by_id("router", "codex")
            .expect("read router")
            .expect("router exists");
        assert_eq!(
            migrated.settings_config["codexRouting"]["routes"][0]["modelSelection"]["mode"],
            "all"
        );
        assert_eq!(
            migrated.settings_config["codexRouting"]["spawnAgentModels"],
            json!(["qwen3.8"])
        );
    }

    #[test]
    fn inline_credentials_create_redacted_migration_provider_and_apply_is_idempotent() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target()).expect("save target");
        db.save_provider(
            "codex",
            &legacy_router(legacy_route(json!({
                "baseUrl": "https://legacy.example/v1",
                "apiKey": "inline-secret",
                "apiFormat": "openai_chat",
                "auth": {"source": "provider_config"}
            }))),
        )
        .expect("save router");
        let revision = codex_multirouter_revision(&db, "router").expect("revision");
        let preview = preview_codex_multirouter_migration(&db, "router", &revision)
            .expect("preview migration");

        assert_eq!(preview.generated_providers.len(), 1);
        assert!(preview.generated_providers[0].migration_generated);
        assert!(!serde_json::to_string(&preview)
            .expect("serialize")
            .contains("inline-secret"));

        let applied =
            apply_codex_multirouter_migration(&db, "router", &revision, &preview.plan_token)
                .expect("apply migration");
        assert!(!applied.already_applied);
        let rerun =
            apply_codex_multirouter_migration(&db, "router", &revision, &preview.plan_token)
                .expect("idempotent rerun");
        assert!(rerun.already_applied);
        let router = db
            .get_provider_by_id("router", "codex")
            .expect("read router")
            .expect("router exists");
        assert_eq!(router.settings_config["codexRouting"]["schemaVersion"], 2);
        assert!(router.settings_config["codexRouting"]["routes"][0]
            .get("upstream")
            .is_none());
    }

    #[test]
    fn disjoint_deepseek_protocol_routes_migrate_to_model_entries_without_clones() {
        let db = Database::memory().expect("memory db");
        let mut deepseek = target();
        deepseek.id = "deepseek".to_string();
        deepseek.name = "DeepSeek".to_string();
        deepseek.settings_config["modelCatalog"]["models"] = json!([
            {"model": "deepseek-flash"},
            {"model": "deepseek-pro"}
        ]);
        db.save_provider("codex", &deepseek).expect("save deepseek");
        let router = Provider::with_id(
            "deepseek-router".to_string(),
            "DeepSeek Legacy Router".to_string(),
            json!({
                "auth": {},
                "codexRouting": {
                    "enabled": true,
                    "routes": [
                        {
                            "id": "flash",
                            "targetProviderId": "deepseek",
                            "match": {"models": ["deepseek-flash"]},
                            "upstream": {"apiFormat": "openai_responses", "apiFormatSource": "route_override", "auth": {"source": "provider_config"}}
                        },
                        {
                            "id": "pro",
                            "targetProviderId": "deepseek",
                            "match": {"models": ["deepseek-pro"]},
                            "upstream": {"apiFormat": "openai_chat", "apiFormatSource": "route_override", "auth": {"source": "provider_config"}}
                        }
                    ]
                }
            }),
            None,
        );
        db.save_provider("codex", &router).expect("save router");
        let revision = codex_multirouter_revision(&db, "deepseek-router").expect("revision");
        let preview = preview_codex_multirouter_migration(&db, "deepseek-router", &revision)
            .expect("preview");

        assert!(preview.generated_providers.is_empty());
        apply_codex_multirouter_migration(&db, "deepseek-router", &revision, &preview.plan_token)
            .expect("apply");
        let saved = db
            .get_provider_by_id("deepseek", "codex")
            .expect("read provider")
            .expect("provider exists");
        assert_eq!(
            saved.settings_config["modelCatalog"]["models"][0]["apiFormat"],
            "openai_responses"
        );
        assert_eq!(
            saved.settings_config["modelCatalog"]["models"][1]["apiFormat"],
            "openai_chat"
        );
    }

    #[test]
    fn route_capabilities_move_to_the_selected_provider_model() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target()).expect("save target");
        let mut route = legacy_route(json!({
            "apiFormat": "openai_responses",
            "apiFormatSource": "provider",
            "auth": {"source": "provider_config"}
        }));
        route["capabilities"] = json!({
            "inputModalities": ["text", "image"],
            "supportsReasoning": true,
            "codexCache": {"enabled": true}
        });
        db.save_provider("codex", &legacy_router(route))
            .expect("save router");
        let revision = codex_multirouter_revision(&db, "router").expect("revision");
        let preview =
            preview_codex_multirouter_migration(&db, "router", &revision).expect("preview");
        apply_codex_multirouter_migration(&db, "router", &revision, &preview.plan_token)
            .expect("apply");

        let saved = db
            .get_provider_by_id("qwen", "codex")
            .expect("read target")
            .expect("target exists");
        let model = &saved.settings_config["modelCatalog"]["models"][0];
        assert_eq!(model["inputModalities"], json!(["text", "image"]));
        assert_eq!(model["reasoning"]["supported"], true);
        assert_eq!(model["codexCache"]["enabled"], true);
    }

    #[test]
    fn apply_rejects_revision_changed_after_preview() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target()).expect("save target");
        let router = legacy_router(legacy_route(json!({
            "apiFormat": "openai_responses",
            "apiFormatSource": "provider",
            "auth": {"source": "provider_config"}
        })));
        db.save_provider("codex", &router).expect("save router");
        let revision = codex_multirouter_revision(&db, "router").expect("revision");
        let preview =
            preview_codex_multirouter_migration(&db, "router", &revision).expect("preview");
        let mut changed = router;
        changed.name = "Changed after preview".to_string();
        db.save_provider("codex", &changed).expect("change router");

        let error =
            apply_codex_multirouter_migration(&db, "router", &revision, &preview.plan_token)
                .expect_err("stale preview must fail");
        assert!(error.to_string().contains("migration_revision_conflict"));
    }
}
