use super::schema::{
    CodexModelSelection, CodexRoutingConfigV2, CodexRoutingDocument, CodexRoutingRouteV2,
};
use crate::protocol_compatibility::{
    compile_provider_probe_candidate_for_model, ProbeFailureKind, ProbeProgressStage,
    ProbeReadiness, ProtocolCompatibilityRecord, TransportKind, PROBE_PROFILE_VERSION,
};
use crate::provider::Provider;
use crate::proxy::providers::codex_provider_upstream_model;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const CODEX_PROTOCOL_SET_VERSION: u32 = 1;
const RESPONSES_LEAF_SUFFIX: &str = "--ccsm-responses";
const CHAT_LEAF_SUFFIX: &str = "--ccsm-chat";
const RESPONSES_ROUTE_SUFFIX: &str = "--ccsm-responses-route";
const CHAT_ROUTE_SUFFIX: &str = "--ccsm-chat-route";
const DEPENDENT_RESPONSES_ROUTE_SUFFIX: &str = "--ccsm-provider-set-responses";
const DEPENDENT_CHAT_ROUTE_SUFFIX: &str = "--ccsm-provider-set-chat";
const GENERATED_ROUTES_EXTENSION: &str = "ccsmProviderSetGeneratedRoutes";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSetPreview {
    pub digest: String,
    pub source_provider_id: String,
    pub responses_models: Vec<String>,
    pub chat_models: Vec<String>,
    pub plan: CodexProviderSetPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodexProviderSetPlan {
    Single {
        transport: TransportKind,
    },
    Split {
        responses_provider_id: String,
        chat_provider_id: String,
    },
    Blocked {
        models: Vec<CodexProviderSetBlockedModel>,
    },
}

#[derive(Debug, Clone)]
pub struct PreparedCodexProviderSetMutation {
    pub preview: CodexProviderSetPreview,
    pub persistence: CodexProviderSetPersistence,
    pub profiles: Vec<ProtocolCompatibilityRecord>,
    pub delete_provider_ids: Vec<String>,
    pub replace_profile_provider_ids: HashSet<String>,
    pub router_updates: Vec<Provider>,
    source_draft: Provider,
    probe_records: Vec<ProtocolCompatibilityRecord>,
    prepared_at: i64,
    planning_mode: CodexProviderSetPlanningMode,
}

#[derive(Debug, Clone, Copy)]
enum CodexProviderSetPlanningMode {
    Automatic,
    Manual(TransportKind),
}

#[derive(Debug, Clone)]
pub enum CodexProviderSetPersistence {
    Single {
        transport: TransportKind,
        provider: Provider,
    },
    Split {
        facade: Provider,
        responses_provider: Provider,
        chat_provider: Provider,
    },
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSetBlockedModel {
    pub model: String,
    pub upstream_model: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<ProbeProgressStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<ProbeFailureKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSetError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for CodexProviderSetError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CodexProviderSetError {}

#[derive(Clone)]
struct CatalogModel {
    public_model: String,
    upstream_model: String,
    enabled: bool,
    value: Value,
}

pub fn responses_leaf_id(source_provider_id: &str) -> String {
    format!("{source_provider_id}{RESPONSES_LEAF_SUFFIX}")
}

pub fn chat_leaf_id(source_provider_id: &str) -> String {
    format!("{source_provider_id}{CHAT_LEAF_SUFFIX}")
}

/// Returns the logical parent ID only for a structurally valid generated leaf marker.
///
/// Provider Set leaves are persistence/runtime implementation details. User-facing service and
/// command boundaries use this function as the single identity predicate instead of duplicating
/// marker parsing or relying on deterministic ID suffixes.
pub fn codex_provider_set_leaf_parent_id(provider: &Provider) -> Option<&str> {
    let marker = provider
        .settings_config
        .get("codexProtocolSet")
        .and_then(Value::as_object)?;
    if marker.get("version").and_then(Value::as_u64) != Some(CODEX_PROTOCOL_SET_VERSION as u64)
        || marker.get("role").and_then(Value::as_str) != Some("leaf")
        || !matches!(
            marker.get("transport").and_then(Value::as_str),
            Some("open_ai_responses" | "open_ai_chat")
        )
    {
        return None;
    }
    marker
        .get("parentProviderId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|parent_id| !parent_id.is_empty())
}

pub fn is_codex_provider_set_generated_leaf(provider: &Provider) -> bool {
    codex_provider_set_leaf_parent_id(provider).is_some()
}

pub fn is_codex_provider_set_facade(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("codexProtocolSet")
        .and_then(Value::as_object)
        .is_some_and(|marker| {
            marker.get("version").and_then(Value::as_u64) == Some(CODEX_PROTOCOL_SET_VERSION as u64)
                && marker.get("role").and_then(Value::as_str) == Some("facade")
        })
}

pub fn plan_codex_provider_set(
    source: &Provider,
    records: &[ProtocolCompatibilityRecord],
    existing_providers: &HashMap<String, Provider>,
    now: i64,
) -> Result<PreparedCodexProviderSetMutation, CodexProviderSetError> {
    let catalog = source_catalog(source)?;
    let mut blocked = Vec::new();
    let mut selections = Vec::new();
    let mut enabled_names = HashSet::new();

    for model in catalog.iter().filter(|model| model.enabled) {
        if !enabled_names.insert(model.public_model.clone()) {
            blocked.push(blocked_model(model, "duplicate_model_identity", None));
            continue;
        }
        let matches = records
            .iter()
            .filter(|record| {
                record.target.public_model == model.public_model
                    && record.target.upstream_model == model.upstream_model
            })
            .collect::<Vec<_>>();
        let Some(record) = matches.first().copied() else {
            blocked.push(blocked_model(model, "probe_required", None));
            continue;
        };
        if matches.len() != 1 {
            blocked.push(blocked_model(model, "conflicting_probe_records", None));
            continue;
        }
        if record.probe_version != PROBE_PROFILE_VERSION || record.expires_at < now {
            blocked.push(blocked_model(model, "probe_stale", Some(record)));
            continue;
        }
        if record.result.readiness != ProbeReadiness::Verified {
            blocked.push(blocked_model(model, "probe_not_verified", Some(record)));
            continue;
        }
        let Some(selected) = record.result.selected_transport else {
            blocked.push(blocked_model(model, "probe_has_no_selection", Some(record)));
            continue;
        };
        if record.target.transport != selected {
            blocked.push(blocked_model(model, "probe_target_mismatch", Some(record)));
            continue;
        }
        selections.push((model.clone(), selected));
    }

    if selections.is_empty() && blocked.is_empty() {
        return Err(error(
            "codex_provider_set_model_catalog_required",
            "Codex Provider has no enabled catalog model",
        ));
    }

    if !blocked.is_empty() {
        let digest = preview_digest(source, records, &blocked)?;
        return Ok(PreparedCodexProviderSetMutation {
            preview: CodexProviderSetPreview {
                digest,
                source_provider_id: source.id.clone(),
                responses_models: Vec::new(),
                chat_models: Vec::new(),
                plan: CodexProviderSetPlan::Blocked { models: blocked },
            },
            persistence: CodexProviderSetPersistence::Blocked,
            profiles: Vec::new(),
            delete_provider_ids: Vec::new(),
            replace_profile_provider_ids: HashSet::new(),
            router_updates: Vec::new(),
            source_draft: source.clone(),
            probe_records: records.to_vec(),
            prepared_at: now,
            planning_mode: CodexProviderSetPlanningMode::Automatic,
        });
    }

    let responses_models = selections
        .iter()
        .filter(|(_, transport)| *transport == TransportKind::OpenAiResponses)
        .map(|(model, _)| model.public_model.clone())
        .collect::<Vec<_>>();
    let chat_models = selections
        .iter()
        .filter(|(_, transport)| *transport == TransportKind::OpenAiChat)
        .map(|(model, _)| model.public_model.clone())
        .collect::<Vec<_>>();

    let persistence = if chat_models.is_empty() || responses_models.is_empty() {
        let transport = selections[0].1;
        let mut provider = source.clone();
        remove_provider_set_fields(&mut provider);
        normalize_provider_transport(&mut provider, transport)?;
        CodexProviderSetPersistence::Single {
            transport,
            provider,
        }
    } else {
        validate_leaf_ownership(source, existing_providers)?;
        let responses_catalog = selections
            .iter()
            .filter(|(_, transport)| *transport == TransportKind::OpenAiResponses)
            .map(|(model, _)| model.value.clone())
            .collect::<Vec<_>>();
        let chat_catalog = selections
            .iter()
            .filter(|(_, transport)| *transport == TransportKind::OpenAiChat)
            .map(|(model, _)| model.value.clone())
            .collect::<Vec<_>>();
        let responses_provider =
            build_leaf(source, TransportKind::OpenAiResponses, responses_catalog)?;
        let chat_provider = build_leaf(source, TransportKind::OpenAiChat, chat_catalog)?;
        let default_transport = source_default_transport(source, &selections)?;
        let facade = build_facade(
            source,
            &responses_provider,
            &chat_provider,
            &responses_models,
            &chat_models,
            default_transport,
        )?;
        CodexProviderSetPersistence::Split {
            facade,
            responses_provider,
            chat_provider,
        }
    };
    let router_updates =
        rewrite_dependent_routers_for_provider_set(source, &persistence, existing_providers)?;
    let digest = preview_digest(
        source,
        records,
        &json!({
            "persistence": persistence_digest_value(&persistence),
            "routerUpdates": router_updates,
        }),
    )?;
    let plan = match &persistence {
        CodexProviderSetPersistence::Single { transport, .. } => CodexProviderSetPlan::Single {
            transport: *transport,
        },
        CodexProviderSetPersistence::Split {
            responses_provider,
            chat_provider,
            ..
        } => CodexProviderSetPlan::Split {
            responses_provider_id: responses_provider.id.clone(),
            chat_provider_id: chat_provider.id.clone(),
        },
        CodexProviderSetPersistence::Blocked => unreachable!("blocked plans return above"),
    };
    let profiles = rebind_profiles(&persistence, records, &responses_models, &chat_models)?;
    let delete_provider_ids = match &persistence {
        CodexProviderSetPersistence::Single { .. } => {
            owned_existing_leaf_ids(source, existing_providers)
        }
        CodexProviderSetPersistence::Split { .. } | CodexProviderSetPersistence::Blocked => {
            Vec::new()
        }
    };
    let mut replace_profile_provider_ids = [source.id.clone()].into_iter().collect::<HashSet<_>>();
    match &persistence {
        CodexProviderSetPersistence::Single { .. } => {
            replace_profile_provider_ids.extend(delete_provider_ids.iter().cloned());
        }
        CodexProviderSetPersistence::Split {
            responses_provider,
            chat_provider,
            ..
        } => {
            replace_profile_provider_ids.insert(responses_provider.id.clone());
            replace_profile_provider_ids.insert(chat_provider.id.clone());
        }
        CodexProviderSetPersistence::Blocked => {}
    }
    Ok(PreparedCodexProviderSetMutation {
        preview: CodexProviderSetPreview {
            digest,
            source_provider_id: source.id.clone(),
            responses_models,
            chat_models,
            plan,
        },
        persistence,
        profiles,
        delete_provider_ids,
        replace_profile_provider_ids,
        router_updates,
        source_draft: source.clone(),
        probe_records: records.to_vec(),
        prepared_at: now,
        planning_mode: CodexProviderSetPlanningMode::Automatic,
    })
}

pub fn plan_manual_codex_provider_set(
    source: &Provider,
    transport: TransportKind,
    existing_providers: &HashMap<String, Provider>,
    now: i64,
) -> Result<PreparedCodexProviderSetMutation, CodexProviderSetError> {
    let mut provider = source.clone();
    remove_provider_set_fields(&mut provider);
    normalize_provider_transport(&mut provider, transport)?;
    let persistence = CodexProviderSetPersistence::Single {
        transport,
        provider,
    };
    let router_updates =
        rewrite_dependent_routers_for_provider_set(source, &persistence, existing_providers)?;
    let digest = preview_digest(
        source,
        &[],
        &json!({
            "planningMode": "manual",
            "persistence": persistence_digest_value(&persistence),
            "routerUpdates": router_updates,
        }),
    )?;
    let delete_provider_ids = owned_existing_leaf_ids(source, existing_providers);
    let mut replace_profile_provider_ids = [source.id.clone()].into_iter().collect::<HashSet<_>>();
    replace_profile_provider_ids.extend(delete_provider_ids.iter().cloned());

    Ok(PreparedCodexProviderSetMutation {
        preview: CodexProviderSetPreview {
            digest,
            source_provider_id: source.id.clone(),
            responses_models: Vec::new(),
            chat_models: Vec::new(),
            plan: CodexProviderSetPlan::Single { transport },
        },
        persistence,
        profiles: Vec::new(),
        delete_provider_ids,
        replace_profile_provider_ids,
        router_updates,
        source_draft: source.clone(),
        probe_records: Vec::new(),
        prepared_at: now,
        planning_mode: CodexProviderSetPlanningMode::Manual(transport),
    })
}

impl PreparedCodexProviderSetMutation {
    pub fn source_draft(&self) -> &Provider {
        &self.source_draft
    }

    pub fn probe_records(&self) -> &[ProtocolCompatibilityRecord] {
        &self.probe_records
    }

    pub fn prepared_at(&self) -> i64 {
        self.prepared_at
    }

    pub(crate) fn manual_transport(&self) -> Option<TransportKind> {
        match self.planning_mode {
            CodexProviderSetPlanningMode::Automatic => None,
            CodexProviderSetPlanningMode::Manual(transport) => Some(transport),
        }
    }
}

fn rebind_profiles(
    persistence: &CodexProviderSetPersistence,
    records: &[ProtocolCompatibilityRecord],
    responses_models: &[String],
    chat_models: &[String],
) -> Result<Vec<ProtocolCompatibilityRecord>, CodexProviderSetError> {
    let mut rebound = Vec::with_capacity(responses_models.len() + chat_models.len());
    for record in records {
        let selected = record.result.selected_transport.ok_or_else(|| {
            error(
                "codex_provider_set_probe_required",
                "Probe selection is missing",
            )
        })?;
        let in_selected_group = match selected {
            TransportKind::OpenAiResponses => {
                responses_models.contains(&record.target.public_model)
            }
            TransportKind::OpenAiChat => chat_models.contains(&record.target.public_model),
        };
        if !in_selected_group {
            continue;
        }
        let provider = match persistence {
            CodexProviderSetPersistence::Single { provider, .. } => provider,
            CodexProviderSetPersistence::Split {
                responses_provider,
                chat_provider,
                ..
            } => match selected {
                TransportKind::OpenAiResponses => responses_provider,
                TransportKind::OpenAiChat => chat_provider,
            },
            CodexProviderSetPersistence::Blocked => {
                return Err(error(
                    "codex_provider_set_model_blocked",
                    "Blocked Provider Set cannot materialize executable profiles",
                ));
            }
        };
        let candidate = compile_provider_probe_candidate_for_model(
            provider,
            record.target.public_model.clone(),
            record.target.upstream_model.clone(),
        )
        .map_err(|message| error("codex_provider_set_profile_rebind_failed", message))?;
        let target = candidate
            .target_key(selected)
            .map_err(|message| error("codex_provider_set_profile_rebind_failed", message))?;
        let mut rebound_record = record.clone();
        rebound_record.target = target;
        rebound.push(rebound_record);
    }
    if rebound.len() != responses_models.len() + chat_models.len() {
        return Err(error(
            "codex_provider_set_profile_rebind_failed",
            "Every selected model must materialize exactly one executable profile",
        ));
    }
    Ok(rebound)
}

fn persistence_digest_value(persistence: &CodexProviderSetPersistence) -> Value {
    match persistence {
        CodexProviderSetPersistence::Single {
            transport,
            provider,
        } => json!({
            "kind": "single",
            "transport": transport,
            "provider": provider,
        }),
        CodexProviderSetPersistence::Split {
            facade,
            responses_provider,
            chat_provider,
        } => json!({
            "kind": "split",
            "facade": facade,
            "responsesProvider": responses_provider,
            "chatProvider": chat_provider,
        }),
        CodexProviderSetPersistence::Blocked => json!({"kind": "blocked"}),
    }
}

pub fn restore_logical_codex_provider(
    facade: &Provider,
    existing_providers: &HashMap<String, Provider>,
) -> Result<Provider, CodexProviderSetError> {
    let marker = facade
        .settings_config
        .get("codexProtocolSet")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            error(
                "codex_provider_set_not_facade",
                "Provider does not contain a Codex Provider Set facade marker",
            )
        })?;
    if marker.get("version").and_then(Value::as_u64) != Some(CODEX_PROTOCOL_SET_VERSION as u64)
        || marker.get("role").and_then(Value::as_str) != Some("facade")
    {
        return Err(error(
            "codex_provider_set_not_facade",
            "Provider Set marker is not a supported facade",
        ));
    }
    let responses_id = marker
        .get("responsesProviderId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            error(
                "codex_provider_set_invalid_facade",
                "Responses leaf ID is missing",
            )
        })?;
    let chat_id = marker
        .get("chatProviderId")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            error(
                "codex_provider_set_invalid_facade",
                "Chat leaf ID is missing",
            )
        })?;
    validate_owned_leaf(
        facade,
        existing_providers.get(responses_id),
        responses_id,
        TransportKind::OpenAiResponses,
    )?;
    validate_owned_leaf(
        facade,
        existing_providers.get(chat_id),
        chat_id,
        TransportKind::OpenAiChat,
    )?;
    let source_catalog = marker.get("sourceModelCatalog").cloned().ok_or_else(|| {
        error(
            "codex_provider_set_invalid_facade",
            "Facade does not contain its authoritative source model catalog",
        )
    })?;

    let mut restored = facade.clone();
    let settings = settings_object_mut(&mut restored)?;
    settings.remove("codexRouting");
    settings.remove("codexProtocolSet");
    settings.insert("modelCatalog".to_string(), source_catalog);
    Ok(restored)
}

