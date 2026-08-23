use chrono::Utc;
use serde::Deserialize;
use tauri::State;

use crate::{
    protocol_compatibility::{
        endpoint::build_probe_url, run_protocol_compatibility_probe, ProbeCandidate,
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityRecord, TransportKind,
    },
    store::AppState,
};

const VERIFIED_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
const UNVERIFIED_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolCompatibilityProbeRequest {
    pub provider_id: String,
    pub route_id: Option<String>,
    pub public_model: String,
    pub upstream_model: String,
    pub base_url: String,
    pub api_key: String,
    pub is_full_url: Option<bool>,
    pub configured_wire_api: Option<String>,
    pub authentication_kind: Option<String>,
}

#[tauri::command]
pub async fn probe_codex_protocol_compatibility(
    state: State<'_, AppState>,
    request: ProtocolCompatibilityProbeRequest,
) -> Result<ProtocolCompatibilityRecord, String> {
    let provider_id = required("providerId", &request.provider_id)?;
    let public_model = required("publicModel", &request.public_model)?;
    let upstream_model = required("upstreamModel", &request.upstream_model)?;
    let base_url = required("baseUrl", &request.base_url)?;
    let api_key = required("apiKey", &request.api_key)?;
    let configured_hint = parse_transport_hint(request.configured_wire_api.as_deref());
    let authentication_kind = request
        .authentication_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("bearer");
    let is_full_url = request.is_full_url.unwrap_or(false);

    let candidate = ProbeCandidate::new(
        Some(provider_id),
        request.route_id.as_deref(),
        public_model,
        upstream_model,
        configured_hint,
        base_url,
        authentication_kind,
    )
    .map_err(|_| "baseUrl is not a valid absolute URL".to_string())?
    .with_full_url(is_full_url)
    .with_bearer_token(api_key)
    .map_err(|_| "apiKey cannot be represented as an HTTP authorization header".to_string())?;

    let client = crate::proxy::http_client::build_protocol_probe_client()?;
    let result = run_protocol_compatibility_probe(candidate, &client).await;
    let selected_transport = result.selected_transport.unwrap_or(configured_hint);
    let effective_endpoint = build_probe_url(base_url, selected_transport, is_full_url)?;
    let target = ProbeTargetKey::new(
        provider_id,
        request.route_id.as_deref(),
        public_model,
        upstream_model,
        selected_transport,
        &effective_endpoint,
        authentication_kind,
    )
    .map_err(|_| "effective probe endpoint is invalid".to_string())?;
    let tested_at = Utc::now().timestamp();
    let ttl = if result.readiness == ProbeReadiness::Verified {
        VERIFIED_TTL_SECONDS
    } else {
        UNVERIFIED_TTL_SECONDS
    };
    let record = ProtocolCompatibilityRecord::new(target, result, tested_at, tested_at + ttl);
    state
        .db
        .save_protocol_compatibility_result(&record)
        .map_err(|error| error.to_string())?;
    Ok(record)
}

fn required<'a>(field: &str, value: &'a str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(value)
    }
}

fn parse_transport_hint(value: Option<&str>) -> TransportKind {
    match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "responses" | "openai_responses" => TransportKind::OpenAiResponses,
        _ => TransportKind::OpenAiChat,
    }
}
