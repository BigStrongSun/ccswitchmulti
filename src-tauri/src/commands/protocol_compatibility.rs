use chrono::Utc;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    app_config::AppType,
    protocol_compatibility::{
        apply_probe_selection_to_provider, compile_provider_probe_candidate,
        endpoint::build_probe_url, run_protocol_compatibility_probe, ProbeCandidate,
        ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
        ProtocolCompatibilityRecord, TransportKind,
    },
    provider::Provider,
    services::ProviderService,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderProtocolPreflightOutcome {
    pub provider: Provider,
    pub record: ProtocolCompatibilityRecord,
    pub protocol_applied: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderProtocolSaveOutcome {
    pub provider: Provider,
    pub record: Option<ProtocolCompatibilityRecord>,
    pub protocol_applied: bool,
    pub probe_error: Option<String>,
    pub saved: bool,
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

    run_candidate_and_persist(state.inner(), candidate).await
}

#[tauri::command]
pub async fn preflight_codex_provider_protocol_compatibility(
    state: State<'_, AppState>,
    provider: Provider,
) -> Result<CodexProviderProtocolPreflightOutcome, String> {
    run_provider_preflight(state.inner(), provider).await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_codex_provider_with_protocol_preflight(
    state: State<'_, AppState>,
    provider: Provider,
    originalId: Option<String>,
    addToLive: Option<bool>,
) -> Result<CodexProviderProtocolSaveOutcome, String> {
    let (provider, record, protocol_applied, probe_error) =
        match run_provider_preflight(state.inner(), provider.clone()).await {
            Ok(outcome) => (
                outcome.provider,
                Some(outcome.record),
                outcome.protocol_applied,
                None,
            ),
            Err(error) => (provider, None, false, Some(error)),
        };

    if let Some(original_id) = originalId.as_deref() {
        ProviderService::update(
            state.inner(),
            AppType::Codex,
            Some(original_id),
            provider.clone(),
        )
    } else {
        ProviderService::add(
            state.inner(),
            AppType::Codex,
            provider.clone(),
            addToLive.unwrap_or(true),
        )
    }
    .map_err(|error| error.to_string())?;

    Ok(CodexProviderProtocolSaveOutcome {
        provider,
        record,
        protocol_applied,
        probe_error,
        saved: true,
    })
}

async fn run_provider_preflight(
    state: &AppState,
    mut provider: Provider,
) -> Result<CodexProviderProtocolPreflightOutcome, String> {
    let candidate = compile_provider_probe_candidate(&provider)?;
    let record = run_candidate_and_persist(state, candidate).await?;
    let protocol_applied = apply_probe_selection_to_provider(&mut provider, &record.result)?;
    Ok(CodexProviderProtocolPreflightOutcome {
        provider,
        record,
        protocol_applied,
    })
}

async fn run_candidate_and_persist(
    state: &AppState,
    candidate: ProbeCandidate,
) -> Result<ProtocolCompatibilityRecord, String> {
    let client = crate::proxy::http_client::build_protocol_probe_client()?;
    let result = run_protocol_compatibility_probe(candidate.clone(), &client).await;
    let record = build_record_for_result(&candidate, result)?;
    state
        .db
        .save_protocol_compatibility_result(&record)
        .map_err(|error| error.to_string())?;
    Ok(record)
}

fn build_record_for_result(
    candidate: &ProbeCandidate,
    result: ProtocolCompatibilityProbeResult,
) -> Result<ProtocolCompatibilityRecord, String> {
    let selected_transport = result.selected_transport.unwrap_or(candidate.transport);
    let effective_endpoint = build_probe_url(
        &candidate.canonical_endpoint(),
        selected_transport,
        candidate.is_full_url(),
    )?;
    let provider_id = candidate
        .provider_id
        .as_deref()
        .ok_or_else(|| "providerId is required before persisting probe evidence".to_string())?;
    let target = ProbeTargetKey::new(
        provider_id,
        candidate.route_id.as_deref(),
        &candidate.public_model,
        &candidate.upstream_model,
        selected_transport,
        &effective_endpoint,
        &candidate.authentication_kind,
    )
    .map_err(|_| "effective probe endpoint is invalid".to_string())?;
    let tested_at = Utc::now().timestamp();
    let ttl = if result.readiness == ProbeReadiness::Verified {
        VERIFIED_TTL_SECONDS
    } else {
        UNVERIFIED_TTL_SECONDS
    };
    Ok(ProtocolCompatibilityRecord::new(
        target,
        result,
        tested_at,
        tested_at + ttl,
    ))
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