pub fn rewrite_dependent_routers_for_provider_set(
    source: &Provider,
    persistence: &CodexProviderSetPersistence,
    existing_providers: &HashMap<String, Provider>,
) -> Result<Vec<Provider>, CodexProviderSetError> {
    let mut provider_ids = existing_providers.keys().cloned().collect::<Vec<_>>();
    provider_ids.sort();
    let mut updates = Vec::new();
    for provider_id in provider_ids {
        if provider_id == source.id {
            continue;
        }
        let Some(provider) = existing_providers.get(&provider_id) else {
            continue;
        };
        let Some(routing) = provider.settings_config.get("codexRouting") else {
            continue;
        };
        let CodexRoutingDocument::V2(mut plan) = CodexRoutingDocument::parse(routing)
            .map_err(|parse_error| error(parse_error.code, parse_error.message))?
        else {
            continue;
        };
        let changed = match persistence {
            CodexProviderSetPersistence::Split {
                responses_provider,
                chat_provider,
                ..
            } => expand_router_routes_for_split(
                source,
                responses_provider,
                chat_provider,
                &mut plan,
            )?,
            CodexProviderSetPersistence::Single { .. } => {
                fold_router_routes_for_single(source, &mut plan)?
            }
            CodexProviderSetPersistence::Blocked => false,
        };
        if !changed {
            continue;
        }
        ensure_unique_route_ids(&plan)?;
        let mut updated = provider.clone();
        updated.settings_config["codexRouting"] =
            serde_json::to_value(plan).map_err(|encode_error| {
                error(
                    "codex_provider_set_router_serialize_failed",
                    format!("Dependent MultiRouter cannot be serialized: {encode_error}"),
                )
            })?;
        updates.push(updated);
    }
    Ok(updates)
}

