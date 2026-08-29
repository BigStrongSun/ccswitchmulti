use crate::protocol_compatibility::{
    compile_provider_probe_candidate_for_model, ProbeReadiness, ProtocolCompatibilityRecord,
    TransportKind, PROBE_PROFILE_VERSION,
};
use crate::provider::Provider;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub const CODEX_PROTOCOL_SET_VERSION: u32 = 1;
const RESPONSES_LEAF_SUFFIX: &str = "--ccsm-responses";
const CHAT_LEAF_SUFFIX: &str = "--ccsm-chat";
const RESPONSES_ROUTE_SUFFIX: &str = "--ccsm-responses-route";
const CHAT_ROUTE_SUFFIX: &str = "--ccsm-chat-route";

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
            blocked.push(blocked_model(model, "duplicate_model_identity"));
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
            blocked.push(blocked_model(model, "probe_required"));
            continue;
        };
        if matches.len() != 1 {
            blocked.push(blocked_model(model, "conflicting_probe_records"));
            continue;
        }
        if record.probe_version != PROBE_PROFILE_VERSION || record.expires_at < now {
            blocked.push(blocked_model(model, "probe_stale"));
            continue;
        }
        if record.result.readiness != ProbeReadiness::Verified {
            blocked.push(blocked_model(model, "probe_not_verified"));
            continue;
        }
        let Some(selected) = record.result.selected_transport else {
            blocked.push(blocked_model(model, "probe_has_no_selection"));
            continue;
        };
        if record.target.transport != selected {
            blocked.push(blocked_model(model, "probe_target_mismatch"));
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
        let facade = build_facade(
            source,
            &responses_provider,
            &chat_provider,
            &responses_models,
            &chat_models,
        )?;
        CodexProviderSetPersistence::Split {
            facade,
            responses_provider,
            chat_provider,
        }
    };
    let digest = preview_digest(source, records, &persistence_digest_value(&persistence))?;
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
    })
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

fn blocked_model(model: &CatalogModel, reason: &str) -> CodexProviderSetBlockedModel {
    CodexProviderSetBlockedModel {
        model: model.public_model.clone(),
        upstream_model: model.upstream_model.clone(),
        reason: reason.to_string(),
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
            "defaultRouteId": format!("{}{}", source.id, RESPONSES_ROUTE_SUFFIX),
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
        chat_leaf_id, plan_codex_provider_set, responses_leaf_id, restore_logical_codex_provider,
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
        let partial = vec![
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
        let prepared = plan_codex_provider_set(&source, &partial, &HashMap::new(), 150)
            .expect("blocked preview");
        let CodexProviderSetPlan::Blocked { models } = prepared.preview.plan else {
            panic!("expected Blocked");
        };
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model, "model-b");
        assert_eq!(models[0].reason, "probe_not_verified");

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
}
