use super::active_codex_router_id;
use super::compiler::{compile_v2, compile_v2_strict};
use super::projection::{
    effective_settings_for_candidate_with_providers, ensure_projection_with_publisher,
    CodexRoutingProjectionArtifact, CodexRoutingProjectionStatus, ProjectionReadBack,
};
use super::provider_set::{
    plan_codex_provider_set, plan_manual_codex_provider_set, CodexProviderSetPersistence,
    CodexProviderSetPlan, CodexProviderSetPreview, PreparedCodexProviderSetMutation,
};
use super::schema::CodexRoutingDocument;
use crate::database::{Database, ProviderSetDatabaseMutation, ProviderSetDatabaseTransaction};
use crate::error::AppError;
use crate::protocol_compatibility::{
    compile_codex_router_probe_candidates, ProbeReadiness, ProbeTargetKey,
    ProtocolCompatibilityRecord, TransportKind, PROBE_PROFILE_VERSION,
};
use crate::provider::Provider;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct CodexProviderMutationOutcome {
    pub projections: Vec<CodexRoutingProjectionStatus>,
}

pub(crate) struct PreparedCodexProviderSetCommit {
    pub transaction: ProviderSetDatabaseTransaction,
    pub projection_router_ids: Vec<String>,
}

pub(crate) struct PreparedCodexProviderSetBatchCommit {
    pub transaction: Option<ProviderSetDatabaseTransaction>,
    pub projection_router_ids: Vec<String>,
    pub source_previews: Vec<CodexProviderSetPreview>,
    pub router: Provider,
    pub editor_router: Provider,
    pub blocked: bool,
}

pub(crate) struct PreparedCodexProviderMutation {
    pub provider: Provider,
    pub profiles: Vec<ProtocolCompatibilityRecord>,
    pub related_provider_ids: HashSet<String>,
    pub router_updates: Vec<Provider>,
    projection_router_ids: Vec<String>,
}