fn expand_router_routes_for_split(
    source: &Provider,
    responses_provider: &Provider,
    chat_provider: &Provider,
    plan: &mut CodexRoutingConfigV2,
) -> Result<bool, CodexProviderSetError> {
    let transport_by_model = provider_set_transport_by_model(responses_provider, chat_provider)?;
    let source_default_transport = codex_provider_upstream_model(source)
        .as_deref()
        .and_then(|model| transport_by_model.get(&model.to_ascii_lowercase()).copied());
    let existing_markers = generated_route_markers(plan);
    let mut markers = existing_markers.clone();
    let mut rewritten = Vec::with_capacity(plan.routes.len() + 1);
    let mut changed = false;
    let original_default = plan.default_route_id.clone();
    let mut rewritten_default = original_default.clone();

    for route in std::mem::take(&mut plan.routes) {
        if route.target_provider_id != source.id {
            rewritten.push(route);
            continue;
        }
        changed = true;
        let original_route_id = route.id.clone();
        let (responses_selection, chat_selection) =
            partition_model_selection(&route.model_selection, &transport_by_model)?;
        let responses_aliases = partition_aliases(
            &route.aliases,
            TransportKind::OpenAiResponses,
            &transport_by_model,
        )?;
        let chat_aliases = partition_aliases(
            &route.aliases,
            TransportKind::OpenAiChat,
            &transport_by_model,
        )?;
        let mut generated = Vec::new();
        if let Some(selection) = responses_selection {
            let generated_route = generated_route(
                &route,
                responses_provider,
                selection,
                responses_aliases,
                TransportKind::OpenAiResponses,
            );
            markers.insert(
                generated_route.id.clone(),
                generated_route_marker(source, &route, TransportKind::OpenAiResponses),
            );
            generated.push((TransportKind::OpenAiResponses, generated_route));
        }
        if let Some(selection) = chat_selection {
            let generated_route = generated_route(
                &route,
                chat_provider,
                selection,
                chat_aliases,
                TransportKind::OpenAiChat,
            );
            markers.insert(
                generated_route.id.clone(),
                generated_route_marker(source, &route, TransportKind::OpenAiChat),
            );
            generated.push((TransportKind::OpenAiChat, generated_route));
        }
        if generated.is_empty() {
            return Err(error(
                "codex_provider_set_router_expansion_ambiguous",
                format!(
                    "Dependent route `{original_route_id}` does not select a model in either generated Provider"
                ),
            ));
        }
        if original_default.as_deref() == Some(original_route_id.as_str()) {
            let default_transport = source_default_transport.ok_or_else(|| {
                error(
                    "codex_provider_set_router_expansion_ambiguous",
                    format!(
                        "Dependent default route `{original_route_id}` cannot be mapped because the logical source default model has no selected protocol"
                    ),
                )
            })?;
            rewritten_default = generated
                .iter()
                .find(|(transport, _)| *transport == default_transport)
                .map(|(_, route)| route.id.clone())
                .ok_or_else(|| {
                    error(
                        "codex_provider_set_router_expansion_ambiguous",
                        format!(
                            "Dependent default route `{original_route_id}` does not include the logical source default model"
                        ),
                    )
                })
                .map(Some)?;
        }
        rewritten.extend(generated.into_iter().map(|(_, route)| route));
    }
    plan.routes = rewritten;
    if changed {
        plan.default_route_id = rewritten_default;
        set_generated_route_markers(plan, markers);
    }
    Ok(changed)
}

fn fold_router_routes_for_single(
    source: &Provider,
    plan: &mut CodexRoutingConfigV2,
) -> Result<bool, CodexProviderSetError> {
    let mut markers = generated_route_markers(plan);
    let source_marker_ids = markers
        .iter()
        .filter_map(|(route_id, marker)| {
            (marker.get("sourceProviderId").and_then(Value::as_str) == Some(source.id.as_str()))
                .then_some(route_id.clone())
        })
        .collect::<HashSet<_>>();
    if source_marker_ids.is_empty() {
        let leaf_ids = [responses_leaf_id(&source.id), chat_leaf_id(&source.id)];
        if plan
            .routes
            .iter()
            .any(|route| leaf_ids.contains(&route.target_provider_id))
        {
            return Err(error(
                "codex_provider_set_dependency_changed",
                "A dependent MultiRouter directly references a generated leaf without an ownership marker",
            ));
        }
        return Ok(false);
    }

    let mut groups = BTreeMap::<String, Vec<CodexRoutingRouteV2>>::new();
    let mut group_order = Vec::new();
    let mut untouched = Vec::new();
    for route in std::mem::take(&mut plan.routes) {
        if !source_marker_ids.contains(&route.id) {
            untouched.push((None, route));
            continue;
        }
        let marker = markers.get(&route.id).expect("marker ID was collected");
        let original_route_id = marker
            .get("originalRouteId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                error(
                    "codex_provider_set_dependency_changed",
                    "Generated route marker is missing originalRouteId",
                )
            })?
            .to_string();
        if !groups.contains_key(&original_route_id) {
            group_order.push(original_route_id.clone());
        }
        groups
            .entry(original_route_id.clone())
            .or_default()
            .push(route);
        untouched.push((Some(original_route_id), empty_route_placeholder()));
    }

    let mut folded_by_id = HashMap::new();
    for original_route_id in group_order {
        let routes = groups.remove(&original_route_id).unwrap_or_default();
        folded_by_id.insert(
            original_route_id.clone(),
            fold_generated_route_group(source, &original_route_id, routes, &markers)?,
        );
    }
    let mut emitted = HashSet::new();
    let mut folded_routes = Vec::new();
    for (group, route) in untouched {
        match group {
            None => folded_routes.push(route),
            Some(original_route_id) if emitted.insert(original_route_id.clone()) => {
                folded_routes.push(
                    folded_by_id
                        .remove(&original_route_id)
                        .expect("folded group exists"),
                );
            }
            Some(_) => {}
        }
    }
    if let Some(default_route_id) = plan.default_route_id.clone() {
        if let Some(marker) = markers.get(&default_route_id) {
            if marker.get("sourceProviderId").and_then(Value::as_str) == Some(source.id.as_str()) {
                plan.default_route_id = marker
                    .get("originalRouteId")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
        }
    }
    for route_id in source_marker_ids {
        markers.remove(&route_id);
    }
    set_generated_route_markers(plan, markers);
    plan.routes = folded_routes;
    Ok(true)
}

fn provider_set_transport_by_model(
    responses_provider: &Provider,
    chat_provider: &Provider,
) -> Result<HashMap<String, TransportKind>, CodexProviderSetError> {
    let mut lookup = HashMap::new();
    for (provider, transport) in [
        (responses_provider, TransportKind::OpenAiResponses),
        (chat_provider, TransportKind::OpenAiChat),
    ] {
        for model in source_catalog(provider)? {
            for identity in [model.public_model, model.upstream_model] {
                let key = identity.to_ascii_lowercase();
                if lookup
                    .insert(key.clone(), transport)
                    .is_some_and(|previous| previous != transport)
                {
                    return Err(error(
                        "codex_provider_set_router_expansion_ambiguous",
                        format!("Model identity `{identity}` belongs to both protocol leaves"),
                    ));
                }
            }
        }
    }
    Ok(lookup)
}

