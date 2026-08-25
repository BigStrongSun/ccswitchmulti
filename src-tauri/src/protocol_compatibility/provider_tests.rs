use serde_json::json;
use std::collections::HashMap;

use crate::provider::{Provider, ProviderMeta};

use super::{
    apply_probe_selection_to_provider, apply_selected_transport_to_provider,
    compile_codex_router_probe_candidates, compile_provider_probe_candidate,
    compile_provider_probe_candidates, ProbeReadiness, ProtocolCompatibilityProbeResult,
    TransportKind,
};

fn codex_provider(config: &str, api_format: &str) -> Provider {
    Provider {
        id: "qwen-provider".to_string(),
        name: "Qwen".to_string(),
        settings_config: json!({
            "auth": {"OPENAI_API_KEY": "probe-secret"},
            "config": config,
            "modelCatalog": {"models": [{
                "model": "qwen-visible",
                "upstreamModel": "Qwen/Qwen3.8",
                "apiFormat": api_format
            }]}
        }),
        website_url: None,
        category: None,
        created_at: None,
        sort_index: None,
        notes: None,
        meta: Some(ProviderMeta {
            api_format: Some(api_format.to_string()),
            ..ProviderMeta::default()
        }),
        icon: None,
        icon_color: None,
        in_failover_queue: false,
    }
}

#[test]
fn compiles_the_unsaved_provider_primary_model_without_trusting_wire_api() {
    let provider = codex_provider(
        r#"model = "qwen-visible"
model_provider = "qwen"
[model_providers.qwen]
base_url = "https://vllm.example/v1"
wire_api = "responses"
"#,
        "openai_responses",
    );

    let candidate = compile_provider_probe_candidate(&provider).expect("compile candidate");

    assert_eq!(candidate.provider_id.as_deref(), Some("qwen-provider"));
    assert_eq!(candidate.public_model, "qwen-visible");
    assert_eq!(candidate.upstream_model, "Qwen/Qwen3.8");
    assert_eq!(candidate.transport, TransportKind::OpenAiResponses);
    assert_eq!(candidate.canonical_endpoint(), "https://vllm.example/v1");
    let debug = format!("{candidate:?}");
    assert!(!debug.contains("probe-secret"));
}

#[test]
fn compiles_every_enabled_catalog_model_for_an_ordinary_provider() {
    let mut provider = codex_provider(
        r#"model = "qwen-visible"
model_provider = "qwen"
[model_providers.qwen]
base_url = "https://vllm.example/v1"
wire_api = "responses"
"#,
        "openai_responses",
    );
    provider.settings_config["modelCatalog"] = json!({"models": [
        {
            "model": "qwen-visible",
            "upstreamModel": "Qwen/Qwen3.8",
            "enabled": true
        },
        {
            "model": "qwen-coder",
            "upstreamModel": "Qwen/Qwen3-Coder"
        },
        {
            "model": "qwen-disabled",
            "upstreamModel": "Qwen/Qwen3-Disabled",
            "enabled": false
        }
    ]});

    let candidates = compile_provider_probe_candidates(&provider).expect("compile candidates");

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].public_model, "qwen-visible");
    assert_eq!(candidates[0].upstream_model, "Qwen/Qwen3.8");
    assert_eq!(candidates[1].public_model, "qwen-coder");
    assert_eq!(candidates[1].upstream_model, "Qwen/Qwen3-Coder");
    assert!(candidates
        .iter()
        .all(|candidate| candidate.provider_id.as_deref() == Some("qwen-provider")));
}

#[test]
fn compiles_an_effective_router_target_with_the_parent_and_route_identity() {
    let mut provider = codex_provider(
        r#"model = "qwen-visible"
model_provider = "qwen"
[model_providers.qwen]
base_url = "https://vllm.example/v1"
wire_api = "chat"
"#,
        "openai_chat",
    );
    provider.id = "router::route::qwen".to_string();
    provider.settings_config["codexRouterParentProviderId"] = json!("router");
    provider.settings_config["codexResolvedRouteId"] = json!("qwen-route");

    let candidate = compile_provider_probe_candidate(&provider).expect("compile routed candidate");

    assert_eq!(candidate.provider_id.as_deref(), Some("router"));
    assert_eq!(candidate.route_id.as_deref(), Some("qwen-route"));
}

#[test]
fn applying_chat_selection_updates_every_runtime_protocol_source() {
    let mut provider = codex_provider(
        r#"model = "qwen-visible"
model_provider = "qwen"
[model_providers.qwen]
base_url = "https://vllm.example/v1"
wire_api = "responses"
"#,
        "openai_responses",
    );

    apply_selected_transport_to_provider(&mut provider, TransportKind::OpenAiChat)
        .expect("apply chat selection");

    assert_eq!(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.api_format.as_deref()),
        Some("openai_chat")
    );
    assert_eq!(provider.settings_config["apiFormat"], "openai_chat");
    assert_eq!(
        provider.settings_config["modelCatalog"]["models"][0]["apiFormat"],
        "openai_chat"
    );
    let config = provider.settings_config["config"].as_str().unwrap();
    assert!(config.contains("wire_api = \"chat\""));
    assert!(!config.contains("wire_api = \"responses\""));
}