impl PreparedCodexProviderMutation {
    pub(crate) fn append_provider_mutations<'a>(
        &'a self,
        mutations: &mut Vec<(&'a str, &'a str, Option<&'a Provider>)>,
    ) {
        mutations.push(("codex", self.provider.id.as_str(), Some(&self.provider)));
        for router in &self.router_updates {
            mutations.push(("codex", router.id.as_str(), Some(router)));
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderDeleteOutcome {
    pub deleted_provider_id: String,
    pub affected_plan_ids: Vec<String>,
    pub disabled_plan_ids: Vec<String>,
    pub removed_candidates: Vec<String>,
    pub projections: Vec<CodexRoutingProjectionStatus>,
    pub warnings: Vec<String>,
}

struct PreparedRouterDeletion {
    router_id: String,
    provider: Provider,
    disabled: bool,
}

pub(crate) struct PreparedCodexProviderDeletion {
    provider_id: String,
    provider_ids: Vec<String>,
    routers: Vec<PreparedRouterDeletion>,
    active_router_id: Option<String>,
    must_restore_official: bool,
    removed_candidates: BTreeSet<String>,
}

impl PreparedCodexProviderDeletion {
    pub(crate) fn must_restore_official(&self) -> bool {
        self.must_restore_official
    }

    pub(crate) fn append_provider_mutations<'a>(
        &'a self,
        mutations: &mut Vec<(&'a str, &'a str, Option<&'a Provider>)>,
    ) {
        for router in &self.routers {
            mutations.push(("codex", router.router_id.as_str(), Some(&router.provider)));
        }
        for provider_id in &self.provider_ids {
            mutations.push(("codex", provider_id.as_str(), None));
        }
    }

    pub(crate) fn projection_setting_keys(&self) -> Vec<String> {
        self.provider_ids
            .iter()
            .map(|provider_id| format!("codex_multirouter_projection:{provider_id}"))
            .collect()
    }

    pub(crate) fn database_transaction(&self) -> ProviderSetDatabaseTransaction {
        let mut mutations = Vec::with_capacity(self.routers.len() + self.provider_ids.len());
        self.append_provider_mutations(&mut mutations);
        ProviderSetDatabaseTransaction {
            mutations: mutations
                .into_iter()
                .map(
                    |(app_type, provider_id, provider)| ProviderSetDatabaseMutation {
                        app_type: app_type.to_string(),
                        provider_id: provider_id.to_string(),
                        provider: provider.cloned(),
                    },
                )
                .collect(),
            profile_owner_ids: HashSet::new(),
            records: Vec::new(),
            observations: Vec::new(),
            replace_profile_provider_ids: HashSet::new(),
            setting_keys_to_delete: self.projection_setting_keys(),
            universal_provider: None,
            current_provider_after: None,
            official_seed_current_after: self.must_restore_official.then(|| {
                (
                    "codex".to_string(),
                    crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string(),
                )
            }),
        }
    }
}

fn current_provider_belongs_to_source(
    providers: &HashMap<String, Provider>,
    current_provider_id: &str,
    source_provider_id: &str,
) -> bool {
    current_provider_id == source_provider_id
        || providers
            .get(current_provider_id)
            .and_then(super::provider_set::codex_provider_set_leaf_parent_id)
            == Some(source_provider_id)
}

struct AffectedRouterIds {
    projection: Vec<String>,
    subagent_profiles: Vec<String>,
}

pub(crate) fn prepare_codex_provider_mutation(
    db: &Database,
    mut provider: Provider,
    profiles: &[ProtocolCompatibilityRecord],
) -> Result<PreparedCodexProviderMutation, AppError> {
    remove_schema_v2_router_derived_catalog(&mut provider);
    let affected = validate_and_collect_affected_router_ids(db, &provider)?;
    let profiles = materialize_equivalent_router_protocol_profiles(
        db,
        &provider,
        &affected.projection,
        profiles,
    )?;
    let mut router_updates =
        prepare_router_subagent_profile_updates(db, &provider, &affected.subagent_profiles)?;
    if let Some(index) = router_updates
        .iter()
        .position(|router| router.id == provider.id)
    {
        provider = router_updates.remove(index);
    }
    let related_provider_ids = affected.projection.iter().cloned().collect();
    Ok(PreparedCodexProviderMutation {
        provider,
        profiles,
        related_provider_ids,
        router_updates,
        projection_router_ids: affected.projection,
    })
}

pub fn apply_codex_provider_mutation(
    db: &Database,
    provider: Provider,
) -> Result<CodexProviderMutationOutcome, AppError> {
    apply_codex_provider_mutation_with_profiles_and_publisher(db, provider, &[], &[], |artifact| {
        crate::codex_config::publish_codex_multirouter_projection_for_database(
            db,
            &artifact.projection_settings,
        )
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
    apply_codex_provider_mutation_with_protocol_state(db, provider, profiles, &[])
}

pub fn apply_codex_provider_mutation_with_protocol_state(
    db: &Database,
    provider: Provider,
    profiles: &[ProtocolCompatibilityRecord],
    observations: &[ProtocolCompatibilityRecord],
) -> Result<CodexProviderMutationOutcome, AppError> {
    apply_codex_provider_mutation_with_profiles_and_publisher(
        db,
        provider,
        profiles,
        observations,
        |artifact| {
            crate::codex_config::publish_codex_multirouter_projection_for_database(
                db,
                &artifact.projection_settings,
            )
            .map_err(|error| error.to_string())
        },
    )
}

pub fn apply_codex_provider_mutation_with_publisher<F>(
    db: &Database,
    provider: Provider,
    publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    apply_codex_provider_mutation_with_profiles_and_publisher(db, provider, &[], &[], publish)
}

pub fn apply_codex_provider_set_mutation_with_publisher<F>(
    db: &Database,
    prepared: PreparedCodexProviderSetMutation,
    publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    apply_codex_provider_set_mutation_with_observations_and_publisher(
        db,
        prepared,
        Vec::new(),
        publish,
    )
}

pub(crate) fn apply_codex_provider_set_mutation_with_observations_and_publisher<F>(
    db: &Database,
    prepared: PreparedCodexProviderSetMutation,
    observations: Vec<ProtocolCompatibilityRecord>,
    publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    let mut commit = prepare_codex_provider_set_commit(db, prepared)?;
    commit.transaction.observations = observations;
    db.apply_provider_set_database_transaction(commit.transaction)?;
    finalize_codex_provider_set_projections_with_publisher(
        db,
        commit.projection_router_ids,
        publish,
    )
}

pub(crate) fn prepare_codex_provider_set_commit(
    db: &Database,
    prepared: PreparedCodexProviderSetMutation,
) -> Result<PreparedCodexProviderSetCommit, AppError> {
    let existing = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let now = chrono::Utc::now().timestamp();
    let replanned = if let Some(transport) = prepared.manual_transport() {
        plan_manual_codex_provider_set(prepared.source_draft(), transport, &existing, now)
    } else {
        plan_codex_provider_set(
            prepared.source_draft(),
            prepared.probe_records(),
            &existing,
            now,
        )
    }
    .map_err(provider_set_error)?;
    if replanned.preview.digest != prepared.preview.digest {
        return Err(AppError::InvalidInput(
            "codex_provider_set_dependency_changed: Provider、探测档案或依赖 MultiRouter 已在确认后发生变化"
                .to_string(),
        ));
    }
    if matches!(replanned.persistence, CodexProviderSetPersistence::Blocked) {
        return Err(AppError::InvalidInput(
            "codex_provider_set_model_blocked: Partial/Failed 模型不能保存为可执行 Provider Set"
                .to_string(),
        ));
    }

    let mut mutations = Vec::new();
    let mut projection_router_ids = Vec::new();
    match &replanned.persistence {
        CodexProviderSetPersistence::Single { provider, .. } => {
            mutations.push(ProviderSetDatabaseMutation {
                app_type: "codex".to_string(),
                provider_id: provider.id.clone(),
                provider: Some(provider.clone()),
            });
        }
        CodexProviderSetPersistence::Split {
            facade,
            responses_provider,
            chat_provider,
        } => {
            for provider in [facade, responses_provider, chat_provider] {
                mutations.push(ProviderSetDatabaseMutation {
                    app_type: "codex".to_string(),
                    provider_id: provider.id.clone(),
                    provider: Some(provider.clone()),
                });
            }
            projection_router_ids.push(facade.id.clone());
        }
        CodexProviderSetPersistence::Blocked => unreachable!("blocked was rejected above"),
    }
    for router in &replanned.router_updates {
        mutations.push(ProviderSetDatabaseMutation {
            app_type: "codex".to_string(),
            provider_id: router.id.clone(),
            provider: Some(router.clone()),
        });
        projection_router_ids.push(router.id.clone());
    }
    for provider_id in &replanned.delete_provider_ids {
        mutations.push(ProviderSetDatabaseMutation {
            app_type: "codex".to_string(),
            provider_id: provider_id.clone(),
            provider: None,
        });
    }

    let current_before = db.get_current_provider("codex")?;
    let should_activate_source = current_before.as_ref().is_none_or(|current| {
        current_provider_belongs_to_source(
            &existing,
            current,
            &replanned.preview.source_provider_id,
        )
    });
    let current_provider_after = should_activate_source.then(|| {
        (
            "codex".to_string(),
            replanned.preview.source_provider_id.clone(),
        )
    });
    let mut profile_owner_ids: HashSet<String> = replanned
        .profiles
        .iter()
        .map(|record| record.target.provider_id.clone())
        .collect();
    profile_owner_ids.insert(replanned.preview.source_provider_id.clone());
    let setting_keys_to_delete = replanned
        .delete_provider_ids
        .iter()
        .map(|provider_id| format!("codex_multirouter_projection:{provider_id}"))
        .collect();
    Ok(PreparedCodexProviderSetCommit {
        transaction: ProviderSetDatabaseTransaction {
            mutations,
            profile_owner_ids,
            records: replanned.profiles.clone(),
            observations: Vec::new(),
            replace_profile_provider_ids: replanned.replace_profile_provider_ids.clone(),
            setting_keys_to_delete,
            universal_provider: None,
            current_provider_after,
            official_seed_current_after: None,
        },
        projection_router_ids,
    })
}

pub(crate) fn prepare_codex_provider_set_batch_commit(
    db: &Database,
    sources: Vec<(Provider, Vec<ProtocolCompatibilityRecord>)>,
    router: Provider,
    now: i64,
) -> Result<PreparedCodexProviderSetBatchCommit, AppError> {
    let mut virtual_providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let providers_before = virtual_providers.clone();
    let current_before = db.get_current_provider("codex")?;
    if sources.iter().any(|(source, _)| source.id == router.id) {
        return Err(AppError::InvalidInput(
            "codex_provider_set_batch_router_id_conflict".to_string(),
        ));
    }
    virtual_providers.insert(router.id.clone(), router.clone());

    let mut mutations = BTreeMap::<String, Option<Provider>>::new();
    let mut profiles = Vec::new();
    let mut profile_owner_ids = HashSet::new();
    let mut replace_profile_provider_ids = HashSet::new();
    let mut setting_keys_to_delete = Vec::new();
    let mut projection_router_ids = Vec::new();
    let mut source_previews = Vec::with_capacity(sources.len());
    let mut blocked = false;
    let mut current_provider_after = None;

    for (source, records) in sources {
        profile_owner_ids.insert(source.id.clone());
        let prepared = if source.uses_fixed_codex_responses_transport() {
            // Official and account-managed Codex sources are native Responses endpoints. They
            // deliberately skip paid compatibility probes, but still use the same single-
            // transport planner so stale generated leaves and dependent routes are reconciled.
            plan_manual_codex_provider_set(
                &source,
                TransportKind::OpenAiResponses,
                &virtual_providers,
                now,
            )
        } else if source.uses_manual_codex_protocol() && source.has_codex_protocol_overrides() {
            plan_codex_provider_set(&source, &records, &virtual_providers, now)
        } else if source.uses_manual_codex_protocol() {
            let api_format = source
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref())
                .or_else(|| {
                    source
                        .settings_config
                        .get("apiFormat")
                        .and_then(serde_json::Value::as_str)
                });
            let transport = match api_format {
                Some("openai_chat") => TransportKind::OpenAiChat,
                Some("openai_responses") => TransportKind::OpenAiResponses,
                _ => {
                    return Err(AppError::InvalidInput(
                        "codex_provider_set_manual_intent_required".to_string(),
                    ))
                }
            };
            plan_manual_codex_provider_set(&source, transport, &virtual_providers, now)
        } else {
            plan_codex_provider_set(&source, &records, &virtual_providers, now)
        }
        .map_err(provider_set_error)?;

        if current_before.as_deref().is_some_and(|current| {
            current_provider_belongs_to_source(&providers_before, current, &source.id)
        }) {
            current_provider_after = Some(("codex".to_string(), source.id.clone()));
        }

        source_previews.push(prepared.preview.clone());
        if matches!(prepared.preview.plan, CodexProviderSetPlan::Blocked { .. }) {
            blocked = true;
            continue;
        }
        profiles.extend(prepared.profiles.iter().cloned());
        replace_profile_provider_ids.extend(prepared.replace_profile_provider_ids.iter().cloned());

        match &prepared.persistence {
            CodexProviderSetPersistence::Single { provider, .. } => {
                virtual_providers.insert(provider.id.clone(), provider.clone());
                mutations.insert(provider.id.clone(), Some(provider.clone()));
            }
            CodexProviderSetPersistence::Split {
                facade,
                responses_provider,
                chat_provider,
            } => {
                for provider in [facade, responses_provider, chat_provider] {
                    virtual_providers.insert(provider.id.clone(), provider.clone());
                    mutations.insert(provider.id.clone(), Some(provider.clone()));
                }
                projection_router_ids.push(facade.id.clone());
            }
            CodexProviderSetPersistence::Blocked => unreachable!("blocked was rejected above"),
        }
        for provider_id in &prepared.delete_provider_ids {
            virtual_providers.remove(provider_id);
            mutations.insert(provider_id.clone(), None);
            setting_keys_to_delete.push(format!("codex_multirouter_projection:{provider_id}"));
        }
        for dependent_router in &prepared.router_updates {
            virtual_providers.insert(dependent_router.id.clone(), dependent_router.clone());
            mutations.insert(dependent_router.id.clone(), Some(dependent_router.clone()));
            projection_router_ids.push(dependent_router.id.clone());
        }
    }

    let mut router = virtual_providers
        .get(&router.id)
        .cloned()
        .ok_or_else(|| AppError::InvalidInput("codex_provider_set_batch_router_missing".into()))?;
    if blocked {
        return Ok(PreparedCodexProviderSetBatchCommit {
            transaction: None,
            projection_router_ids: Vec::new(),
            source_previews,
            editor_router: router.clone(),
            router,
            blocked: true,
        });
    }
    let needs_subagent_v2_initialization = router
        .settings_config
        .pointer("/codexRouting/subagentVersion")
        .and_then(serde_json::Value::as_str)
        == Some("v2")
        && router
            .settings_config
            .pointer("/codexRouting/subagentV2")
            .is_none();
    if needs_subagent_v2_initialization {
        let effective =
            effective_settings_for_candidate_with_providers(&router, false, &virtual_providers)?;
        let provider_context = crate::codex_config::ProviderClassificationContext::from_providers(
            virtual_providers.values(),
        );
        router.settings_config["codexRouting"]["subagentV2"] =
            crate::codex_config::initialize_codex_subagent_v2_for_candidate(
                &effective,
                Some(&provider_context),
            )?;
        virtual_providers.insert(router.id.clone(), router.clone());
    }
    let routing = router
        .settings_config
        .get("codexRouting")
        .ok_or_else(|| AppError::InvalidInput("codex_provider_set_batch_router_required".into()))?;
    let CodexRoutingDocument::V2(plan) = CodexRoutingDocument::parse(routing)
        .map_err(|error| AppError::InvalidInput(format!("{}: {}", error.code, error.message)))?
    else {
        return Err(AppError::InvalidInput(
            "codex_provider_set_batch_router_requires_v2".to_string(),
        ));
    };
    for route in &plan.routes {
        if virtual_providers
            .get(&route.target_provider_id)
            .is_some_and(|provider| provider.settings_config.get("codexRouting").is_some())
        {
            return Err(AppError::InvalidInput(format!(
                "codex_provider_set_batch_nested_router: {}",
                route.target_provider_id
            )));
        }
    }
    compile_v2_strict(&plan, &virtual_providers)
        .map_err(|error| AppError::InvalidInput(format!("{}: {}", error.code, error.message)))?;
    if router
        .settings_config
        .pointer("/codexRouting/subagentV2")
        .is_some()
    {
        let effective =
            effective_settings_for_candidate_with_providers(&router, true, &virtual_providers)?;
        let provider_context = crate::codex_config::ProviderClassificationContext::from_providers(
            virtual_providers.values(),
        );
        crate::codex_config::validate_codex_subagent_v2_candidate(
            &effective,
            Some(&provider_context),
            true,
        )?;
    }
    // Keep the logical editing contract alongside the runtime leaf graph. Build
    // it before any writes, using exactly the candidate graph being committed.
    let editor_router = super::provider_set::project_codex_editor_providers(&virtual_providers)
        .map_err(provider_set_error)?
        .remove(&router.id)
        .ok_or_else(|| AppError::InvalidInput("codex_editor_router_missing".to_string()))?;
    mutations.insert(router.id.clone(), Some(router.clone()));
    projection_router_ids.push(router.id.clone());
    projection_router_ids.sort();
    projection_router_ids.dedup();

    profile_owner_ids.extend(
        profiles
            .iter()
            .map(|record| record.target.provider_id.clone()),
    );
    Ok(PreparedCodexProviderSetBatchCommit {
        transaction: Some(ProviderSetDatabaseTransaction {
            mutations: mutations
                .into_iter()
                .map(|(provider_id, provider)| ProviderSetDatabaseMutation {
                    app_type: "codex".to_string(),
                    provider_id,
                    provider,
                })
                .collect(),
            profile_owner_ids,
            records: profiles,
            observations: Vec::new(),
            replace_profile_provider_ids,
            setting_keys_to_delete,
            universal_provider: None,
            current_provider_after,
            official_seed_current_after: None,
        }),
        projection_router_ids,
        source_previews,
        router,
        editor_router,
        blocked: false,
    })
}

pub(crate) fn finalize_codex_provider_set_projections_with_publisher<F>(
    db: &Database,
    mut projection_router_ids: Vec<String>,
    mut publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    projection_router_ids.sort();
    projection_router_ids.dedup();
    let active_router_id = active_codex_router_id(db)?;
    let publish_without_active_router =
        active_router_id.is_none() && projection_router_ids.len() == 1;
    let mut projections = Vec::new();
    for router_id in projection_router_ids {
        if active_router_id.as_deref() != Some(router_id.as_str()) && !publish_without_active_router
        {
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

pub fn apply_codex_provider_set_mutation(
    db: &Database,
    prepared: PreparedCodexProviderSetMutation,
) -> Result<CodexProviderMutationOutcome, AppError> {
    apply_codex_provider_set_mutation_with_publisher(db, prepared, |artifact| {
        crate::codex_config::publish_codex_multirouter_projection_for_database(
            db,
            &artifact.projection_settings,
        )
        .map_err(|error| error.to_string())
    })
}

fn provider_set_error(error: super::provider_set::CodexProviderSetError) -> AppError {
    AppError::InvalidInput(format!("{}: {}", error.code, error.message))
}

fn apply_codex_provider_mutation_with_profiles_and_publisher<F>(
    db: &Database,
    provider: Provider,
    profiles: &[ProtocolCompatibilityRecord],
    observations: &[ProtocolCompatibilityRecord],
    mut publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    let prepared = prepare_codex_provider_mutation(db, provider, profiles)?;
    let mut mutations = Vec::with_capacity(1 + prepared.router_updates.len());
    prepared.append_provider_mutations(&mut mutations);
    db.apply_provider_set_with_protocol_state_and_setting_cleanup(
        &mutations,
        Some(prepared.provider.id.as_str()),
        &prepared.profiles,
        observations,
        &prepared.related_provider_ids,
        &[],
    )?;

    finalize_codex_provider_mutation(db, &prepared, |artifact| publish(artifact))
}

pub(crate) fn finalize_codex_provider_mutation<F>(
    db: &Database,
    prepared: &PreparedCodexProviderMutation,
    mut publish: F,
) -> Result<CodexProviderMutationOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    // Provider、协议档案和 Provider-derived V2 子 Agent 档案已经在一个 SQLite
    // 事务中提交。这里仅发布可重试的 live 派生投影，不能再写持久化策略事实。
    let active_router_id = active_codex_router_id(db)?;
    let publish_without_active_router =
        active_router_id.is_none() && prepared.projection_router_ids.len() == 1;
    let mut projections = Vec::with_capacity(prepared.projection_router_ids.len());
    for router_id in &prepared.projection_router_ids {
        let owns_shared_projection = active_router_id.as_deref() == Some(router_id.as_str())
            || publish_without_active_router;
        if !owns_shared_projection {
            continue;
        }
        projections.push(ensure_projection_with_publisher(
            db,
            router_id,
            false,
            |artifact| publish(artifact),
        )?);
    }
    Ok(CodexProviderMutationOutcome { projections })
}

fn materialize_equivalent_router_protocol_profiles(
    db: &Database,
    provider: &Provider,
    affected_router_ids: &[String],
    profiles: &[ProtocolCompatibilityRecord],
) -> Result<Vec<ProtocolCompatibilityRecord>, AppError> {
    let mut expanded = profiles.to_vec();
    if !profiles.iter().any(|profile| {
        profile.target.provider_id == provider.id
            && profile.target.route_id.is_none()
            && profile.probe_version == PROBE_PROFILE_VERSION
            && profile.result.readiness == ProbeReadiness::Verified
    }) {
        return Ok(expanded);
    }

    let mut providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    providers.insert(provider.id.clone(), provider.clone());

    for router_id in affected_router_ids {
        if router_id == &provider.id {
            continue;
        }
        let Some(router) = providers.get(router_id) else {
            continue;
        };
        let candidates = match compile_codex_router_probe_candidates(router, &providers) {
            Ok(candidates) => candidates,
            Err(error) => {
                log::warn!(
                    "Skipping protocol profile synchronization for Codex MultiRouter {router_id}: {error}"
                );
                continue;
            }
        };
        for candidate in candidates {
            for profile in profiles.iter().filter(|profile| {
                profile.target.provider_id == provider.id
                    && profile.target.route_id.is_none()
                    && profile.probe_version == PROBE_PROFILE_VERSION
                    && profile.result.readiness == ProbeReadiness::Verified
            }) {
                let Some(selected_transport) = profile.result.selected_transport else {
                    continue;
                };
                let Some(route_target) = probe_target_for_candidate(&candidate, selected_transport)
                else {
                    continue;
                };
                if !same_protocol_target(&profile.target, &route_target) {
                    continue;
                }
                let mut route_profile = profile.clone();
                route_profile.target = route_target;
                if expanded
                    .iter()
                    .all(|existing| existing.storage_key() != route_profile.storage_key())
                {
                    expanded.push(route_profile);
                }
            }
        }
    }
    Ok(expanded)
}

fn probe_target_for_candidate(
    candidate: &crate::protocol_compatibility::ProbeCandidate,
    transport: TransportKind,
) -> Option<ProbeTargetKey> {
    candidate.target_key(transport).ok()
}

fn same_protocol_target(left: &ProbeTargetKey, right: &ProbeTargetKey) -> bool {
    left.public_model == right.public_model
        && left.upstream_model == right.upstream_model
        && left.transport == right.transport
        && left.endpoint_fingerprint == right.endpoint_fingerprint
        && left.authentication_kind == right.authentication_kind
        && left.credential_fingerprint == right.credential_fingerprint
        && left.request_policy_fingerprint == right.request_policy_fingerprint
}

fn prepare_router_subagent_profile_updates(
    db: &Database,
    candidate: &Provider,
    router_ids: &[String],
) -> Result<Vec<Provider>, AppError> {
    let mut providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    providers.insert(candidate.id.clone(), candidate.clone());
    let provider_context =
        crate::codex_config::ProviderClassificationContext::from_providers(providers.values());
    let mut updates = Vec::new();

    for router_id in router_ids {
        let mut router = providers.get(router_id).cloned().ok_or_else(|| {
            AppError::Message(format!("Codex MultiRouter provider not found: {router_id}"))
        })?;
        let Some(current) = router
            .settings_config
            .pointer("/codexRouting/subagentV2")
            .cloned()
        else {
            continue;
        };
        let effective =
            effective_settings_for_candidate_with_providers(&router, false, &providers)?;
        let reconciled = crate::codex_config::reconcile_codex_subagent_v2_for_candidate(
            &effective,
            crate::codex_config::CodexSubagentV2ReconcileAction::SyncCatalog,
            Some(&current),
            Some(&provider_context),
        )?;
        if reconciled == current {
            continue;
        }
        router.settings_config["codexRouting"]["subagentV2"] = reconciled;
        updates.push(router);
    }

    Ok(updates)
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
    let prepared = prepare_codex_provider_deletion(db, provider_id, current_provider_id)?;
    db.apply_provider_set_database_transaction(prepared.database_transaction())?;

    let mut warnings = Vec::new();
    if prepared.must_restore_official() {
        if let Err(error) = restore_official() {
            log::warn!(
                "Codex official live projection pending after authoritative deletion commit: {}",
                error
            );
            warnings.push("codex_official_projection_pending_retry".to_string());
        }
    }
    let mut outcome = finalize_codex_provider_deletion(db, prepared, |artifact| publish(artifact))?;
    outcome.warnings.extend(warnings);
    Ok(outcome)
}

pub(crate) fn prepare_codex_provider_deletion(
    db: &Database,
    provider_id: &str,
    current_provider_id: Option<&str>,
) -> Result<PreparedCodexProviderDeletion, AppError> {
    let providers = db
        .get_all_providers("codex")?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let Some(source_provider) = providers.get(provider_id) else {
        return Err(AppError::InvalidInput(format!(
            "Codex provider does not exist: {provider_id}"
        )));
    };
    let mut provider_ids = vec![provider_id.to_string()];
    if source_provider
        .settings_config
        .pointer("/codexProtocolSet/role")
        .and_then(serde_json::Value::as_str)
        == Some("facade")
    {
        super::provider_set::restore_logical_codex_provider(source_provider, &providers)
            .map_err(provider_set_error)?;
        let marker = source_provider
            .settings_config
            .get("codexProtocolSet")
            .and_then(serde_json::Value::as_object)
            .expect("validated Provider Set facade marker");
        for key in ["responsesProviderId", "chatProviderId"] {
            let member_id = marker
                .get(key)
                .and_then(serde_json::Value::as_str)
                .expect("validated Provider Set facade member");
            provider_ids.push(member_id.to_string());
        }
    }
    let provider_id_set = provider_ids.iter().cloned().collect::<HashSet<_>>();

    let mut prepared = Vec::new();
    let mut removed_candidates = BTreeSet::new();
    for router in providers.values() {
        if provider_id_set.contains(&router.id) {
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
                        .is_some_and(|target| provider_id_set.contains(target))
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
                .filter(|model| provider_id_set.contains(&model.target_provider_id))
                .map(|model| model.visible_model.clone()),
        );
        plan.routes
            .retain(|route| !provider_id_set.contains(&route.target_provider_id));
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
        let mut provider = router.clone();
        provider.settings_config["codexRouting"] = serde_json::to_value(plan).map_err(|error| {
            AppError::Database(format!(
                "Failed to serialize cascaded Codex routes: {error}"
            ))
        })?;
        remove_derived_catalog_fields(&mut provider.settings_config);
        prepared.push(PreparedRouterDeletion {
            router_id: router.id.clone(),
            provider,
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
    Ok(PreparedCodexProviderDeletion {
        provider_id: provider_id.to_string(),
        provider_ids,
        routers: prepared,
        active_router_id,
        must_restore_official,
        removed_candidates,
    })
}

pub(crate) fn finalize_codex_provider_deletion<F>(
    db: &Database,
    prepared: PreparedCodexProviderDeletion,
    mut publish: F,
) -> Result<CodexProviderDeleteOutcome, AppError>
where
    F: FnMut(&CodexRoutingProjectionArtifact) -> Result<ProjectionReadBack, String>,
{
    let publish_without_active_router =
        prepared.active_router_id.is_none() && prepared.routers.len() == 1;
    let mut projections = Vec::with_capacity(prepared.routers.len());
    for router in &prepared.routers {
        let owns_shared_projection = prepared.active_router_id.as_deref()
            == Some(router.router_id.as_str())
            || publish_without_active_router;
        if !owns_shared_projection || (prepared.active_router_id.is_some() && router.disabled) {
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
        deleted_provider_id: prepared.provider_id,
        affected_plan_ids: prepared
            .routers
            .iter()
            .map(|router| router.router_id.clone())
            .collect(),
        disabled_plan_ids: prepared
            .routers
            .iter()
            .filter(|router| router.disabled)
            .map(|router| router.router_id.clone())
            .collect(),
        removed_candidates: prepared.removed_candidates.into_iter().collect(),
        projections,
        warnings: Vec::new(),
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
                "config": "model = \"qwen3.8\"\nbase_url = \"https://qwen.example/v1\"\nwire_api = \"chat\"\n",
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

    fn verified_qwen_profile() -> ProtocolCompatibilityRecord {
        use crate::protocol_compatibility::{
            compile_provider_probe_candidate_for_model, ProtocolCompatibilityProbeResult,
            TransportKind,
        };

        let target_provider = target("openai_chat");
        let target = compile_provider_probe_candidate_for_model(
            &target_provider,
            "qwen3.8".to_string(),
            "qwen3.8".to_string(),
        )
        .expect("compile profile Provider policy")
        .target_key(TransportKind::OpenAiChat)
        .expect("profile target");
        let result: ProtocolCompatibilityProbeResult = serde_json::from_value(json!({
            "selected_transport": "open_ai_chat",
            "readiness": "verified",
            "branches": [{
                "assessment": {
                    "transport": "open_ai_chat",
                    "baseline": "passed",
                    "streaming": "passed",
                    "forced_tool": "passed",
                    "continuation": "passed"
                },
                "reasoning_shape": {
                    "semantic": "readable",
                    "source": "reasoning",
                    "pre_tool_visible_content": "absent"
                },
                "tool_schema_dialect": "moonshot_mfjs",
                "history_replay": "chat_reasoning_content",
                "evidence": []
            }]
        }))
        .expect("verified probe result");
        ProtocolCompatibilityRecord::new(target, result, 100, 200)
    }

    fn compiled_router_target(db: &Database) -> ProbeTargetKey {
        let providers = db
            .get_all_providers("codex")
            .expect("load providers")
            .into_iter()
            .collect::<HashMap<_, _>>();
        let router = providers.get("router-a").expect("router exists");
        compile_codex_router_probe_candidates(router, &providers)
            .expect("compile router candidates")
            .into_iter()
            .find(|candidate| candidate.public_model == "qwen3.8")
            .expect("qwen route candidate")
            .target_key(TransportKind::OpenAiChat)
            .expect("route target")
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

    fn mixed_source() -> Provider {
        let mut provider = Provider::with_id(
            "relay".to_string(),
            "Relay".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "secret"},
                "config": "model = \"model-a\"\nmodel_provider = \"relay\"\n[model_providers.relay]\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n",
                "apiFormat": "openai_responses",
                "modelCatalog": {"models": [
                    {"model": "model-a", "upstreamModel": "upstream-a"},
                    {"model": "model-b", "upstreamModel": "upstream-b"}
                ]}
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        provider
    }

    fn mixed_profile(
        model: &str,
        upstream_model: &str,
        transport: TransportKind,
        now: i64,
    ) -> ProtocolCompatibilityRecord {
        let target = crate::protocol_compatibility::compile_provider_probe_candidate_for_model(
            &mixed_source(),
            model.to_string(),
            upstream_model.to_string(),
        )
        .expect("compile mixed Provider candidate")
        .target_key(transport)
        .expect("mixed Provider target");
        ProtocolCompatibilityRecord::new(
            target,
            crate::protocol_compatibility::ProtocolCompatibilityProbeResult {
                selected_transport: Some(transport),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            now,
            now + 600,
        )
    }

    fn mixed_records(now: i64) -> Vec<ProtocolCompatibilityRecord> {
        vec![
            mixed_profile("model-a", "upstream-a", TransportKind::OpenAiResponses, now),
            mixed_profile("model-b", "upstream-b", TransportKind::OpenAiChat, now),
        ]
    }

    fn uniform_responses_records(now: i64) -> Vec<ProtocolCompatibilityRecord> {
        vec![
            mixed_profile("model-a", "upstream-a", TransportKind::OpenAiResponses, now),
            mixed_profile("model-b", "upstream-b", TransportKind::OpenAiResponses, now),
        ]
    }

    fn seed_split_provider_set(db: &Database, now: i64) -> Provider {
        let source = mixed_source();
        db.save_provider("codex", &source)
            .expect("seed logical source");
        let existing = db
            .get_all_providers("codex")
            .expect("load Providers")
            .into_iter()
            .collect::<HashMap<_, _>>();
        let prepared = crate::codex_multirouter::provider_set::plan_codex_provider_set(
            &source,
            &mixed_records(now),
            &existing,
            now,
        )
        .expect("prepare initial split");
        let commit = prepare_codex_provider_set_commit(db, prepared).expect("prepare split commit");
        db.apply_provider_set_database_transaction(commit.transaction)
            .expect("commit initial split");
        source
    }

    #[test]
    fn provider_set_commit_atomically_splits_and_rewrites_the_active_dependent_router() {
        let db = Database::memory().expect("memory db");
        let source = mixed_source();
        db.save_provider("codex", &source).expect("seed source");
        let mut outer = router("outer-router", "relay");
        outer.settings_config["codexRouting"]["defaultRouteId"] = json!("route-qwen");
        db.save_provider("codex", &outer)
            .expect("seed outer Router");
        db.set_current_provider("codex", "outer-router")
            .expect("activate outer Router");
        let now = chrono::Utc::now().timestamp();
        let records = mixed_records(now);
        let existing = db
            .get_all_providers("codex")
            .expect("load Providers")
            .into_iter()
            .collect::<HashMap<_, _>>();
        let prepared = crate::codex_multirouter::provider_set::plan_codex_provider_set(
            &source, &records, &existing, now,
        )
        .expect("prepare split");
        let published = RefCell::new(Vec::new());

        apply_codex_provider_set_mutation_with_publisher(&db, prepared, |artifact| {
            published
                .borrow_mut()
                .push(artifact.router_provider_id.clone());
            Ok(ProjectionReadBack::verified(
                artifact.dependency_fingerprint.clone(),
            ))
        })
        .expect("commit Provider Set");

        let facade = db
            .get_provider_by_id("relay", "codex")
            .expect("read facade")
            .expect("facade exists");
        assert_eq!(facade.settings_config["codexProtocolSet"]["role"], "facade");
        assert!(db
            .get_provider_by_id("relay--ccsm-responses", "codex")
            .expect("read Responses leaf")
            .is_some());
        assert!(db
            .get_provider_by_id("relay--ccsm-chat", "codex")
            .expect("read Chat leaf")
            .is_some());
        let outer = db
            .get_provider_by_id("outer-router", "codex")
            .expect("read outer Router")
            .expect("outer Router exists");
        let routes = outer.settings_config["codexRouting"]["routes"]
            .as_array()
            .expect("routes");
        assert_eq!(routes.len(), 2);
        assert!(routes
            .iter()
            .all(|route| route["targetProviderId"] != "relay"));
        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some("outer-router".to_string())
        );
        assert_eq!(published.into_inner(), vec!["outer-router".to_string()]);
    }

    #[test]
    fn wizard_batch_prepares_split_leaves_and_final_router_as_one_database_transaction() {
        let db = Database::memory().expect("memory db");
        let source = mixed_source();
        db.save_provider("codex", &source).expect("seed source");
        let now = chrono::Utc::now().timestamp();
        let records = mixed_records(now);
        let mut final_router = router("wizard-router", "relay");
        final_router.settings_config["codexRouting"]["defaultRouteId"] = json!("route-qwen");
        final_router.settings_config["codexRouting"]["subagentVersion"] = json!("v2");

        let prepared = prepare_codex_provider_set_batch_commit(
            &db,
            vec![(source, records)],
            final_router,
            now,
        )
        .expect("prepare one atomic wizard transaction");

        assert_eq!(prepared.source_previews.len(), 1);
        assert!(matches!(
            prepared.source_previews[0].plan,
            crate::codex_multirouter::provider_set::CodexProviderSetPlan::Split { .. }
        ));
        assert_eq!(prepared.router.id, "wizard-router");
        db.apply_provider_set_database_transaction(
            prepared
                .transaction
                .expect("unblocked batch has a transaction"),
        )
        .expect("commit the batch once");

        let saved_router = db
            .get_provider_by_id("wizard-router", "codex")
            .expect("read router")
            .expect("router exists");
        let routes = saved_router.settings_config["codexRouting"]["routes"]
            .as_array()
            .expect("routes");
        assert_eq!(routes.len(), 2);
        assert_eq!(
            routes
                .iter()
                .map(|route| route["targetProviderId"].as_str().expect("target"))
                .collect::<HashSet<_>>(),
            HashSet::from(["relay--ccsm-responses", "relay--ccsm-chat"])
        );
        assert!(db
            .get_provider_by_id("relay--ccsm-responses", "codex")
            .expect("read Responses leaf")
            .is_some());
        assert!(db
            .get_provider_by_id("relay--ccsm-chat", "codex")
            .expect("read Chat leaf")
            .is_some());
        let profiles = saved_router
            .settings_config
            .pointer("/codexRouting/subagentV2/profiles")
            .and_then(serde_json::Value::as_object)
            .expect("Subagent V2 profiles are initialized before the transaction");
        assert!(profiles.contains_key("model-a"));
        assert!(profiles.contains_key("model-b"));
    }

    #[test]
    fn wizard_batch_split_to_single_maps_a_current_generated_leaf_to_the_source() {
        let db = Database::memory().expect("memory db");
        let now = chrono::Utc::now().timestamp();
        let source = seed_split_provider_set(&db, now);
        db.set_current_provider("codex", "relay--ccsm-chat")
            .expect("activate generated Chat leaf");
        let mut final_router = router("wizard-router", "relay");
        final_router.settings_config["codexRouting"]["defaultRouteId"] = json!("route-qwen");

        let prepared = prepare_codex_provider_set_batch_commit(
            &db,
            vec![(source, uniform_responses_records(now))],
            final_router,
            now,
        )
        .expect("prepare uniform batch");
        db.apply_provider_set_database_transaction(
            prepared
                .transaction
                .expect("unblocked batch has a transaction"),
        )
        .expect("commit uniform batch");

        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some("relay".to_string()),
            "deleting the current generated leaf must atomically select its logical source"
        );
        assert!(db
            .get_provider_by_id("relay--ccsm-chat", "codex")
            .expect("read Chat leaf")
            .is_none());
    }

    #[test]
    fn provider_set_split_replan_maps_a_current_leaf_to_the_facade() {
        let db = Database::memory().expect("memory db");
        let now = chrono::Utc::now().timestamp();
        let source = seed_split_provider_set(&db, now);
        db.set_current_provider("codex", "relay--ccsm-responses")
            .expect("activate generated Responses leaf");
        let existing = db
            .get_all_providers("codex")
            .expect("load Providers")
            .into_iter()
            .collect::<HashMap<_, _>>();
        let prepared = crate::codex_multirouter::provider_set::plan_codex_provider_set(
            &source,
            &mixed_records(now),
            &existing,
            now,
        )
        .expect("replan split");

        apply_codex_provider_set_mutation_with_publisher(&db, prepared, |_| {
            Ok(ProjectionReadBack::verified("split-replan".to_string()))
        })
        .expect("commit split replan");

        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some("relay".to_string()),
            "an internal leaf must never remain the user-visible active Provider"
        );
    }

    #[test]
    fn failed_wizard_batch_rolls_back_the_current_leaf_transition() {
        let db = Database::memory().expect("memory db");
        let now = chrono::Utc::now().timestamp();
        let source = seed_split_provider_set(&db, now);
        db.set_current_provider("codex", "relay--ccsm-chat")
            .expect("activate generated Chat leaf");
        {
            let conn = db.conn.lock().expect("lock database");
            conn.execute_batch(
                "CREATE TRIGGER fail_current_normalization_batch
                 BEFORE INSERT ON providers
                 WHEN NEW.id = 'wizard-router'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected current normalization failure');
                 END;",
            )
            .expect("install failure trigger");
        }
        let mut final_router = router("wizard-router", "relay");
        final_router.settings_config["codexRouting"]["defaultRouteId"] = json!("route-qwen");
        let prepared = prepare_codex_provider_set_batch_commit(
            &db,
            vec![(source, uniform_responses_records(now))],
            final_router,
            now,
        )
        .expect("prepare uniform batch");

        let error = db
            .apply_provider_set_database_transaction(
                prepared
                    .transaction
                    .expect("unblocked batch has a transaction"),
            )
            .expect_err("injected router failure must abort the batch");

        assert!(error
            .to_string()
            .contains("injected current normalization failure"));
        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some("relay--ccsm-chat".to_string()),
            "the activity transition must roll back with the Provider mutations"
        );
        assert!(db
            .get_provider_by_id("relay--ccsm-chat", "codex")
            .expect("read Chat leaf")
            .is_some());
    }

    #[test]
    fn wizard_batch_failure_rolls_back_sources_leaves_profiles_and_final_router() {
        let db = Database::memory().expect("memory db");
        let source = mixed_source();
        db.save_provider("codex", &source).expect("seed source");
        {
            let conn = db.conn.lock().expect("lock database");
            conn.execute_batch(
                "CREATE TRIGGER fail_wizard_router
                 BEFORE INSERT ON providers
                 WHEN NEW.id = 'wizard-router'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected wizard router failure');
                 END;",
            )
            .expect("install failure trigger");
        }
        let now = chrono::Utc::now().timestamp();
        let mut final_router = router("wizard-router", "relay");
        final_router.settings_config["codexRouting"]["defaultRouteId"] = json!("route-qwen");
        let prepared = prepare_codex_provider_set_batch_commit(
            &db,
            vec![(source.clone(), mixed_records(now))],
            final_router,
            now,
        )
        .expect("prepare batch");

        let error = db
            .apply_provider_set_database_transaction(
                prepared
                    .transaction
                    .expect("unblocked batch has a transaction"),
            )
            .expect_err("router failure must roll back the entire batch");

        assert!(error.to_string().contains("injected wizard router failure"));
        let unchanged = db
            .get_provider_by_id("relay", "codex")
            .expect("read source")
            .expect("source remains");
        assert_eq!(unchanged.settings_config, source.settings_config);
        for absent in ["relay--ccsm-responses", "relay--ccsm-chat", "wizard-router"] {
            assert!(db
                .get_provider_by_id(absent, "codex")
                .expect("read Provider")
                .is_none());
        }
        let profile_count: i64 = db
            .conn
            .lock()
            .expect("lock database")
            .query_row(
                "SELECT COUNT(*) FROM protocol_compatibility_profiles",
                [],
                |row| row.get(0),
            )
            .expect("count profiles");
        assert_eq!(profile_count, 0);
    }

    #[test]
    fn provider_set_commit_rejects_changed_dependencies_before_any_write() {
        let db = Database::memory().expect("memory db");
        let source = mixed_source();
        db.save_provider("codex", &source).expect("seed source");
        let outer = router("outer-router", "relay");
        db.save_provider("codex", &outer)
            .expect("seed outer Router");
        let now = chrono::Utc::now().timestamp();
        let records = mixed_records(now);
        let existing = db
            .get_all_providers("codex")
            .expect("load Providers")
            .into_iter()
            .collect::<HashMap<_, _>>();
        let prepared = crate::codex_multirouter::provider_set::plan_codex_provider_set(
            &source, &records, &existing, now,
        )
        .expect("prepare split");
        let mut changed = outer;
        changed.settings_config["codexRouting"]["routes"][0]["label"] = json!("concurrent edit");
        db.save_provider("codex", &changed)
            .expect("commit concurrent Router edit");

        let error = apply_codex_provider_set_mutation_with_publisher(&db, prepared, |_| {
            panic!("stale Provider Set must not publish")
        })
        .expect_err("dependency change must reject commit");
        assert!(error
            .to_string()
            .contains("codex_provider_set_dependency_changed"));
        assert!(db
            .get_provider_by_id("relay--ccsm-responses", "codex")
            .expect("read Responses leaf")
            .is_none());
        assert!(db
            .get_provider_by_id("relay--ccsm-chat", "codex")
            .expect("read Chat leaf")
            .is_none());
        assert_eq!(
            db.get_provider_by_id("outer-router", "codex")
                .expect("read Router")
                .expect("Router exists")
                .settings_config["codexRouting"]["routes"][0]["label"],
            "concurrent edit"
        );
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
    fn provider_verified_protocol_profile_is_materialized_for_equivalent_router_target() {
        use crate::protocol_compatibility::ReasoningProjection;

        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");

        apply_codex_provider_mutation_with_profiles_and_publisher(
            &db,
            target("openai_chat"),
            &[verified_qwen_profile()],
            &[],
            |artifact| {
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            },
        )
        .expect("save provider profile and rebuild router");

        let route_target = compiled_router_target(&db);
        let route_profile = db
            .get_protocol_compatibility_result(&route_target)
            .expect("read route profile")
            .expect("equivalent route profile must be materialized");

        assert_eq!(
            route_profile.automatic_reasoning_projection(150),
            ReasoningProjection::RawReasoningText
        );
        assert_eq!(
            route_profile.result.branches[0].tool_schema_dialect,
            crate::protocol_compatibility::ToolSchemaDialect::MoonshotMfjs
        );
        assert_eq!(
            route_profile.result.branches[0].history_replay,
            crate::protocol_compatibility::HistoryReplay::ChatReasoningContent
        );
    }

    #[test]
    fn provider_protocol_profile_is_not_materialized_when_route_credentials_differ() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");
        let mut mismatched = verified_qwen_profile();
        mismatched.target = mismatched.target.with_credential("different-secret");

        apply_codex_provider_mutation_with_profiles_and_publisher(
            &db,
            target("openai_chat"),
            &[mismatched],
            &[],
            |artifact| {
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            },
        )
        .expect("save provider profile without borrowing it for the route");

        let route_target = compiled_router_target(&db);
        assert!(db
            .get_protocol_compatibility_result(&route_target)
            .expect("read route profile")
            .is_none());
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
    fn subagent_profile_sync_failure_rolls_back_the_provider_mutation() -> Result<(), AppError> {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
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
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER fail_router_subagent_profile_update
                 BEFORE UPDATE ON providers
                 WHEN NEW.app_type = 'codex' AND NEW.id = 'router-a'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected router subagent profile failure');
                 END;",
            )
            .expect("install Router profile failure trigger");
        }

        let mut updated = target("openai_responses");
        updated.settings_config["modelCatalog"]["models"] =
            json!([{"model": "qwen3.8"}, {"model": "qwen3.9"}]);
        let error = apply_codex_provider_mutation_with_publisher(&db, updated, |_| {
            panic!("a failed database transaction must not publish a live projection")
        })
        .expect_err("the injected Router profile failure must abort the Provider mutation");

        assert!(error
            .to_string()
            .contains("injected router subagent profile failure"));
        let saved_target = db
            .get_provider_by_id("qwen", "codex")
            .expect("read target")
            .expect("target remains");
        assert_eq!(
            saved_target.meta.and_then(|meta| meta.api_format),
            Some("openai_chat".to_string()),
            "Provider facts must roll back with the dependent Router profile"
        );
        let saved_router = db
            .get_provider_by_id("router-a", "codex")
            .expect("read router")
            .expect("router remains");
        assert!(
            saved_router.settings_config["codexRouting"]["subagentV2"]["profiles"]
                .get("qwen3.9")
                .is_none()
        );
        Ok(())
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
    fn deleting_target_removes_owned_and_removed_route_protocol_state() {
        use crate::protocol_compatibility::{
            HistoryReplay, ManualReasoningOverride, ReasoningProjection, ReasoningSemantic,
            ReasoningSource,
        };

        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");

        let provider_profile = verified_qwen_profile();
        let provider_target = provider_profile.target.clone();
        let route_target = compiled_router_target(&db);
        let mut route_profile = provider_profile.clone();
        route_profile.target = route_target.clone();
        db.save_protocol_compatibility_result(&provider_profile)
            .expect("seed provider profile");
        db.save_protocol_compatibility_result(&route_profile)
            .expect("seed routed profile");

        let unrelated_target = ProbeTargetKey::new(
            "unrelated-provider",
            Some("unrelated-route"),
            "unrelated-model",
            "unrelated-model",
            TransportKind::OpenAiChat,
            "https://unrelated.example/v1/chat/completions",
            "bearer",
        )
        .expect("unrelated target");
        let mut unrelated_profile = provider_profile.clone();
        unrelated_profile.target = unrelated_target.clone();
        db.save_protocol_compatibility_result(&unrelated_profile)
            .expect("seed unrelated profile");

        let override_spec = ManualReasoningOverride::new(
            ReasoningSemantic::Readable,
            ReasoningSource::ReasoningContent,
            HistoryReplay::ChatReasoningContent,
        );
        for target in [&provider_target, &route_target, &unrelated_target] {
            db.save_reasoning_manual_override(
                target,
                override_spec,
                ReasoningProjection::RawReasoningText,
                "advanced override",
                150,
                0,
            )
            .expect("seed reasoning override");
        }

        apply_codex_provider_delete_with_hooks(
            &db,
            "qwen",
            None,
            || Ok(()),
            |artifact| {
                Ok(ProjectionReadBack::verified(
                    artifact.dependency_fingerprint.clone(),
                ))
            },
        )
        .expect("delete target");

        for removed in [&provider_target, &route_target] {
            assert_eq!(
                db.get_protocol_compatibility_result(removed)
                    .expect("read removed profile"),
                None,
                "deleted Provider and route profiles must not survive"
            );
            assert_eq!(
                db.get_reasoning_manual_override(removed)
                    .expect("read removed override"),
                None,
                "deleted Provider and route overrides must not survive"
            );
        }
        assert!(db
            .get_protocol_compatibility_result(&unrelated_target)
            .expect("read unrelated profile")
            .is_some());
        assert!(db
            .get_reasoning_manual_override(&unrelated_target)
            .expect("read unrelated override")
            .is_some());
    }

    #[test]
    fn failed_official_projection_is_reported_after_authoritative_deletion_commits() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");
        db.set_current_provider("codex", "router-a")
            .expect("activate dependent Router");

        let outcome = apply_codex_provider_delete_with_hooks(
            &db,
            "qwen",
            Some("router-a"),
            || Err(AppError::Message("restore failed".to_string())),
            |_| panic!("failed restore must not publish"),
        )
        .expect("derived official projection failure must not roll back the database");

        assert_eq!(
            outcome.warnings,
            vec!["codex_official_projection_pending_retry".to_string()]
        );
        assert!(db
            .get_provider_by_id("qwen", "codex")
            .expect("read target")
            .is_none());
        assert_eq!(
            db.get_provider_by_id("router-a", "codex")
                .expect("read router")
                .expect("router exists")
                .settings_config["codexRouting"]["enabled"],
            false
        );
        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some(crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string())
        );
    }

    #[test]
    fn deletion_transaction_failure_does_not_project_official_early() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");
        db.set_current_provider("codex", "router-a")
            .expect("activate dependent Router");
        {
            let conn = db.conn.lock().expect("lock database");
            conn.execute_batch(
                "CREATE TRIGGER fail_disabled_router_update
                 BEFORE UPDATE ON providers
                 WHEN NEW.id = 'router-a'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected deletion transaction failure');
                 END;",
            )
            .expect("install failure trigger");
        }
        let official_projection_calls = Cell::new(0);

        let error = apply_codex_provider_delete_with_hooks(
            &db,
            "qwen",
            Some("router-a"),
            || {
                official_projection_calls.set(official_projection_calls.get() + 1);
                Ok(())
            },
            |_| panic!("failed transaction must not publish a Router projection"),
        )
        .expect_err("injected database failure must abort deletion");

        assert!(error
            .to_string()
            .contains("injected deletion transaction failure"));
        assert_eq!(
            official_projection_calls.get(),
            0,
            "official live projection belongs after the database commit"
        );
        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some("router-a".to_string())
        );
        assert!(db
            .get_provider_by_id(crate::database::CODEX_OFFICIAL_PROVIDER_ID, "codex")
            .expect("read official seed")
            .is_none());
    }

    #[test]
    fn successful_deletion_seeds_and_selects_official_inside_the_transaction() {
        let db = Database::memory().expect("memory db");
        db.save_provider("codex", &target("openai_chat"))
            .expect("seed target");
        db.save_provider("codex", &router("router-a", "qwen"))
            .expect("seed router");
        db.set_current_provider("codex", "router-a")
            .expect("activate dependent Router");
        let projection_observed_current = RefCell::new(None);

        apply_codex_provider_delete_with_hooks(
            &db,
            "qwen",
            Some("router-a"),
            || {
                *projection_observed_current.borrow_mut() = db
                    .get_current_provider("codex")
                    .expect("read committed current");
                Ok(())
            },
            |_| panic!("disabled active Router must not own the live projection"),
        )
        .expect("delete target");

        assert!(db
            .get_provider_by_id(crate::database::CODEX_OFFICIAL_PROVIDER_ID, "codex")
            .expect("read official seed")
            .is_some());
        assert_eq!(
            db.get_current_provider("codex").expect("read current"),
            Some(crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string())
        );
        assert_eq!(
            projection_observed_current.into_inner(),
            Some(crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string()),
            "the live projection hook must observe the committed official current state"
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
