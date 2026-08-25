use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    future::Future,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, State};
use uuid::Uuid;

use crate::{
    app_config::AppType,
    protocol_compatibility::{
        apply_probe_selection_to_provider, compile_codex_router_probe_candidates,
        compile_provider_probe_candidates, endpoint::build_probe_url,
        run_protocol_compatibility_probe, run_protocol_compatibility_probe_with_reporter,
        ManualReasoningOverride, ProbeCandidate, ProbeReadiness, ProbeTargetKey,
        ProtocolCompatibilityProbeResult, ProtocolCompatibilityRecord, ProtocolProbeProgressEvent,
        ReasoningManualOverrideRecord, ReasoningProjection, ReasoningSemantic, TransportKind,
        PROBE_PROFILE_VERSION,
    },
    provider::Provider,
    services::ProviderService,
    store::AppState,
};

const VERIFIED_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;
const UNVERIFIED_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
const OVERRIDE_PLAN_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanReasoningOverrideRequest {
    pub target: ProbeTargetKey,
    pub override_spec: ManualReasoningOverride,
    pub projection: ReasoningProjection,
    pub reason: String,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningOverridePlan {
    pub plan_token: String,
    pub target: ProbeTargetKey,
    pub expected_revision: i64,
    pub next_revision: i64,
    pub override_spec: ManualReasoningOverride,
    pub projection: ReasoningProjection,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReasoningOverrideRequest {
    pub plan_token: String,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearReasoningOverrideRequest {
    pub target: ProbeTargetKey,
    pub expected_revision: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningCompatibilityInspection {
    pub target: ProbeTargetKey,
    pub profile: Option<ProtocolCompatibilityRecord>,
    pub manual_override: Option<ReasoningManualOverrideRecord>,
    pub effective_projection: ReasoningProjection,
    pub revision: i64,
}

#[derive(Clone)]
struct PendingReasoningOverridePlan {
    request: PlanReasoningOverrideRequest,
    expires_at: Instant,
}

static REASONING_OVERRIDE_PLANS: OnceLock<Mutex<HashMap<String, PendingReasoningOverridePlan>>> =
    OnceLock::new();

fn reasoning_override_plans() -> &'static Mutex<HashMap<String, PendingReasoningOverridePlan>> {
    REASONING_OVERRIDE_PLANS.get_or_init(|| Mutex::new(HashMap::new()))
}

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
    pub records: Vec<ProtocolCompatibilityRecord>,
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
    on_event: Channel<ProtocolProbeProgressEvent>,
) -> Result<CodexProviderProtocolPreflightOutcome, String> {
    run_provider_preflight(state.inner(), provider, move |event| {
        let _ = on_event.send(event);
    })
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_codex_provider_with_protocol_preflight(
    state: State<'_, AppState>,
    provider: Provider,
    originalId: Option<String>,
    addToLive: Option<bool>,
) -> Result<CodexProviderProtocolSaveOutcome, String> {
    let (provider, records, protocol_applied, probe_error) =
        match automatic_codex_provider_preflight(state.inner(), provider.clone()).await {
            Ok((provider, records)) => {
                let protocol_applied = unanimous_selection_was_applied(&records);
                (provider, records, protocol_applied, None)
            }
            Err(error) => (provider, Vec::new(), false, Some(error)),
        };
    let record = records.first().cloned();

    if let Some(original_id) = originalId.as_deref() {
        ProviderService::update_with_protocol_profiles(
            state.inner(),
            AppType::Codex,
            Some(original_id),
            provider.clone(),
            &records,
        )
    } else {
        ProviderService::add_with_protocol_profiles(
            state.inner(),
            AppType::Codex,
            provider.clone(),
            addToLive.unwrap_or(true),
            &records,
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

async fn run_provider_preflight<F>(
    state: &AppState,
    mut provider: Provider,
    reporter: F,
) -> Result<CodexProviderProtocolPreflightOutcome, String>
where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    let candidates = compile_provider_probe_candidates(&provider)?;
    let total = candidates.len();
    let records = run_candidate_batch_with(candidates, |candidate| {
        run_candidate_result_with_reporter(state, candidate, &reporter)
    })
    .await?;
    let protocol_applied = apply_unanimous_probe_selection(&mut provider, &records)?;
    reporter(batch_finished_event(total, &records));
    Ok(CodexProviderProtocolPreflightOutcome {
        provider,
        records,
        protocol_applied,
    })
}

pub(crate) async fn automatic_codex_provider_preflight(
    state: &AppState,
    provider: Provider,
) -> Result<(Provider, Vec<ProtocolCompatibilityRecord>), String> {
    if provider.uses_manual_codex_protocol()
        || provider.category.as_deref() == Some("official")
        || provider.uses_managed_account_auth()
    {
        return Ok((provider, Vec::new()));
    }

    if provider.settings_config.get("codexRouting").is_some() {
        let providers = state
            .db
            .get_all_providers(AppType::Codex.as_str())
            .map_err(|error| error.to_string())?
            .into_iter()
            .collect::<HashMap<_, _>>();
        let candidates = compile_codex_router_probe_candidates(&provider, &providers)?;
        if candidates.is_empty() {
            return Ok((provider, Vec::new()));
        }
        let records = run_candidate_batch_with(candidates, |candidate| {
            run_automatic_candidate_result(state, candidate)
        })
        .await?;
        return Ok((provider, records));
    }
    let mut provider = provider;
    let candidates = compile_provider_probe_candidates(&provider)?;
    let records = run_candidate_batch_with(candidates, |candidate| {
        run_automatic_candidate_result(state, candidate)
    })
    .await?;
    apply_unanimous_probe_selection(&mut provider, &records)?;
    Ok((provider, records))
}

fn apply_unanimous_probe_selection(
    provider: &mut Provider,
    records: &[ProtocolCompatibilityRecord],
) -> Result<bool, String> {
    let Some(selected) = unanimous_selected_transport(records) else {
        return Ok(false);
    };
    let mut selected_result = records[0].result.clone();
    selected_result.selected_transport = Some(selected);
    apply_probe_selection_to_provider(provider, &selected_result)
}

fn unanimous_selected_transport(records: &[ProtocolCompatibilityRecord]) -> Option<TransportKind> {
    let selected = records
        .first()
        .and_then(|record| record.result.selected_transport)?;
    records
        .iter()
        .all(|record| record.result.selected_transport == Some(selected))
        .then_some(selected)
}

fn unanimous_selection_was_applied(records: &[ProtocolCompatibilityRecord]) -> bool {
    unanimous_selected_transport(records).is_some()
}

fn batch_finished_event(
    total: usize,
    records: &[ProtocolCompatibilityRecord],
) -> ProtocolProbeProgressEvent {
    let verified = records
        .iter()
        .filter(|record| record.result.readiness == ProbeReadiness::Verified)
        .count();
    let partial = records
        .iter()
        .filter(|record| record.result.readiness == ProbeReadiness::Partial)
        .count();
    ProtocolProbeProgressEvent::BatchFinished {
        total,
        verified,
        partial,
        failed: total.saturating_sub(verified + partial),
    }
}

async fn run_candidate_and_persist(
    state: &AppState,
    candidate: ProbeCandidate,
) -> Result<ProtocolCompatibilityRecord, String> {
    let record = run_candidate(state, candidate).await?;
    state
        .db
        .save_protocol_compatibility_result(&record)
        .map_err(|error| error.to_string())?;
    Ok(record)
}

async fn run_candidate(
    state: &AppState,
    candidate: ProbeCandidate,
) -> Result<ProtocolCompatibilityRecord, String> {
    run_candidate_batch_with(vec![candidate], |candidate| {
        run_candidate_result(state, candidate)
    })
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| "protocol probe produced no record".to_string())
}

async fn run_candidate_result(
    state: &AppState,
    candidate: ProbeCandidate,
) -> Result<ProtocolCompatibilityProbeResult, String> {
    let _lease = state.try_acquire_protocol_probe(&candidate.lease_key())?;
    let client = crate::proxy::http_client::build_protocol_probe_client()?;
    Ok(run_protocol_compatibility_probe(candidate, &client).await)
}

async fn run_candidate_result_with_reporter<F>(
    state: &AppState,
    candidate: ProbeCandidate,
    reporter: &F,
) -> Result<ProtocolCompatibilityProbeResult, String>
where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    let _lease = state.try_acquire_protocol_probe(&candidate.lease_key())?;
    let client = crate::proxy::http_client::build_protocol_probe_client()?;
    Ok(run_protocol_compatibility_probe_with_reporter(candidate, &client, reporter).await)
}

async fn run_automatic_candidate_result(
    state: &AppState,
    candidate: ProbeCandidate,
) -> Result<ProtocolCompatibilityProbeResult, String> {
    if let Some(result) = find_cached_candidate_result(state, &candidate, Utc::now().timestamp())? {
        return Ok(result);
    }
    run_candidate_result(state, candidate).await
}

fn find_cached_candidate_result(
    state: &AppState,
    candidate: &ProbeCandidate,
    now: i64,
) -> Result<Option<ProtocolCompatibilityProbeResult>, String> {
    let mut newest: Option<ProtocolCompatibilityRecord> = None;
    for transport in [TransportKind::OpenAiResponses, TransportKind::OpenAiChat] {
        let target = target_for_candidate(candidate, transport)?;
        let Some(record) = state
            .db
            .get_protocol_compatibility_result(&target)
            .map_err(|error| error.to_string())?
        else {
            continue;
        };
        if record.probe_version != PROBE_PROFILE_VERSION
            || record.expires_at < now
            || record.result.selected_transport != Some(transport)
        {
            continue;
        }
        if newest
            .as_ref()
            .is_none_or(|current| record.tested_at > current.tested_at)
        {
            newest = Some(record);
        }
    }
    Ok(newest.map(|record| record.result))
}

async fn run_candidate_batch_with<F, Fut>(
    candidates: Vec<ProbeCandidate>,
    mut execute: F,
) -> Result<Vec<ProtocolCompatibilityRecord>, String>
where
    F: FnMut(ProbeCandidate) -> Fut,
    Fut: Future<Output = Result<ProtocolCompatibilityProbeResult, String>>,
{
    let mut results_by_target: HashMap<String, ProtocolCompatibilityProbeResult> = HashMap::new();
    let mut records = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let execution_key = candidate.lease_key();
        let result = if let Some(result) = results_by_target.get(&execution_key) {
            result.clone()
        } else {
            let result = execute(candidate.clone()).await?;
            results_by_target.insert(execution_key, result.clone());
            result
        };
        records.push(build_record_for_result(&candidate, result)?);
    }
    Ok(records)
}

fn build_record_for_result(
    candidate: &ProbeCandidate,
    result: ProtocolCompatibilityProbeResult,
) -> Result<ProtocolCompatibilityRecord, String> {
    let selected_transport = result.selected_transport.unwrap_or(candidate.transport);
    let target = target_for_candidate(candidate, selected_transport)?;
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

fn target_for_candidate(
    candidate: &ProbeCandidate,
    transport: TransportKind,
) -> Result<ProbeTargetKey, String> {
    let effective_endpoint = build_probe_url(
        &candidate.canonical_endpoint(),
        transport,
        candidate.is_full_url(),
    )?;
    let provider_id = candidate
        .provider_id
        .as_deref()
        .ok_or_else(|| "providerId is required before persisting probe evidence".to_string())?;
    ProbeTargetKey::new(
        provider_id,
        candidate.route_id.as_deref(),
        &candidate.public_model,
        &candidate.upstream_model,
        transport,
        &effective_endpoint,
        &candidate.authentication_kind,
    )
    .map_err(|_| "effective probe endpoint is invalid".to_string())
    .map(|target| target.with_credential_fingerprint(candidate.credential_fingerprint()))
}

#[tauri::command]
pub fn get_codex_protocol_compatibility(
    state: State<'_, AppState>,
    target: ProbeTargetKey,
) -> Result<ReasoningCompatibilityInspection, String> {
    inspect_reasoning_compatibility(state.inner(), target)
}

#[tauri::command]
pub fn plan_codex_reasoning_override(
    state: State<'_, AppState>,
    request: PlanReasoningOverrideRequest,
) -> Result<ReasoningOverridePlan, String> {
    plan_reasoning_override(state.inner(), request)
}

#[tauri::command]
pub fn apply_codex_reasoning_override(
    state: State<'_, AppState>,
    request: ApplyReasoningOverrideRequest,
) -> Result<ReasoningCompatibilityInspection, String> {
    apply_reasoning_override(state.inner(), request)
}

#[tauri::command]
pub fn clear_codex_reasoning_override(
    state: State<'_, AppState>,
    request: ClearReasoningOverrideRequest,
) -> Result<ReasoningCompatibilityInspection, String> {
    clear_reasoning_override(state.inner(), request)
}

pub(crate) fn inspect_reasoning_compatibility(
    state: &AppState,
    target: ProbeTargetKey,
) -> Result<ReasoningCompatibilityInspection, String> {
    validate_override_target(&target)?;
    let profile = state
        .db
        .get_protocol_compatibility_result(&target)
        .map_err(|error| error.to_string())?;
    let manual_override = state
        .db
        .get_reasoning_manual_override(&target)
        .map_err(|error| error.to_string())?;
    let revision = state
        .db
        .get_reasoning_manual_override_revision(&target)
        .map_err(|error| error.to_string())?;
    let effective_projection = manual_override
        .as_ref()
        .map(|record| record.projection)
        .or_else(|| {
            profile
                .as_ref()
                .map(|record| record.automatic_reasoning_projection(Utc::now().timestamp()))
        })
        .unwrap_or(ReasoningProjection::None);
    Ok(ReasoningCompatibilityInspection {
        target,
        profile,
        manual_override,
        effective_projection,
        revision,
    })
}

pub(crate) fn plan_reasoning_override(
    state: &AppState,
    request: PlanReasoningOverrideRequest,
) -> Result<ReasoningOverridePlan, String> {
    validate_override_target(&request.target)?;
    if request.reason.trim().is_empty() {
        return Err("validation_failed: reason is required".to_string());
    }
    if request.expected_revision < 0 {
        return Err("validation_failed: expectedRevision must be non-negative".to_string());
    }
    let current_revision = state
        .db
        .get_reasoning_manual_override_revision(&request.target)
        .map_err(|error| error.to_string())?;
    if current_revision != request.expected_revision {
        return Err("revision_conflict".to_string());
    }
    let observed = state
        .db
        .get_protocol_compatibility_result(&request.target)
        .map_err(|error| error.to_string())?
        .as_ref()
        .map(observed_reasoning_semantic)
        .unwrap_or(ReasoningSemantic::None);
    request
        .override_spec
        .validate_against(observed)
        .and_then(|_| {
            request
                .override_spec
                .validate_projection(request.projection)
        })
        .map_err(|error| format!("validation_failed: {error}"))?;

    let plan_token = Uuid::new_v4().simple().to_string();
    let plan = ReasoningOverridePlan {
        plan_token: plan_token.clone(),
        target: request.target.clone(),
        expected_revision: request.expected_revision,
        next_revision: request.expected_revision + 1,
        override_spec: request.override_spec,
        projection: request.projection,
        reason: request.reason.trim().to_string(),
    };
    let mut plans = reasoning_override_plans()
        .lock()
        .map_err(|_| "override_plan_lock_failed".to_string())?;
    plans.retain(|_, pending| pending.expires_at > Instant::now());
    plans.insert(
        plan_token,
        PendingReasoningOverridePlan {
            request,
            expires_at: Instant::now() + OVERRIDE_PLAN_TTL,
        },
    );
    Ok(plan)
}

pub(crate) fn apply_reasoning_override(
    state: &AppState,
    request: ApplyReasoningOverrideRequest,
) -> Result<ReasoningCompatibilityInspection, String> {
    let pending = {
        let plans = reasoning_override_plans()
            .lock()
            .map_err(|_| "override_plan_lock_failed".to_string())?;
        plans
            .get(request.plan_token.trim())
            .cloned()
            .ok_or_else(|| "approval_required: invalid plan token".to_string())?
    };
    if pending.expires_at <= Instant::now() {
        return Err("approval_required: expired plan token".to_string());
    }
    if request.expected_revision != pending.request.expected_revision {
        return Err("revision_conflict".to_string());
    }
    state
        .db
        .save_reasoning_manual_override(
            &pending.request.target,
            pending.request.override_spec,
            pending.request.projection,
            &pending.request.reason,
            Utc::now().timestamp(),
            request.expected_revision,
        )
        .map_err(|error| error.to_string())?;
    reasoning_override_plans()
        .lock()
        .map_err(|_| "override_plan_lock_failed".to_string())?
        .remove(request.plan_token.trim());
    inspect_reasoning_compatibility(state, pending.request.target)
}

pub(crate) fn clear_reasoning_override(
    state: &AppState,
    request: ClearReasoningOverrideRequest,
) -> Result<ReasoningCompatibilityInspection, String> {
    validate_override_target(&request.target)?;
    state
        .db
        .clear_reasoning_manual_override(
            &request.target,
            request.expected_revision,
            Utc::now().timestamp(),
        )
        .map_err(|error| error.to_string())?;
    inspect_reasoning_compatibility(state, request.target)
}

fn observed_reasoning_semantic(record: &ProtocolCompatibilityRecord) -> ReasoningSemantic {
    let Some(selected_transport) = record.result.selected_transport else {
        return ReasoningSemantic::None;
    };
    record
        .result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == selected_transport)
        .map(|branch| branch.reasoning_shape.semantic)
        .unwrap_or(ReasoningSemantic::None)
}

fn validate_override_target(target: &ProbeTargetKey) -> Result<(), String> {
    if target.provider_id.trim().is_empty()
        || target.public_model.trim().is_empty()
        || target.upstream_model.trim().is_empty()
        || target.endpoint_fingerprint.trim().is_empty()
        || target.authentication_kind.trim().is_empty()
    {
        return Err("invalid_target".to_string());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::{
        apply_reasoning_override, apply_unanimous_probe_selection, batch_finished_event,
        clear_reasoning_override, find_cached_candidate_result, inspect_reasoning_compatibility,
        plan_reasoning_override, run_candidate_batch_with, unanimous_selection_was_applied,
        ApplyReasoningOverrideRequest, ClearReasoningOverrideRequest, PlanReasoningOverrideRequest,
    };
    use crate::protocol_compatibility::{
        HistoryReplay, ManualReasoningOverride, ProbeCandidate, ProbeReadiness, ProbeTargetKey,
        ProtocolCompatibilityProbeResult, ProtocolCompatibilityRecord, ProtocolProbeProgressEvent,
        ReasoningProjection, ReasoningSemantic, ReasoningSource, TransportKind,
    };
    use crate::provider::{Provider, ProviderMeta};
    use crate::{database::Database, store::AppState};
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn alias_candidate(provider_id: &str, route_id: &str, public_model: &str) -> ProbeCandidate {
        ProbeCandidate::new(
            Some(provider_id),
            Some(route_id),
            public_model,
            "Qwen/Qwen3.8",
            TransportKind::OpenAiResponses,
            "https://vllm.example/v1",
            "bearer",
        )
        .unwrap()
        .with_bearer_token("probe-secret")
        .unwrap()
    }

    fn ordinary_provider() -> Provider {
        Provider {
            id: "provider-a".to_string(),
            name: "Provider A".to_string(),
            settings_config: json!({
                "auth": {"OPENAI_API_KEY": "probe-secret"},
                "apiFormat": "openai_responses",
                "config": "model = \"model-a\"\nmodel_provider = \"provider-a\"\n[model_providers.provider-a]\nbase_url = \"https://example.test/v1\"\nwire_api = \"responses\"\n",
                "modelCatalog": {"models": [
                    {"model": "model-a", "upstreamModel": "model-a", "apiFormat": "openai_responses"},
                    {"model": "model-b", "upstreamModel": "model-b", "apiFormat": "openai_responses"}
                ]}
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                api_format: Some("openai_responses".to_string()),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn probe_record(
        public_model: &str,
        transport: Option<TransportKind>,
        readiness: ProbeReadiness,
    ) -> ProtocolCompatibilityRecord {
        let candidate = alias_candidate("provider-a", "route-a", public_model);
        super::build_record_for_result(
            &candidate,
            ProtocolCompatibilityProbeResult {
                selected_transport: transport,
                readiness,
                branches: Vec::new(),
            },
        )
        .expect("build record")
    }

    #[test]
    fn unanimous_multi_model_selection_applies_the_global_protocol() {
        let mut provider = ordinary_provider();
        let records = vec![
            probe_record(
                "model-a",
                Some(TransportKind::OpenAiChat),
                ProbeReadiness::Verified,
            ),
            probe_record(
                "model-b",
                Some(TransportKind::OpenAiChat),
                ProbeReadiness::Partial,
            ),
        ];

        assert!(apply_unanimous_probe_selection(&mut provider, &records)
            .expect("apply unanimous selection"));
        assert_eq!(
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_chat")
        );
        assert_eq!(provider.settings_config["apiFormat"], "openai_chat");
        assert!(provider.settings_config["config"]
            .as_str()
            .expect("config")
            .contains("wire_api = \"chat\""));
    }

    #[test]
    fn mixed_multi_model_selection_keeps_the_existing_global_protocol() {
        let mut provider = ordinary_provider();
        let original = provider.clone();
        let records = vec![
            probe_record(
                "model-a",
                Some(TransportKind::OpenAiResponses),
                ProbeReadiness::Verified,
            ),
            probe_record(
                "model-b",
                Some(TransportKind::OpenAiChat),
                ProbeReadiness::Verified,
            ),
        ];

        assert!(!apply_unanimous_probe_selection(&mut provider, &records)
            .expect("reject mixed selection"));
        assert_eq!(
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            original
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref())
        );
        assert_eq!(provider.settings_config, original.settings_config);
        assert!(!unanimous_selection_was_applied(&records));
    }

    #[test]
    fn batch_finished_counts_verified_partial_and_failed_models() {
        let records = vec![
            probe_record(
                "model-a",
                Some(TransportKind::OpenAiResponses),
                ProbeReadiness::Verified,
            ),
            probe_record(
                "model-b",
                Some(TransportKind::OpenAiChat),
                ProbeReadiness::Partial,
            ),
        ];

        assert_eq!(
            batch_finished_event(3, &records),
            ProtocolProbeProgressEvent::BatchFinished {
                total: 3,
                verified: 1,
                partial: 1,
                failed: 1,
            }
        );
    }

    #[tokio::test]
    async fn routed_aliases_share_one_physical_probe_but_persist_distinct_profiles() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_probe = calls.clone();
        let candidates = vec![
            alias_candidate("router", "qwen", "qwen3.8"),
            alias_candidate("router", "qwen", "qwen-flash"),
        ];

        let records = run_candidate_batch_with(candidates, move |_| {
            calls_for_probe.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiChat),
                readiness: ProbeReadiness::Partial,
                branches: Vec::new(),
            }))
        })
        .await
        .expect("run batch");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].target.public_model, "qwen3.8");
        assert_eq!(records[1].target.public_model, "qwen-flash");
        assert_ne!(records[0].storage_key(), records[1].storage_key());
    }

    #[test]
    fn automatic_probe_cache_reuses_only_unexpired_current_version_evidence() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let candidate = alias_candidate("router", "qwen", "qwen3.8");
        let result = ProtocolCompatibilityProbeResult {
            selected_transport: Some(TransportKind::OpenAiChat),
            readiness: ProbeReadiness::Partial,
            branches: Vec::new(),
        };
        let mut current = super::build_record_for_result(&candidate, result.clone())
            .expect("build current record");
        current.tested_at = 100;
        current.expires_at = 200;
        db.save_protocol_compatibility_result(&current)
            .expect("save current record");

        assert_eq!(
            find_cached_candidate_result(&state, &candidate, 150).expect("read cache"),
            Some(result.clone())
        );
        assert_eq!(
            find_cached_candidate_result(&state, &candidate, 201).expect("expired cache"),
            None
        );

        let mut stale_version =
            ProtocolCompatibilityRecord::new(current.target.clone(), result, 300, 400);
        stale_version.probe_version = 0;
        db.save_protocol_compatibility_result(&stale_version)
            .expect("replace with stale version");
        assert_eq!(
            find_cached_candidate_result(&state, &candidate, 350).expect("versioned cache"),
            None
        );
    }

    #[test]
    fn manual_override_plan_apply_and_clear_are_revision_guarded() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db);
        let target = ProbeTargetKey::new(
            "provider-a",
            Some("route-a"),
            "public-model",
            "upstream-model",
            TransportKind::OpenAiChat,
            "https://example.test/v1/chat/completions",
            "bearer",
        )
        .unwrap()
        .with_credential("secret-a");

        let invalid = plan_reasoning_override(
            &state,
            PlanReasoningOverrideRequest {
                target: target.clone(),
                override_spec: ManualReasoningOverride::new(
                    ReasoningSemantic::Summary,
                    ReasoningSource::NativeResponses,
                    HistoryReplay::Omit,
                ),
                projection: ReasoningProjection::RawReasoningText,
                reason: "invalid raw projection".to_string(),
                expected_revision: 0,
            },
        )
        .expect_err("summary evidence cannot become raw reasoning");
        assert!(invalid.contains("validation_failed"));

        let plan = plan_reasoning_override(
            &state,
            PlanReasoningOverrideRequest {
                target: target.clone(),
                override_spec: ManualReasoningOverride::new(
                    ReasoningSemantic::Readable,
                    ReasoningSource::ReasoningContent,
                    HistoryReplay::ChatReasoningContent,
                ),
                projection: ReasoningProjection::RawReasoningText,
                reason: "provider documentation confirms reasoning_content".to_string(),
                expected_revision: 0,
            },
        )
        .expect("plan override");
        assert!(!plan
            .plan_token
            .contains("provider documentation confirms reasoning_content"));

        let conflict = apply_reasoning_override(
            &state,
            ApplyReasoningOverrideRequest {
                plan_token: plan.plan_token.clone(),
                expected_revision: 1,
            },
        )
        .expect_err("apply must use planned revision");
        assert!(conflict.contains("revision_conflict"));

        let applied = apply_reasoning_override(
            &state,
            ApplyReasoningOverrideRequest {
                plan_token: plan.plan_token,
                expected_revision: 0,
            },
        )
        .expect("apply override");
        assert_eq!(applied.manual_override.as_ref().unwrap().revision, 1);
        assert_eq!(
            applied.effective_projection,
            ReasoningProjection::RawReasoningText
        );
        assert_eq!(
            inspect_reasoning_compatibility(&state, target.clone())
                .expect("inspect override")
                .effective_projection,
            ReasoningProjection::RawReasoningText
        );

        let cleared = clear_reasoning_override(
            &state,
            ClearReasoningOverrideRequest {
                target,
                expected_revision: 1,
            },
        )
        .expect("clear override");
        assert!(cleared.manual_override.is_none());
        assert_eq!(cleared.revision, 2);
    }
}