#[test]
fn applying_responses_selection_replaces_a_historical_chat_hint() {
    let mut provider = codex_provider(
        r#"model = "qwen-visible"
model_provider = "qwen"
[model_providers.qwen]
base_url = "https://vllm.example/v1"
wire_api = "chat"
"#,
        "openai_chat",
    );

    apply_selected_transport_to_provider(&mut provider, TransportKind::OpenAiResponses)
        .expect("apply responses selection");

    assert_eq!(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.api_format.as_deref()),
        Some("openai_responses")
    );
    assert_eq!(provider.settings_config["apiFormat"], "openai_responses");
    assert_eq!(
        provider.settings_config["modelCatalog"]["models"][0]["apiFormat"],
        "openai_responses"
    );
    assert!(provider.settings_config["config"]
        .as_str()
        .unwrap()
        .contains("wire_api = \"responses\""));
}

#[test]
fn managed_oauth_provider_is_not_an_active_third_party_probe_candidate() {
    let mut provider = codex_provider(
        "model = \"gpt-5.6\"\nbase_url = \"https://chatgpt.com/backend-api/codex\"\n",
        "openai_responses",
    );
    provider.meta.as_mut().unwrap().provider_type = Some("codex_oauth".to_string());

    assert!(compile_provider_probe_candidate(&provider).is_err());
}

#[test]
fn a_partial_but_reachable_selection_updates_transport_without_enabling_reasoning_by_itself() {
    let mut provider = codex_provider(
        "model = \"qwen-visible\"\nbase_url = \"https://vllm.example/v1\"\nwire_api = \"responses\"\n",
        "openai_responses",
    );
    let result = ProtocolCompatibilityProbeResult {
        selected_transport: Some(TransportKind::OpenAiChat),
        readiness: ProbeReadiness::Partial,
        branches: Vec::new(),
    };

    assert!(apply_probe_selection_to_provider(&mut provider, &result).unwrap());
    assert_eq!(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.api_format.as_deref()),
        Some("openai_chat")
    );
}

#[test]
fn an_unreachable_probe_keeps_the_historical_transport_unchanged() {
    let mut provider = codex_provider(
        "model = \"qwen-visible\"\nbase_url = \"https://vllm.example/v1\"\nwire_api = \"responses\"\n",
        "openai_responses",
    );
    let before = serde_json::to_value(&provider).unwrap();
    let result = ProtocolCompatibilityProbeResult {
        selected_transport: None,
        readiness: ProbeReadiness::Unverified,
        branches: Vec::new(),
    };

    assert!(!apply_probe_selection_to_provider(&mut provider, &result).unwrap());
    assert_eq!(serde_json::to_value(&provider).unwrap(), before);
}

#[test]
fn compiles_every_v2_router_model_from_the_real_effective_provider() {
    let mut target = codex_provider(
        r#"model = "qwen-visible"
model_provider = "qwen"
[model_providers.qwen]
base_url = "https://vllm.example/v1"
wire_api = "chat"
"#,
        "openai_chat",
    );
    target.id = "qwen-target".to_string();
    target.settings_config["modelCatalog"] = json!({"models": [
        {"model": "qwen-visible", "upstreamModel": "Qwen/Qwen3.8"},
        {"model": "qwen-coder", "upstreamModel": "Qwen/Qwen3-Coder"}
    ]});
    let router = Provider::with_id(
        "router".to_string(),
        "Router".to_string(),
        json!({
            "auth": {},
            "config": "",
            "codexRouting": {
                "schemaVersion": 2,
                "enabled": true,
                "defaultRouteId": "qwen-route",
                "routes": [{
                    "id": "qwen-route",
                    "enabled": true,
                    "targetProviderId": "qwen-target",
                    "modelSelection": {"mode": "all"},
                    "authPolicy": {"source": "provider_config"}
                }]
            }
        }),
        None,
    );
    let providers = HashMap::from([(target.id.clone(), target)]);

    let candidates =
        compile_codex_router_probe_candidates(&router, &providers).expect("compile candidates");

    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().all(|candidate| {
        candidate.provider_id.as_deref() == Some("router")
            && candidate.route_id.as_deref() == Some("qwen-route")
            && candidate.canonical_endpoint() == "https://vllm.example/v1"
    }));
    assert_eq!(candidates[0].public_model, "qwen-visible");
    assert_eq!(candidates[0].upstream_model, "Qwen/Qwen3.8");
    assert_eq!(candidates[1].public_model, "qwen-coder");
    assert_eq!(candidates[1].upstream_model, "Qwen/Qwen3-Coder");
}
