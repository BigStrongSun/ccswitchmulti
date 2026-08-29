use std::collections::HashMap;

use http::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};

use crate::provider::{LocalProxyRequestOverrides, Provider, ProviderMeta};

use super::codex_request::{
    CodexRequestOptions, CodexRequestTransport, CodexThirdPartyRequestPolicy,
};

fn third_party_provider(api_format: &str) -> Provider {
    let mut provider = Provider::with_id(
        "third-party-provider".to_string(),
        "Third Party".to_string(),
        json!({
            "auth": {"OPENAI_API_KEY": "request-policy-secret"},
            "config": format!(r#"model = "visible-model"
model_provider = "third-party"
[model_providers.third-party]
base_url = "https://gateway.example/custom/v1"
wire_api = "{}"
"#, if api_format == "openai_chat" { "chat" } else { "responses" }),
            "apiFormat": api_format,
            "modelCatalog": {"models": [{
                "model": "visible-model",
                "upstreamModel": "upstream/model-v2",
                "apiFormat": api_format,
                "reasoning": {
                    "schemaVersion": 2,
                    "supportStatus": "confirmed_supported",
                    "controlKind": "graded",
                    "supportedEfforts": ["low", "medium", "xhigh"],
                    "defaultEffort": "medium",
                    "disableAllowed": false,
                    "upstream": {
                        "format": "string",
                        "parameter": "reasoning_effort",
                        "effortMap": {
                            "low": "low",
                            "medium": "medium",
                            "high": "xhigh",
                            "xhigh": "xhigh",
                            "max": "xhigh"
                        }
                    },
                    "outputFormat": "reasoning_content"
                }
            }]}
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some(api_format.to_string()),
        custom_user_agent: Some("CCSM-Contract/2".to_string()),
        local_proxy_request_overrides: Some(LocalProxyRequestOverrides {
            headers: HashMap::from([
                ("X-Provider-Policy".to_string(), "enabled".to_string()),
                ("Authorization".to_string(), "Bearer forbidden".to_string()),
                ("Content-Type".to_string(), "text/plain".to_string()),
            ]),
            body: Some(json!({
                "metadata": {"probePolicy": "shared"},
                "stream": false
            })),
        }),
        ..ProviderMeta::default()
    });
    provider
}

fn logical_request() -> Value {
    json!({
        "model": "visible-model",
        "input": [{
            "role": "user",
            "content": [{"type": "input_text", "text": "contract probe"}]
        }],
        "reasoning": {"effort": "high", "summary": "auto"},
        "max_output_tokens": 128,
        "stream": true,
        "_privateProbeField": "must-not-leak"
    })
}

#[test]
fn chat_request_policy_prepares_literal_production_wire_request() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_chat"))
        .expect("compile third-party request policy");

    let prepared = policy
        .prepare(
            CodexRequestTransport::ChatCompletions,
            logical_request(),
            CodexRequestOptions::default(),
        )
        .expect("prepare Chat request");

    assert_eq!(
        prepared.url,
        "https://gateway.example/custom/v1/chat/completions"
    );
    assert_eq!(
        prepared.headers[AUTHORIZATION],
        "Bearer request-policy-secret"
    );
    assert_eq!(prepared.headers[CONTENT_TYPE], "application/json");
    assert_eq!(prepared.headers[USER_AGENT], "CCSM-Contract/2");
    assert_eq!(prepared.headers["x-provider-policy"], "enabled");
    assert_ne!(prepared.headers[AUTHORIZATION], "Bearer forbidden");
    assert_eq!(prepared.body["model"], "upstream/model-v2");
    assert_eq!(prepared.body["reasoning_effort"], "xhigh");
    assert_eq!(prepared.body["max_tokens"], 128);
    assert_eq!(prepared.body["stream"], true);
    assert_eq!(prepared.body["metadata"]["probePolicy"], "shared");
    assert!(prepared.body.get("_privateProbeField").is_none());
}

#[test]
fn responses_request_policy_maps_effort_model_and_provider_overrides() {
    let policy = CodexThirdPartyRequestPolicy::compile(&third_party_provider("openai_responses"))
        .expect("compile third-party request policy");

    let prepared = policy
        .prepare(
            CodexRequestTransport::Responses,
            logical_request(),
            CodexRequestOptions::default(),
        )
        .expect("prepare Responses request");

    assert_eq!(prepared.url, "https://gateway.example/custom/v1/responses");
    assert_eq!(prepared.body["model"], "upstream/model-v2");
    assert_eq!(prepared.body["reasoning"]["effort"], "xhigh");
    assert_eq!(prepared.body["metadata"]["probePolicy"], "shared");
    assert_eq!(prepared.body["stream"], true);
    assert!(prepared.body.get("_privateProbeField").is_none());
    assert!(!policy.fingerprint().is_empty());
    assert!(!policy.credential_fingerprint().is_empty());

    let debug = format!("{policy:?}");
    assert!(!debug.contains("request-policy-secret"));
    assert!(!debug.contains("Bearer forbidden"));
}

#[test]
fn managed_provider_cannot_compile_into_active_probe_policy() {
    let mut provider = third_party_provider("openai_responses");
    provider.meta.as_mut().unwrap().provider_type = Some("codex_oauth".to_string());

    assert!(CodexThirdPartyRequestPolicy::compile(&provider).is_err());
}