fn partition_model_selection(
    selection: &CodexModelSelection,
    transport_by_model: &HashMap<String, TransportKind>,
) -> Result<(Option<CodexModelSelection>, Option<CodexModelSelection>), CodexProviderSetError> {
    match selection {
        CodexModelSelection::All => Ok((
            Some(CodexModelSelection::All),
            Some(CodexModelSelection::All),
        )),
        CodexModelSelection::Include { models } => {
            let mut responses = Vec::new();
            let mut chat = Vec::new();
            for model in models {
                match transport_for_model(model, transport_by_model)? {
                    TransportKind::OpenAiResponses => responses.push(model.clone()),
                    TransportKind::OpenAiChat => chat.push(model.clone()),
                }
            }
            Ok((
                (!responses.is_empty())
                    .then_some(CodexModelSelection::Include { models: responses }),
                (!chat.is_empty()).then_some(CodexModelSelection::Include { models: chat }),
            ))
        }
    }
}

fn partition_aliases(
    aliases: &BTreeMap<String, String>,
    expected: TransportKind,
    transport_by_model: &HashMap<String, TransportKind>,
) -> Result<BTreeMap<String, String>, CodexProviderSetError> {
    let mut partition = BTreeMap::new();
    for (alias, target) in aliases {
        if transport_for_model(target, transport_by_model)? == expected {
            partition.insert(alias.clone(), target.clone());
        }
    }
    Ok(partition)
}

fn transport_for_model(
    model: &str,
    transport_by_model: &HashMap<String, TransportKind>,
) -> Result<TransportKind, CodexProviderSetError> {
    transport_by_model
        .get(&model.trim().to_ascii_lowercase())
        .copied()
        .ok_or_else(|| {
            error(
                "codex_provider_set_router_expansion_ambiguous",
                format!("Dependent route model `{model}` is not present in the logical source"),
            )
        })
}

fn generated_route(
    original: &CodexRoutingRouteV2,
    target: &Provider,
    selection: CodexModelSelection,
    aliases: BTreeMap<String, String>,
    transport: TransportKind,
) -> CodexRoutingRouteV2 {
    let mut route = original.clone();
    route.id = format!(
        "{}{}",
        original.id,
        match transport {
            TransportKind::OpenAiResponses => DEPENDENT_RESPONSES_ROUTE_SUFFIX,
            TransportKind::OpenAiChat => DEPENDENT_CHAT_ROUTE_SUFFIX,
        }
    );
    route.label = original.label.as_ref().map(|label| {
        format!(
            "{} · {}",
            label,
            match transport {
                TransportKind::OpenAiResponses => "Responses",
                TransportKind::OpenAiChat => "Chat",
            }
        )
    });
    route.target_provider_id = target.id.clone();
    route.model_selection = selection;
    route.aliases = aliases;
    route
}

fn generated_route_marker(
    source: &Provider,
    original: &CodexRoutingRouteV2,
    transport: TransportKind,
) -> Value {
    json!({
        "version": CODEX_PROTOCOL_SET_VERSION,
        "sourceProviderId": source.id,
        "originalRouteId": original.id,
        "originalLabel": original.label,
        "transport": transport_value(transport)
    })
}

