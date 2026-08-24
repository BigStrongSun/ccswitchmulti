use serde_json::Value;
use std::collections::HashMap;

use crate::{
    app_config::AppType,
    codex_multirouter::{compiler::compile_v2, schema::CodexRoutingDocument},
    provider::Provider,
    proxy::providers::{
        codex_provider_upstream_model, explain_codex_responses_upstream_protocol,
        resolve_codex_v2_routed_provider,
    },
};

use super::{ProbeCandidate, ProtocolCompatibilityProbeResult, TransportKind};

pub fn apply_probe_selection_to_provider(
    provider: &mut Provider,
    result: &ProtocolCompatibilityProbeResult,
) -> Result<bool, String> {
    let Some(transport) = result.selected_transport else {
        return Ok(false);
    };
    apply_selected_transport_to_provider(provider, transport)?;
    Ok(true)
}

pub fn compile_provider_probe_candidate(provider: &Provider) -> Result<ProbeCandidate, String> {
    if provider.uses_managed_account_auth() {
        return Err("managed or official providers do not use third-party protocol probing".into());
    }

    let configured_model = codex_provider_upstream_model(provider)
        .ok_or_else(|| "Codex provider has no configured model".to_string())?;
    let (public_model, upstream_model) = resolve_primary_model(provider, &configured_model);
    let (base_url, api_key) = provider.resolve_usage_credentials(&AppType::Codex);
    if base_url.trim().is_empty() {
        return Err("Codex provider has no base URL".to_string());
    }
    if api_key.trim().is_empty() {
        return Err("Codex provider has no API key".to_string());
    }

    let transport = match explain_codex_responses_upstream_protocol(provider)
        .protocol
        .api_format()
    {
        "openai_chat" => TransportKind::OpenAiChat,
        "openai_responses" => TransportKind::OpenAiResponses,
        _ => return Err("provider is not an OpenAI Chat/Responses protocol candidate".to_string()),
    };
    let is_full_url = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.is_full_url)
        .unwrap_or(false);

    let provider_id = string_field(&provider.settings_config, &["codexRouterParentProviderId"])
        .unwrap_or(provider.id.as_str());
    let route_id = string_field(&provider.settings_config, &["codexResolvedRouteId"]);

    ProbeCandidate::new(
        Some(provider_id.to_string()),
        route_id.map(str::to_string),
        public_model,
        upstream_model,
        transport,
        &base_url,
        "bearer",
    )
    .map_err(|_| "Codex provider base URL is not a valid absolute URL".to_string())?
    .with_full_url(is_full_url)
    .with_bearer_token(&api_key)
    .map_err(|_| "Codex provider API key cannot be represented as an HTTP header".to_string())
}

pub fn compile_codex_router_probe_candidates(
    router: &Provider,
    providers: &HashMap<String, Provider>,
) -> Result<Vec<ProbeCandidate>, String> {
    let Some(routing) = router.settings_config.get("codexRouting") else {
        return Ok(Vec::new());
    };
    let document = CodexRoutingDocument::parse(routing)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let CodexRoutingDocument::V2(plan) = document else {
        return Ok(Vec::new());
    };
    if !plan.enabled {
        return Ok(Vec::new());
    }
    let compiled = compile_v2(&plan, providers)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let mut candidates = Vec::with_capacity(compiled.model_catalog.len());
    for model in compiled.model_catalog {
        let Some(resolved) = resolve_codex_v2_routed_provider(
            router,
            &serde_json::json!({"model": model.visible_model}),
            providers,
        )
        .map_err(|error| format!("{}: {}", error.code, error.message))?
        else {
            continue;
        };
        let effective = resolved.into_effective_provider();
        if effective.uses_manual_codex_protocol()
            || effective.category.as_deref() == Some("official")
            || effective.uses_managed_account_auth()
        {
            continue;
        }
        candidates.push(compile_provider_probe_candidate(&effective)?);
    }
    Ok(candidates)
}

pub fn apply_selected_transport_to_provider(
    provider: &mut Provider,
    transport: TransportKind,
) -> Result<(), String> {
    let configured_model = codex_provider_upstream_model(provider)
        .ok_or_else(|| "Codex provider has no configured model".to_string())?;
    let (public_model, upstream_model) = resolve_primary_model(provider, &configured_model);
    let (api_format, wire_api) = match transport {
        TransportKind::OpenAiChat => ("openai_chat", "chat"),
        TransportKind::OpenAiResponses => ("openai_responses", "responses"),
    };

    provider
        .meta
        .get_or_insert_with(Default::default)
        .api_format = Some(api_format.to_string());
    provider.settings_config["apiFormat"] = Value::String(api_format.to_string());
    update_primary_catalog_protocol(
        &mut provider.settings_config,
        &public_model,
        &upstream_model,
        api_format,
    );

    if let Some(config) = provider
        .settings_config
        .get("config")
        .and_then(Value::as_str)
    {
        let updated = crate::codex_config::update_codex_toml_field(config, "wire_api", wire_api)?;
        provider.settings_config["config"] = Value::String(updated);
    }
    Ok(())
}

fn resolve_primary_model(provider: &Provider, configured_model: &str) -> (String, String) {
    let models = provider
        .settings_config
        .get("modelCatalog")
        .or_else(|| provider.settings_config.get("model_catalog"))
        .and_then(|catalog| catalog.get("models"))
        .and_then(Value::as_array);
    if let Some(models) = models {
        for model in models {
            let Some(public_model) = string_field(model, &["model", "id", "slug"]) else {
                continue;
            };
            let upstream_model =
                string_field(model, &["upstreamModel", "upstream_model"]).unwrap_or(public_model);
            if public_model == configured_model || upstream_model == configured_model {
                return (public_model.to_string(), upstream_model.to_string());
            }
        }
    }
    (configured_model.to_string(), configured_model.to_string())
}

fn update_primary_catalog_protocol(
    settings: &mut Value,
    public_model: &str,
    upstream_model: &str,
    api_format: &str,
) {
    let catalog_key = if settings.get("modelCatalog").is_some() {
        "modelCatalog"
    } else {
        "model_catalog"
    };
    let Some(models) = settings
        .get_mut(catalog_key)
        .and_then(|catalog| catalog.get_mut("models"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for model in models {
        let visible = string_field(model, &["model", "id", "slug"]);
        let upstream = string_field(model, &["upstreamModel", "upstream_model"]);
        if visible == Some(public_model) || upstream == Some(upstream_model) {
            model["apiFormat"] = Value::String(api_format.to_string());
        }
    }
}

fn string_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
