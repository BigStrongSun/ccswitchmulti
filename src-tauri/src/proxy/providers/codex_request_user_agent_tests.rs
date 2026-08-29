use std::collections::HashMap;

use http::{header, HeaderMap, HeaderValue};
use serde_json::json;

use crate::{
    protocol_compatibility::compile_provider_probe_candidate,
    provider::{LocalProxyRequestOverrides, Provider, ProviderMeta},
};

use super::codex_request::{apply_provider_header_policy, CodexThirdPartyRequestPolicy};

fn third_party_provider() -> Provider {
    let mut provider = Provider::with_id(
        "provider-a".to_string(),
        "Provider A".to_string(),
        json!({
            "auth": {"OPENAI_API_KEY": "probe-secret"},
            "base_url": "https://example.test/v1",
            "apiFormat": "openai_responses",
            "config": "model = \"model-a\"\nbase_url = \"https://example.test/v1\"\nwire_api = \"responses\"\n",
            "modelCatalog": {"models": [{
                "model": "model-a",
                "upstreamModel": "model-a",
                "apiFormat": "openai_responses"
            }]}
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some("openai_responses".to_string()),
        ..ProviderMeta::default()
    });
    provider
}

fn user_agent(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
}

#[test]
fn default_product_user_agent_is_identical_for_probe_and_production() {
    let provider = third_party_provider();
    let probe_headers = CodexThirdPartyRequestPolicy::compile(&provider)
        .expect("compile probe policy")
        .prepare_headers();
    let mut production_headers = HeaderMap::new();
    production_headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static("codex_cli_rs/0.151.0"),
    );
    apply_provider_header_policy(&provider, &mut production_headers);

    let expected = format!("CCSwitchMulti/{}", env!("CARGO_PKG_VERSION"));
    assert_eq!(user_agent(&probe_headers), Some(expected.as_str()));
    assert_eq!(user_agent(&production_headers), Some(expected.as_str()));
}

#[test]
fn custom_user_agent_wins_in_probe_and_production_header_policy() {
    let mut provider = third_party_provider();
    let meta = provider.meta.as_mut().unwrap();
    meta.custom_user_agent = Some("Gateway-Compatible/2".to_string());
    meta.local_proxy_request_overrides = Some(LocalProxyRequestOverrides {
        headers: HashMap::from([("User-Agent".to_string(), "generic-override".to_string())]),
        body: None,
    });

    let probe_headers = CodexThirdPartyRequestPolicy::compile(&provider)
        .expect("compile probe policy")
        .prepare_headers();
    let mut production_headers = HeaderMap::new();
    production_headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static("codex_cli_rs/0.151.0"),
    );
    apply_provider_header_policy(&provider, &mut production_headers);

    assert_eq!(user_agent(&probe_headers), Some("Gateway-Compatible/2"));
    assert_eq!(
        user_agent(&production_headers),
        Some("Gateway-Compatible/2")
    );
}

#[test]
fn changing_user_agent_changes_probe_request_policy_fingerprint() {
    let provider = third_party_provider();
    let mut changed = provider.clone();
    changed.meta.as_mut().unwrap().custom_user_agent = Some("Gateway-Compatible/2".to_string());

    let original_candidate =
        compile_provider_probe_candidate(&provider).expect("compile original candidate");
    let changed_candidate =
        compile_provider_probe_candidate(&changed).expect("compile changed candidate");

    assert_ne!(
        original_candidate.request_policy_fingerprint(),
        changed_candidate.request_policy_fingerprint()
    );
}