fn generated_route_markers(plan: &CodexRoutingConfigV2) -> BTreeMap<String, Value> {
    plan.extensions
        .get(GENERATED_ROUTES_EXTENSION)
        .and_then(Value::as_object)
        .map(|markers| {
            markers
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn set_generated_route_markers(plan: &mut CodexRoutingConfigV2, markers: BTreeMap<String, Value>) {
    if markers.is_empty() {
        plan.extensions.remove(GENERATED_ROUTES_EXTENSION);
    } else {
        plan.extensions.insert(
            GENERATED_ROUTES_EXTENSION.to_string(),
            Value::Object(markers.into_iter().collect()),
        );
    }
}

fn fold_generated_route_group(
    source: &Provider,
    original_route_id: &str,
    mut routes: Vec<CodexRoutingRouteV2>,
    markers: &BTreeMap<String, Value>,
) -> Result<CodexRoutingRouteV2, CodexProviderSetError> {
    let first = routes.first().cloned().ok_or_else(|| {
        error(
            "codex_provider_set_dependency_changed",
            format!("Generated route group `{original_route_id}` is empty"),
        )
    })?;
    if routes.iter().any(|route| {
        route.enabled != first.enabled
            || route.match_prefixes != first.match_prefixes
            || route.auth_policy != first.auth_policy
    }) {
        return Err(error(
            "codex_provider_set_dependency_changed",
            format!(
                "Generated route group `{original_route_id}` was edited inconsistently and cannot be folded safely"
            ),
        ));
    }
    let mut aliases = BTreeMap::new();
    let mut include_models = Vec::new();
    let mut all = false;
    for route in &routes {
        aliases.extend(route.aliases.clone());
        match &route.model_selection {
            CodexModelSelection::All => all = true,
            CodexModelSelection::Include { models } => include_models.extend(models.clone()),
        }
    }
    include_models.sort();
    include_models.dedup();
    let original_label = routes
        .iter()
        .find_map(|route| markers.get(&route.id))
        .and_then(|marker| marker.get("originalLabel"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut folded = routes.remove(0);
    folded.id = original_route_id.to_string();
    folded.label = original_label;
    folded.target_provider_id = source.id.clone();
    folded.model_selection = if all {
        CodexModelSelection::All
    } else {
        CodexModelSelection::Include {
            models: include_models,
        }
    };
    folded.aliases = aliases;
    Ok(folded)
}

fn empty_route_placeholder() -> CodexRoutingRouteV2 {
    CodexRoutingRouteV2 {
        id: String::new(),
        label: None,
        enabled: false,
        target_provider_id: String::new(),
        model_selection: CodexModelSelection::All,
        match_prefixes: Vec::new(),
        aliases: BTreeMap::new(),
        auth_policy: Default::default(),
    }
}

fn ensure_unique_route_ids(plan: &CodexRoutingConfigV2) -> Result<(), CodexProviderSetError> {
    let mut route_ids = HashSet::new();
    for route in &plan.routes {
        if !route_ids.insert(route.id.to_ascii_lowercase()) {
            return Err(error(
                "codex_provider_set_router_route_conflict",
                format!(
                    "Generated dependent route ID `{}` is already in use",
                    route.id
                ),
            ));
        }
    }
    Ok(())
}

fn source_catalog(source: &Provider) -> Result<Vec<CatalogModel>, CodexProviderSetError> {
    let models = source
        .settings_config
        .get("modelCatalog")
        .or_else(|| source.settings_config.get("model_catalog"))
        .and_then(|catalog| catalog.get("models"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            error(
                "codex_provider_set_model_catalog_required",
                "Codex Provider must synchronize a model catalog before protocol planning",
            )
        })?;
    let mut parsed = Vec::with_capacity(models.len());
    for value in models {
        let Some(public_model) = string_field(value, &["model", "id", "slug"]) else {
            continue;
        };
        let upstream_model =
            string_field(value, &["upstreamModel", "upstream_model"]).unwrap_or(public_model);
        parsed.push(CatalogModel {
            public_model: public_model.to_string(),
            upstream_model: upstream_model.to_string(),
            enabled: value.get("enabled").and_then(Value::as_bool) != Some(false),
            value: value.clone(),
        });
    }
    Ok(parsed)
}

fn blocked_model(
    model: &CatalogModel,
    reason: &str,
    record: Option<&ProtocolCompatibilityRecord>,
) -> CodexProviderSetBlockedModel {
    let failure = record.and_then(|record| {
        record
            .result
            .branches
            .iter()
            .find(|branch| branch.assessment.transport == record.target.transport)
            .and_then(|branch| branch.failures.first())
            .or_else(|| {
                record
                    .result
                    .branches
                    .iter()
                    .find_map(|branch| branch.failures.first())
            })
    });
    CodexProviderSetBlockedModel {
        model: model.public_model.clone(),
        upstream_model: model.upstream_model.clone(),
        reason: reason.to_string(),
        stage: failure.map(|failure| failure.stage),
        failure_kind: failure.map(|failure| failure.kind),
        status_code: failure.and_then(|failure| failure.status_code),
    }
}

fn validate_leaf_ownership(
    source: &Provider,
    existing_providers: &HashMap<String, Provider>,
) -> Result<(), CodexProviderSetError> {
    for (leaf_id, transport) in [
        (
            responses_leaf_id(&source.id),
            TransportKind::OpenAiResponses,
        ),
        (chat_leaf_id(&source.id), TransportKind::OpenAiChat),
    ] {
        if let Some(existing) = existing_providers.get(&leaf_id) {
            validate_owned_leaf(source, Some(existing), &leaf_id, transport)?;
        }
    }
    Ok(())
}

fn owned_existing_leaf_ids(
    source: &Provider,
    existing_providers: &HashMap<String, Provider>,
) -> Vec<String> {
    [
        (
            responses_leaf_id(&source.id),
            TransportKind::OpenAiResponses,
        ),
        (chat_leaf_id(&source.id), TransportKind::OpenAiChat),
    ]
    .into_iter()
    .filter_map(|(leaf_id, transport)| {
        existing_providers
            .get(&leaf_id)
            .filter(|leaf| leaf_is_owned(source, leaf, transport))
            .map(|_| leaf_id)
    })
    .collect()
}

fn validate_owned_leaf(
    source: &Provider,
    leaf: Option<&Provider>,
    leaf_id: &str,
    transport: TransportKind,
) -> Result<(), CodexProviderSetError> {
    let Some(leaf) = leaf else {
        return Err(error(
            "codex_provider_set_leaf_missing",
            format!("Generated Provider `{leaf_id}` is missing"),
        ));
    };
    if leaf_is_owned(source, leaf, transport) {
        Ok(())
    } else {
        Err(error(
            "codex_provider_set_leaf_id_conflict",
            format!("Provider ID `{leaf_id}` is already owned by another Provider"),
        ))
    }
}

fn leaf_is_owned(source: &Provider, leaf: &Provider, transport: TransportKind) -> bool {
    let marker = leaf
        .settings_config
        .get("codexProtocolSet")
        .and_then(Value::as_object);
    marker.is_some_and(|marker| {
        marker.get("version").and_then(Value::as_u64) == Some(CODEX_PROTOCOL_SET_VERSION as u64)
            && marker.get("role").and_then(Value::as_str) == Some("leaf")
            && marker.get("parentProviderId").and_then(Value::as_str) == Some(source.id.as_str())
            && marker.get("transport").and_then(Value::as_str) == Some(transport_value(transport))
    })
}

fn build_leaf(
    source: &Provider,
    transport: TransportKind,
    models: Vec<Value>,
) -> Result<Provider, CodexProviderSetError> {
    let mut leaf = source.clone();
    leaf.id = match transport {
        TransportKind::OpenAiResponses => responses_leaf_id(&source.id),
        TransportKind::OpenAiChat => chat_leaf_id(&source.id),
    };
    leaf.name = format!(
        "{} · {}",
        source.name,
        match transport {
            TransportKind::OpenAiResponses => "Responses",
            TransportKind::OpenAiChat => "Chat",
        }
    );
    remove_provider_set_fields(&mut leaf);
    let catalog = source
        .settings_config
        .get("modelCatalog")
        .or_else(|| source.settings_config.get("model_catalog"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let mut filtered_catalog = catalog;
    filtered_catalog["models"] = Value::Array(models);
    let settings = settings_object_mut(&mut leaf)?;
    settings.insert("modelCatalog".to_string(), filtered_catalog);
    settings.insert(
        "codexProtocolSet".to_string(),
        json!({
            "version": CODEX_PROTOCOL_SET_VERSION,
            "role": "leaf",
            "parentProviderId": source.id,
            "transport": transport_value(transport)
        }),
    );
    normalize_provider_transport(&mut leaf, transport)?;
    Ok(leaf)
}

fn build_facade(
    source: &Provider,
    responses_provider: &Provider,
    chat_provider: &Provider,
    responses_models: &[String],
    chat_models: &[String],
    default_transport: TransportKind,
) -> Result<Provider, CodexProviderSetError> {
    let mut facade = source.clone();
    remove_provider_set_fields(&mut facade);
    normalize_provider_transport(&mut facade, TransportKind::OpenAiResponses)?;
    let mut source_catalog = source
        .settings_config
        .get("modelCatalog")
        .or_else(|| source.settings_config.get("model_catalog"))
        .cloned()
        .ok_or_else(|| {
            error(
                "codex_provider_set_model_catalog_required",
                "Source Provider model catalog is missing",
            )
        })?;
    remove_model_protocols_from_catalog(&mut source_catalog);
    let settings = settings_object_mut(&mut facade)?;
    settings.remove("modelCatalog");
    settings.remove("model_catalog");
    settings.insert(
        "codexProtocolSet".to_string(),
        json!({
            "version": CODEX_PROTOCOL_SET_VERSION,
            "role": "facade",
            "responsesProviderId": responses_provider.id,
            "chatProviderId": chat_provider.id,
            "sourceModelCatalog": source_catalog
        }),
    );
    settings.insert(
        "codexRouting".to_string(),
        json!({
            "schemaVersion": 2,
            "enabled": true,
            "defaultRouteId": match default_transport {
                TransportKind::OpenAiResponses => format!("{}{}", source.id, RESPONSES_ROUTE_SUFFIX),
                TransportKind::OpenAiChat => format!("{}{}", source.id, CHAT_ROUTE_SUFFIX),
            },
            "routes": [
                {
                    "id": format!("{}{}", source.id, RESPONSES_ROUTE_SUFFIX),
                    "label": "Responses",
                    "enabled": true,
                    "targetProviderId": responses_provider.id,
                    "modelSelection": {"mode": "include", "models": responses_models},
                    "authPolicy": {"source": "provider_config"}
                },
                {
                    "id": format!("{}{}", source.id, CHAT_ROUTE_SUFFIX),
                    "label": "Chat",
                    "enabled": true,
                    "targetProviderId": chat_provider.id,
                    "modelSelection": {"mode": "include", "models": chat_models},
                    "authPolicy": {"source": "provider_config"}
                }
            ]
        }),
    );
    Ok(facade)
}

fn source_default_transport(
    source: &Provider,
    selections: &[(CatalogModel, TransportKind)],
) -> Result<TransportKind, CodexProviderSetError> {
    let default_model = codex_provider_upstream_model(source).ok_or_else(|| {
        error(
            "codex_provider_set_router_expansion_ambiguous",
            "Logical source default model is missing",
        )
    })?;
    let default_model = default_model.trim().to_ascii_lowercase();
    let mut matched = None;
    for (model, transport) in selections {
        if model.public_model.to_ascii_lowercase() != default_model
            && model.upstream_model.to_ascii_lowercase() != default_model
        {
            continue;
        }
        if matched.is_some_and(|current| current != *transport) {
            return Err(error(
                "codex_provider_set_router_expansion_ambiguous",
                format!(
                    "Logical source default model `{}` maps to multiple selected protocols",
                    default_model
                ),
            ));
        }
        matched = Some(*transport);
    }
    matched.ok_or_else(|| {
        error(
            "codex_provider_set_router_expansion_ambiguous",
            format!(
                "Logical source default model `{}` has no selected protocol",
                default_model
            ),
        )
    })
}

fn normalize_provider_transport(
    provider: &mut Provider,
    transport: TransportKind,
) -> Result<(), CodexProviderSetError> {
    let (api_format, wire_api) = match transport {
        TransportKind::OpenAiResponses => ("openai_responses", "responses"),
        TransportKind::OpenAiChat => ("openai_chat", "chat"),
    };
    provider
        .meta
        .get_or_insert_with(Default::default)
        .api_format = Some(api_format.to_string());
    let settings = settings_object_mut(provider)?;
    settings.remove("api_format");
    settings.remove("wire_api");
    settings.insert(
        "apiFormat".to_string(),
        Value::String(api_format.to_string()),
    );
    if let Some(config) = settings.get("config").and_then(Value::as_str) {
        let updated = crate::codex_config::update_codex_toml_field(config, "wire_api", wire_api)
            .map_err(|message| error("codex_provider_set_invalid_toml", message))?;
        settings.insert("config".to_string(), Value::String(updated));
    }
    let catalog_key = if settings.contains_key("modelCatalog") {
        "modelCatalog"
    } else {
        "model_catalog"
    };
    if let Some(catalog) = settings.get_mut(catalog_key) {
        remove_model_protocols_from_catalog(catalog);
    }
    Ok(())
}

fn remove_model_protocols_from_catalog(catalog: &mut Value) {
    let Some(models) = catalog.get_mut("models").and_then(Value::as_array_mut) else {
        return;
    };
    for model in models {
        if let Some(model) = model.as_object_mut() {
            model.remove("apiFormat");
            model.remove("api_format");
            model.remove("wireApi");
            model.remove("wire_api");
        }
    }
}

fn remove_provider_set_fields(provider: &mut Provider) {
    if let Some(settings) = provider.settings_config.as_object_mut() {
        settings.remove("codexRouting");
        settings.remove("codexProtocolSet");
    }
}

fn settings_object_mut(
    provider: &mut Provider,
) -> Result<&mut Map<String, Value>, CodexProviderSetError> {
    provider.settings_config.as_object_mut().ok_or_else(|| {
        error(
            "codex_provider_set_invalid_settings",
            "Provider settingsConfig must be an object",
        )
    })
}

fn preview_digest<T: Serialize>(
    source: &Provider,
    records: &[ProtocolCompatibilityRecord],
    value: &T,
) -> Result<String, CodexProviderSetError> {
    let encoded = serde_json::to_vec(&(CODEX_PROTOCOL_SET_VERSION, source, records, value))
        .map_err(|serialize_error| {
            error(
                "codex_provider_set_digest_failed",
                format!("Provider Set plan cannot be serialized: {serialize_error}"),
            )
        })?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn transport_value(transport: TransportKind) -> &'static str {
    match transport {
        TransportKind::OpenAiResponses => "open_ai_responses",
        TransportKind::OpenAiChat => "open_ai_chat",
    }
}

fn string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn error(code: impl Into<String>, message: impl Into<String>) -> CodexProviderSetError {
    CodexProviderSetError {
        code: code.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        chat_leaf_id, plan_codex_provider_set, plan_manual_codex_provider_set, responses_leaf_id,
        restore_logical_codex_provider, rewrite_dependent_routers_for_provider_set,
        CodexProviderSetPersistence, CodexProviderSetPlan, CODEX_PROTOCOL_SET_VERSION,
    };
    use crate::protocol_compatibility::{
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    };
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::{json, Value};
    use std::collections::HashMap;

    fn source_provider() -> Provider {
        let mut provider = Provider::with_id(
            "relay".to_string(),
            "Relay".to_string(),
            json!({
                "auth": {"OPENAI_API_KEY": "secret"},
                "config": "model = \"model-a\"\nmodel_provider = \"relay\"\n[model_providers.relay]\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n",
                "apiFormat": "openai_responses",
                "modelCatalog": {
                    "providerContextWindow": 200000,
                    "models": [
                        {
                            "model": "model-a",
                            "upstreamModel": "upstream-a",
                            "apiFormat": "openai_chat",
                            "contextWindow": 100000
                        },
                        {
                            "model": "model-b",
                            "upstreamModel": "upstream-b",
                            "api_format": "openai_responses"
                        }
                    ]
                }
            }),
            Some("https://relay.example".to_string()),
        );
        provider.category = Some("custom".to_string());
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..ProviderMeta::default()
        });
        provider
    }

    fn record(
        model: &str,
        upstream_model: &str,
        selected: Option<TransportKind>,
        readiness: ProbeReadiness,
    ) -> ProtocolCompatibilityRecord {
        let transport = selected.unwrap_or(TransportKind::OpenAiResponses);
        let target = ProbeTargetKey::new(
            "relay",
            None::<String>,
            model,
            upstream_model,
            transport,
            "https://relay.example/v1/responses",
            "bearer",
        )
        .expect("probe target");
        ProtocolCompatibilityRecord::new(
            target,
            ProtocolCompatibilityProbeResult {
                selected_transport: selected,
                readiness,
                branches: Vec::new(),
            },
            100,
            200,
        )
    }

    fn records(a: TransportKind, b: TransportKind) -> Vec<ProtocolCompatibilityRecord> {
        vec![
            record("model-a", "upstream-a", Some(a), ProbeReadiness::Verified),
            record("model-b", "upstream-b", Some(b), ProbeReadiness::Verified),
        ]
    }

    fn model_names(provider: &Provider) -> Vec<String> {
        provider.settings_config["modelCatalog"]["models"]
            .as_array()
            .expect("model catalog")
            .iter()
            .map(|model| model["model"].as_str().expect("public model").to_string())
            .collect()
    }

    fn assert_no_model_protocols(provider: &Provider) {
        for model in provider.settings_config["modelCatalog"]["models"]
            .as_array()
            .expect("model catalog")
        {
            assert!(
                model.get("apiFormat").is_none(),
                "apiFormat must be removed"
            );
            assert!(
                model.get("api_format").is_none(),
                "api_format must be removed"
            );
            assert!(model.get("wire_api").is_none(), "wire_api must be removed");
        }
    }

    fn dependent_router(selection: Value) -> Provider {
        Provider::with_id(
            "outer-router".to_string(),
            "Outer Router".to_string(),
            json!({
                "codexRouting": {
                    "schemaVersion": 2,
                    "enabled": true,
                    "defaultRouteId": "relay-route",
                    "routes": [{
                        "id": "relay-route",
                        "label": "Relay route",
                        "enabled": true,
                        "targetProviderId": "relay",
                        "modelSelection": selection,
                        "matchPrefixes": ["model-"],
                        "aliases": {
                            "fast-a": "model-a",
                            "fast-b": "model-b"
                        },
                        "authPolicy": {"source": "provider_config"}
                    }]
                }
            }),
            None,
        )
    }

    fn split_prepared(source: &Provider) -> super::PreparedCodexProviderSetMutation {
        plan_codex_provider_set(
            source,
            &records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat),
            &HashMap::new(),
            150,
        )
        .expect("split plan")
    }

    #[test]
    fn all_responses_is_one_homogeneous_provider() {
        let prepared = plan_codex_provider_set(
            &source_provider(),
            &records(
                TransportKind::OpenAiResponses,
                TransportKind::OpenAiResponses,
            ),
            &HashMap::new(),
            150,
        )
        .expect("uniform plan");

        let CodexProviderSetPersistence::Single {
            transport,
            provider,
        } = &prepared.persistence
        else {
            panic!("expected Single");
        };
        assert_eq!(*transport, TransportKind::OpenAiResponses);
        assert_eq!(provider.id, "relay");
        assert_eq!(
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_responses")
        );
        assert_eq!(provider.settings_config["apiFormat"], "openai_responses");
        assert!(provider.settings_config["config"]
            .as_str()
            .expect("config")
            .contains("wire_api = \"responses\""));
        assert_no_model_protocols(&provider);
        assert_eq!(prepared.profiles.len(), 2);
        assert!(prepared
            .profiles
            .iter()
            .all(|record| record.target.provider_id == "relay"));
    }

    #[test]
    fn all_chat_is_one_homogeneous_provider() {
        let prepared = plan_codex_provider_set(
            &source_provider(),
            &records(TransportKind::OpenAiChat, TransportKind::OpenAiChat),
            &HashMap::new(),
            150,
        )
        .expect("uniform plan");

        let CodexProviderSetPersistence::Single {
            transport,
            provider,
        } = &prepared.persistence
        else {
            panic!("expected Single");
        };
        assert_eq!(*transport, TransportKind::OpenAiChat);
        assert_eq!(provider.settings_config["apiFormat"], "openai_chat");
        assert!(provider.settings_config["config"]
            .as_str()
            .expect("config")
            .contains("wire_api = \"chat\""));
        assert_no_model_protocols(&provider);
    }

    #[test]
    fn mixed_verified_models_create_facade_and_two_homogeneous_leaves() {
        let source = source_provider();
        let prepared = plan_codex_provider_set(
            &source,
            &records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat),
            &HashMap::new(),
            150,
        )
        .expect("split plan");

        let CodexProviderSetPersistence::Split {
            facade,
            responses_provider,
            chat_provider,
        } = &prepared.persistence
        else {
            panic!("expected Split");
        };
        assert_eq!(facade.id, source.id);
        assert_eq!(responses_provider.id, responses_leaf_id(&source.id));
        assert_eq!(chat_provider.id, chat_leaf_id(&source.id));
        assert_eq!(model_names(&responses_provider), vec!["model-a"]);
        assert_eq!(model_names(&chat_provider), vec!["model-b"]);
        assert_eq!(
            responses_provider.settings_config["apiFormat"],
            "openai_responses"
        );
        assert_eq!(chat_provider.settings_config["apiFormat"], "openai_chat");
        assert_no_model_protocols(&responses_provider);
        assert_no_model_protocols(&chat_provider);
        assert_eq!(
            facade.settings_config["codexProtocolSet"]["version"],
            CODEX_PROTOCOL_SET_VERSION
        );
        assert_eq!(facade.settings_config["codexProtocolSet"]["role"], "facade");
        assert_eq!(
            facade.settings_config["codexRouting"]["routes"][0]["targetProviderId"],
            responses_provider.id
        );
        assert_eq!(
            facade.settings_config["codexRouting"]["routes"][1]["targetProviderId"],
            chat_provider.id
        );
        assert_eq!(prepared.profiles.len(), 2);
        assert_eq!(
            prepared.profiles[0].target.provider_id,
            responses_provider.id
        );
        assert_eq!(prepared.profiles[1].target.provider_id, chat_provider.id);
        assert!(prepared
            .profiles
            .iter()
            .all(|record| !record.target.request_policy_fingerprint.is_empty()));
    }

    #[test]
    fn split_facade_default_route_follows_the_source_default_model_transport() {
        let mut source = source_provider();
        source.settings_config["config"] = Value::String(
            "model = \"model-b\"\nmodel_provider = \"relay\"\n[model_providers.relay]\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n"
                .to_string(),
        );

        let prepared = plan_codex_provider_set(
            &source,
            &records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat),
            &HashMap::new(),
            150,
        )
        .expect("split plan");
        let CodexProviderSetPersistence::Split { facade, .. } = prepared.persistence else {
            panic!("expected Split");
        };

        assert_eq!(
            facade.settings_config["codexRouting"]["defaultRouteId"],
            "relay--ccsm-chat-route"
        );
    }

    #[test]
    fn selector_choice_places_each_model_in_exactly_one_group() {
        let source = source_provider();
        let mut selected_responses = record(
            "model-a",
            "upstream-a",
            Some(TransportKind::OpenAiResponses),
            ProbeReadiness::Verified,
        );
        selected_responses.result.branches = serde_json::from_value(json!([
            {
                "assessment": {
                    "transport": "open_ai_chat",
                    "baseline": "passed",
                    "streaming": "passed",
                    "forced_tool": "passed",
                    "continuation": "passed"
                },
                "reasoning_shape": {
                    "semantic": "readable",
                    "source": "reasoning_content",
                    "pre_tool_visible_content": "absent"
                },
                "tool_schema_dialect": "open_ai",
                "history_replay": "chat_reasoning_content",
                "evidence": [],
                "failures": []
            },
            {
                "assessment": {
                    "transport": "open_ai_responses",
                    "baseline": "passed",
                    "streaming": "passed",
                    "forced_tool": "passed",
                    "continuation": "passed"
                },
                "reasoning_shape": {
                    "semantic": "summary",
                    "source": "native_responses",
                    "pre_tool_visible_content": "absent"
                },
                "tool_schema_dialect": "open_ai",
                "history_replay": "native_only",
                "evidence": [],
                "failures": []
            }
        ]))
        .expect("two verified branches");
        let records = vec![
            selected_responses,
            record(
                "model-b",
                "upstream-b",
                Some(TransportKind::OpenAiChat),
                ProbeReadiness::Verified,
            ),
        ];

        let prepared =
            plan_codex_provider_set(&source, &records, &HashMap::new(), 150).expect("split plan");
        let CodexProviderSetPersistence::Split {
            responses_provider,
            chat_provider,
            ..
        } = prepared.persistence
        else {
            panic!("expected Split");
        };
        assert_eq!(model_names(&responses_provider), vec!["model-a"]);
        assert_eq!(model_names(&chat_provider), vec!["model-b"]);
    }

    #[test]
    fn partial_or_missing_enabled_model_blocks_without_grouping() {
        let source = source_provider();
        let mut partial = vec![
            record(
                "model-a",
                "upstream-a",
                Some(TransportKind::OpenAiResponses),
                ProbeReadiness::Verified,
            ),
            record(
                "model-b",
                "upstream-b",
                Some(TransportKind::OpenAiChat),
                ProbeReadiness::Partial,
            ),
        ];
        partial[1].result = serde_json::from_value(json!({
            "selected_transport": "open_ai_chat",
            "readiness": "partial",
            "branches": [{
                "assessment": {
                    "transport": "open_ai_chat",
                    "baseline": "passed",
                    "streaming": "passed",
                    "forced_tool": "passed",
                    "continuation": "failed"
                },
                "reasoning_shape": {
                    "semantic": "readable",
                    "source": "reasoning_content",
                    "pre_tool_visible_content": "present"
                },
                "tool_schema_dialect": "open_ai",
                "history_replay": "chat_reasoning_content",
                "evidence": [],
                "failures": [{
                    "stage": "continuation",
                    "kind": "http_status",
                    "status_code": 422
                }]
            }]
        }))
        .expect("partial branch with redacted failure");
        let prepared = plan_codex_provider_set(&source, &partial, &HashMap::new(), 150)
            .expect("blocked preview");
        let CodexProviderSetPlan::Blocked { models } = prepared.preview.plan else {
            panic!("expected Blocked");
        };
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model, "model-b");
        assert_eq!(models[0].reason, "probe_not_verified");
        assert_eq!(
            serde_json::to_value(&models[0]).expect("serialize blocked model"),
            json!({
                "model": "model-b",
                "upstreamModel": "upstream-b",
                "reason": "probe_not_verified",
                "stage": "continuation",
                "failureKind": "http_status",
                "statusCode": 422
            }),
            "Blocked preview must preserve the actionable redacted failure classification"
        );

        let missing = plan_codex_provider_set(&source, &partial[..1], &HashMap::new(), 150)
            .expect("missing preview");
        let CodexProviderSetPlan::Blocked { models } = missing.preview.plan else {
            panic!("expected Blocked");
        };
        assert_eq!(models[0].model, "model-b");
        assert_eq!(models[0].reason, "probe_required");
    }

    #[test]
    fn disabled_models_do_not_require_a_probe_or_enter_a_leaf() {
        let mut source = source_provider();
        source.settings_config["modelCatalog"]["models"][1]["enabled"] = Value::Bool(false);
        let prepared = plan_codex_provider_set(
            &source,
            &[record(
                "model-a",
                "upstream-a",
                Some(TransportKind::OpenAiResponses),
                ProbeReadiness::Verified,
            )],
            &HashMap::new(),
            150,
        )
        .expect("uniform plan");
        let CodexProviderSetPersistence::Single { provider, .. } = prepared.persistence else {
            panic!("expected Single");
        };
        assert_eq!(model_names(&provider), vec!["model-a", "model-b"]);
    }

    #[test]
    fn unowned_leaf_id_collision_fails_closed() {
        let source = source_provider();
        let occupied = Provider::with_id(
            responses_leaf_id(&source.id),
            "User Provider".to_string(),
            json!({}),
            None,
        );
        let existing = [(occupied.id.clone(), occupied)].into_iter().collect();
        let error = plan_codex_provider_set(
            &source,
            &records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat),
            &existing,
            150,
        )
        .expect_err("must reject unowned collision");
        assert_eq!(error.code, "codex_provider_set_leaf_id_conflict");
    }

    #[test]
    fn stale_selectionless_and_duplicate_records_are_blocked() {
        let source = source_provider();
        let mut stale = record(
            "model-a",
            "upstream-a",
            Some(TransportKind::OpenAiResponses),
            ProbeReadiness::Verified,
        );
        stale.expires_at = 149;
        let no_selection = record("model-b", "upstream-b", None, ProbeReadiness::Verified);
        let prepared =
            plan_codex_provider_set(&source, &[stale, no_selection], &HashMap::new(), 150)
                .expect("blocked preview");
        let CodexProviderSetPlan::Blocked { models } = prepared.preview.plan else {
            panic!("expected Blocked");
        };
        assert_eq!(models[0].reason, "probe_stale");
        assert_eq!(models[1].reason, "probe_has_no_selection");

        let duplicate = record(
            "model-a",
            "upstream-a",
            Some(TransportKind::OpenAiResponses),
            ProbeReadiness::Verified,
        );
        let prepared = plan_codex_provider_set(
            &source,
            &[
                duplicate.clone(),
                duplicate,
                record(
                    "model-b",
                    "upstream-b",
                    Some(TransportKind::OpenAiChat),
                    ProbeReadiness::Verified,
                ),
            ],
            &HashMap::new(),
            150,
        )
        .expect("blocked preview");
        let CodexProviderSetPlan::Blocked { models } = prepared.preview.plan else {
            panic!("expected Blocked");
        };
        assert_eq!(models[0].model, "model-a");
        assert_eq!(models[0].reason, "conflicting_probe_records");
    }

    #[test]
    fn matching_owned_leaves_can_be_replanned_without_an_id_conflict() {
        let source = source_provider();
        let records = records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat);
        let first =
            plan_codex_provider_set(&source, &records, &HashMap::new(), 150).expect("first split");
        let CodexProviderSetPersistence::Split {
            responses_provider,
            chat_provider,
            ..
        } = first.persistence
        else {
            panic!("expected Split");
        };
        let existing = [
            (responses_provider.id.clone(), responses_provider),
            (chat_provider.id.clone(), chat_provider),
        ]
        .into_iter()
        .collect();

        let second = plan_codex_provider_set(&source, &records, &existing, 150)
            .expect("owned leaves are reusable");
        assert!(matches!(
            second.preview.plan,
            CodexProviderSetPlan::Split { .. }
        ));
    }

    #[test]
    fn uniform_replan_marks_only_owned_split_leaves_for_deletion() {
        let source = source_provider();
        let first_records = records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat);
        let first = plan_codex_provider_set(&source, &first_records, &HashMap::new(), 150)
            .expect("first split");
        let CodexProviderSetPersistence::Split {
            responses_provider,
            chat_provider,
            ..
        } = first.persistence
        else {
            panic!("expected Split");
        };
        let unrelated = Provider::with_id(
            "relay--unrelated".to_string(),
            "Unrelated".to_string(),
            json!({}),
            None,
        );
        let existing = [
            (responses_provider.id.clone(), responses_provider),
            (chat_provider.id.clone(), chat_provider),
            (unrelated.id.clone(), unrelated),
        ]
        .into_iter()
        .collect();
        let prepared = plan_codex_provider_set(
            &source,
            &records(
                TransportKind::OpenAiResponses,
                TransportKind::OpenAiResponses,
            ),
            &existing,
            150,
        )
        .expect("uniform replan");

        assert_eq!(
            prepared.delete_provider_ids,
            vec![responses_leaf_id("relay"), chat_leaf_id("relay")]
        );
        assert!(prepared
            .replace_profile_provider_ids
            .contains("relay--ccsm-responses"));
        assert!(prepared
            .replace_profile_provider_ids
            .contains("relay--ccsm-chat"));
        assert!(!prepared
            .replace_profile_provider_ids
            .contains("relay--unrelated"));
    }

    #[test]
    fn serialized_preview_never_contains_provider_credentials() {
        let prepared = plan_codex_provider_set(
            &source_provider(),
            &records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat),
            &HashMap::new(),
            150,
        )
        .expect("split plan");
        let serialized = serde_json::to_string(&prepared.preview).expect("serialize preview");
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("OPENAI_API_KEY"));
        assert!(serialized.contains("relay--ccsm-responses"));
        assert!(serialized.contains("model-a"));
    }

    #[test]
    fn split_facade_restores_the_logical_source_catalog_for_editing() {
        let source = source_provider();
        let prepared = plan_codex_provider_set(
            &source,
            &records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat),
            &HashMap::new(),
            150,
        )
        .expect("split plan");
        let CodexProviderSetPersistence::Split {
            facade,
            responses_provider,
            chat_provider,
        } = prepared.persistence
        else {
            panic!("expected Split");
        };
        let existing = [
            (responses_provider.id.clone(), responses_provider),
            (chat_provider.id.clone(), chat_provider),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();

        let restored =
            restore_logical_codex_provider(&facade, &existing).expect("restore logical source");
        assert_eq!(restored.id, source.id);
        assert_eq!(restored.name, source.name);
        assert_eq!(model_names(&restored), vec!["model-a", "model-b"]);
        assert_no_model_protocols(&restored);
        assert_eq!(
            restored.settings_config["modelCatalog"]["providerContextWindow"],
            source.settings_config["modelCatalog"]["providerContextWindow"]
        );
        assert!(restored.settings_config.get("codexRouting").is_none());
        assert!(restored.settings_config.get("codexProtocolSet").is_none());
    }

    #[test]
    fn dependent_all_route_expands_to_leaves_and_default_follows_source_default_model() {
        let source = source_provider();
        let prepared = split_prepared(&source);
        let existing = [(
            "outer-router".to_string(),
            dependent_router(json!({"mode": "all"})),
        )]
        .into_iter()
        .collect();

        let updates =
            rewrite_dependent_routers_for_provider_set(&source, &prepared.persistence, &existing)
                .expect("rewrite dependent Router");
        assert_eq!(updates.len(), 1);
        let routing = &updates[0].settings_config["codexRouting"];
        let routes = routing["routes"].as_array().expect("routes");
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0]["targetProviderId"], responses_leaf_id("relay"));
        assert_eq!(routes[1]["targetProviderId"], chat_leaf_id("relay"));
        assert_eq!(routes[0]["modelSelection"]["mode"], "all");
        assert_eq!(routes[1]["modelSelection"]["mode"], "all");
        assert_eq!(routing["defaultRouteId"], routes[0]["id"]);
        assert_ne!(routing["defaultRouteId"], routes[1]["id"]);
    }

    #[test]
    fn dependent_include_route_partitions_models_and_aliases() {
        let source = source_provider();
        let prepared = split_prepared(&source);
        let existing = [(
            "outer-router".to_string(),
            dependent_router(json!({"mode": "include", "models": ["model-a", "model-b"]})),
        )]
        .into_iter()
        .collect();

        let updates =
            rewrite_dependent_routers_for_provider_set(&source, &prepared.persistence, &existing)
                .expect("partition dependent route");
        let routes = updates[0].settings_config["codexRouting"]["routes"]
            .as_array()
            .expect("routes");
        assert_eq!(routes[0]["modelSelection"]["models"], json!(["model-a"]));
        assert_eq!(routes[1]["modelSelection"]["models"], json!(["model-b"]));
        assert_eq!(routes[0]["aliases"], json!({"fast-a": "model-a"}));
        assert_eq!(routes[1]["aliases"], json!({"fast-b": "model-b"}));
        assert_eq!(routes[0]["matchPrefixes"], json!(["model-"]));
        assert_eq!(routes[1]["matchPrefixes"], json!(["model-"]));
    }

    #[test]
    fn dependent_generated_routes_fold_back_to_source_on_uniform_reprobe() {
        let source = source_provider();
        let split = split_prepared(&source);
        let initial = [(
            "outer-router".to_string(),
            dependent_router(json!({"mode": "all"})),
        )]
        .into_iter()
        .collect();
        let split_updates =
            rewrite_dependent_routers_for_provider_set(&source, &split.persistence, &initial)
                .expect("split dependent route");
        let existing_leaves = match split.persistence {
            CodexProviderSetPersistence::Split {
                responses_provider,
                chat_provider,
                ..
            } => [
                (responses_provider.id.clone(), responses_provider),
                (chat_provider.id.clone(), chat_provider),
            ]
            .into_iter()
            .collect(),
            _ => panic!("expected Split"),
        };
        let uniform = plan_codex_provider_set(
            &source,
            &records(
                TransportKind::OpenAiResponses,
                TransportKind::OpenAiResponses,
            ),
            &existing_leaves,
            150,
        )
        .expect("uniform plan");
        let routers = [("outer-router".to_string(), split_updates[0].clone())]
            .into_iter()
            .collect();

        let folded =
            rewrite_dependent_routers_for_provider_set(&source, &uniform.persistence, &routers)
                .expect("fold generated routes");
        let routing = &folded[0].settings_config["codexRouting"];
        let routes = routing["routes"].as_array().expect("routes");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0]["id"], "relay-route");
        assert_eq!(routes[0]["targetProviderId"], "relay");
        assert_eq!(routes[0]["label"], "Relay route");
        assert_eq!(routing["defaultRouteId"], "relay-route");
    }

    #[test]
    fn split_plan_blocks_when_source_default_model_has_no_selected_group() {
        let mut source = source_provider();
        source.settings_config["config"] = Value::String(
            "model = \"missing-model\"\nmodel_provider = \"relay\"\n[model_providers.relay]\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n"
                .to_string(),
        );

        let error = plan_codex_provider_set(
            &source,
            &records(TransportKind::OpenAiResponses, TransportKind::OpenAiChat),
            &HashMap::new(),
            150,
        )
        .expect_err("ambiguous default must block before a split facade is materialized");
        assert_eq!(error.code, "codex_provider_set_router_expansion_ambiguous");
    }

    #[test]
    fn manual_whole_provider_protocol_collapses_owned_split_without_fake_profiles() {
        let source = source_provider();
        let split = split_prepared(&source);
        let CodexProviderSetPersistence::Split {
            facade,
            responses_provider,
            chat_provider,
        } = split.persistence
        else {
            panic!("expected Split");
        };
        let existing = [
            (facade.id.clone(), facade.clone()),
            (responses_provider.id.clone(), responses_provider),
            (chat_provider.id.clone(), chat_provider),
        ]
        .into_iter()
        .collect::<HashMap<_, _>>();
        let mut restored =
            restore_logical_codex_provider(&facade, &existing).expect("restore logical source");
        restored
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .codex_protocol_mode = Some(crate::provider::CodexProtocolMode::Manual);

        let prepared = plan_manual_codex_provider_set(
            &restored,
            TransportKind::OpenAiResponses,
            &existing,
            150,
        )
        .expect("plan manual uniform Provider");

        assert!(matches!(
            prepared.persistence,
            CodexProviderSetPersistence::Single {
                transport: TransportKind::OpenAiResponses,
                ..
            }
        ));
        assert_eq!(
            prepared.delete_provider_ids,
            vec![responses_leaf_id("relay"), chat_leaf_id("relay")]
        );
        assert!(prepared.profiles.is_empty());
        assert!(prepared.probe_records.is_empty());
    }

    #[test]
    fn manual_whole_provider_protocol_does_not_require_or_fabricate_a_catalog() {
        let mut source = source_provider();
        source
            .settings_config
            .as_object_mut()
            .expect("settings object")
            .remove("modelCatalog");

        let prepared = plan_manual_codex_provider_set(
            &source,
            TransportKind::OpenAiChat,
            &HashMap::new(),
            150,
        )
        .expect("manual protocol is an explicit whole-Provider choice");

        let CodexProviderSetPersistence::Single {
            transport,
            provider,
        } = prepared.persistence
        else {
            panic!("expected Single");
        };
        assert_eq!(transport, TransportKind::OpenAiChat);
        assert_eq!(provider.settings_config["apiFormat"], "openai_chat");
        assert!(prepared.profiles.is_empty());
        assert!(prepared.probe_records.is_empty());
    }
}
