use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
    codex_multirouter::projection::{CodexRoutingProjectionStatus, ProjectionState},
    codex_multirouter::provider_set::{
        plan_codex_provider_set, plan_manual_codex_provider_set, CodexProviderSetPlan,
        CodexProviderSetPreview,
    },
    protocol_compatibility::{
        compile_codex_router_probe_candidates, compile_provider_probe_candidates,
        run_protocol_compatibility_probe, run_protocol_compatibility_probe_with_reporter,
        ManualReasoningOverride, ProbeCandidate, ProbeReadiness, ProbeStageStatus, ProbeTargetKey,
        ProtocolCompatibilityProbeResult, ProtocolCompatibilityRecord, ProtocolProbeProgressEvent,
        ReasoningManualOverrideRecord, ReasoningProjection, ReasoningSemantic, TransportKind,
        PROBE_PROFILE_VERSION,
    },
    provider::{Provider, UniversalProvider},
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
    pub adaptation_preview: crate::commands::provider::CodexProviderAdaptationView,
    pub records: Vec<ProtocolCompatibilityRecord>,
    pub observations: Vec<ProtocolCompatibilityRecord>,
    pub receipt_ids: Vec<String>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexProtocolProbeScope {
    #[default]
    AutomaticModels,
    AllEnabledModels,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCodexProviderSetRequest {
    pub provider: Provider,
    pub receipt_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexProviderSetCommitIntent {
    AcceptAuto,
    ConfirmManual,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitCodexProviderSetRequest {
    pub provider: Provider,
    pub receipt_ids: Vec<String>,
    pub digest: String,
    pub intent: CodexProviderSetCommitIntent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSetCommitOutcome {
    pub preview: CodexProviderSetPreview,
    pub snapshot: crate::commands::provider::CodexProviderEditorSnapshot,
    pub projections: Vec<crate::codex_multirouter::projection::CodexRoutingProjectionStatus>,
    pub status: CodexProviderSetCommitStatus,
    pub projection_error_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSetBatchSourceRequest {
    pub provider: Provider,
    pub receipt_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareCodexProviderSetBatchRequest {
    pub sources: Vec<CodexProviderSetBatchSourceRequest>,
    pub router: Provider,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSetBatchPreview {
    pub digest: String,
    pub source_previews: Vec<CodexProviderSetPreview>,
    pub router_provider_id: String,
    pub blocked: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitCodexProviderSetBatchRequest {
    pub sources: Vec<CodexProviderSetBatchSourceRequest>,
    pub router: Provider,
    pub digest: String,
    pub intent: CodexProviderSetCommitIntent,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderSetBatchCommitOutcome {
    pub preview: CodexProviderSetBatchPreview,
    pub router: Provider,
    pub source_snapshots: Vec<crate::commands::provider::CodexProviderEditorSnapshot>,
    pub projections: Vec<crate::codex_multirouter::projection::CodexRoutingProjectionStatus>,
    pub status: CodexProviderSetCommitStatus,
    pub projection_error_code: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexProviderSetCommitStatus {
    Committed,
    CommittedWithProjectionError,
}

fn classify_codex_provider_set_projection_commit(
    result: Result<
        crate::codex_multirouter::mutation::CodexProviderMutationOutcome,
        crate::error::AppError,
    >,
) -> (
    Vec<CodexRoutingProjectionStatus>,
    CodexProviderSetCommitStatus,
    Option<String>,
) {
    match result {
        Ok(outcome)
            if outcome
                .projections
                .iter()
                .all(|projection| projection.state == ProjectionState::Ready) =>
        {
            (
                outcome.projections,
                CodexProviderSetCommitStatus::Committed,
                None,
            )
        }
        Ok(outcome) => {
            log::warn!("Codex Provider Set committed; live projection is pending and will retry");
            (
                outcome.projections,
                CodexProviderSetCommitStatus::CommittedWithProjectionError,
                Some("codex_provider_set_live_projection_failed".to_string()),
            )
        }
        Err(error) => {
            log::warn!("Codex Provider Set committed; live projection will retry: {error}");
            (
                Vec::new(),
                CodexProviderSetCommitStatus::CommittedWithProjectionError,
                Some("codex_provider_set_live_projection_failed".to_string()),
            )
        }
    }
}

#[tauri::command]
pub fn prepare_codex_provider_set_batch(
    state: State<'_, AppState>,
    request: PrepareCodexProviderSetBatchRequest,
) -> Result<CodexProviderSetBatchPreview, String> {
    prepare_codex_provider_set_batch_internal(state.inner(), request, Utc::now().timestamp())
        .map(|(_, preview)| preview)
}

#[tauri::command]
pub fn commit_codex_provider_set_batch(
    state: State<'_, AppState>,
    request: CommitCodexProviderSetBatchRequest,
) -> Result<CodexProviderSetBatchCommitOutcome, String> {
    commit_codex_provider_set_batch_internal_with_publisher(
        state.inner(),
        request,
        Utc::now().timestamp(),
        |artifact| {
            crate::codex_config::publish_codex_multirouter_projection_for_database(
                state.db.as_ref(),
                &artifact.projection_settings,
            )
            .map_err(|error| error.to_string())
        },
    )
}

fn prepare_codex_provider_set_batch_internal(
    state: &AppState,
    request: PrepareCodexProviderSetBatchRequest,
    now: i64,
) -> Result<
    (
        crate::codex_multirouter::mutation::PreparedCodexProviderSetBatchCommit,
        CodexProviderSetBatchPreview,
    ),
    String,
> {
    if request.sources.is_empty() {
        return Err("codex_provider_set_batch_sources_required".to_string());
    }
    let mut source_ids = std::collections::HashSet::new();
    let mut sources = Vec::with_capacity(request.sources.len());
    for source in request.sources {
        let provider =
            ProviderService::prepare_provider_for_mutation(state, &AppType::Codex, source.provider)
                .map_err(|error| error.to_string())?;
        if !source_ids.insert(provider.id.clone()) {
            return Err("codex_provider_set_batch_duplicate_source".to_string());
        }
        let records = if provider.uses_fixed_codex_responses_transport()
            || (provider.uses_manual_codex_protocol() && !provider.has_codex_protocol_overrides())
        {
            Vec::new()
        } else {
            if source.receipt_ids.is_empty() && !provider.uses_manual_codex_protocol() {
                return Err("codex_provider_set_probe_required".to_string());
            }
            if source.receipt_ids.is_empty() {
                Vec::new()
            } else {
                let (records, _) =
                    load_codex_provider_set_probe_receipts(state, &source.receipt_ids)?;
                validate_codex_provider_set_probe_records(&provider, &records)?;
                records
            }
        };
        sources.push((provider, records));
    }
    let router =
        ProviderService::prepare_provider_for_mutation(state, &AppType::Codex, request.router)
            .map_err(|error| error.to_string())?;
    let prepared = crate::codex_multirouter::mutation::prepare_codex_provider_set_batch_commit(
        state.db.as_ref(),
        sources,
        router,
        now,
    )
    .map_err(|error| error.to_string())?;
    let digest_material = json!({
        "sourcePreviews": prepared.source_previews,
        "routerProviderId": prepared.router.id,
        "codexRouting": prepared.router.settings_config.get("codexRouting"),
        "projectionRouterIds": prepared.projection_router_ids,
        "currentProvider": state.db.get_current_provider(AppType::Codex.as_str()).map_err(|error| error.to_string())?,
    });
    let digest_bytes = serde_json::to_vec(&digest_material).map_err(|error| error.to_string())?;
    let digest = Sha256::digest(digest_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let preview = CodexProviderSetBatchPreview {
        digest,
        source_previews: prepared.source_previews.clone(),
        router_provider_id: prepared.router.id.clone(),
        blocked: prepared.blocked,
    };
    Ok((prepared, preview))
}

fn commit_codex_provider_set_batch_internal_with_publisher<F>(
    state: &AppState,
    request: CommitCodexProviderSetBatchRequest,
    now: i64,
    publish: F,
) -> Result<CodexProviderSetBatchCommitOutcome, String>
where
    F: FnMut(
        &crate::codex_multirouter::projection::CodexRoutingProjectionArtifact,
    ) -> Result<crate::codex_multirouter::projection::ProjectionReadBack, String>,
{
    let receipt_ids = request
        .sources
        .iter()
        .flat_map(|source| source.receipt_ids.iter().cloned())
        .collect::<Vec<_>>();
    let receipt_claim = state.claim_codex_provider_set_probe_receipts(&receipt_ids)?;
    let (prepared, preview) = prepare_codex_provider_set_batch_internal(
        state,
        PrepareCodexProviderSetBatchRequest {
            sources: request.sources,
            router: request.router,
        },
        now,
    )?;
    if preview.digest != request.digest {
        return Err("codex_provider_set_dependency_changed".to_string());
    }
    if preview.blocked {
        return Err("codex_provider_set_batch_blocked".to_string());
    }
    if request.intent != CodexProviderSetCommitIntent::AcceptAuto {
        return Err("codex_provider_set_batch_intent_required".to_string());
    }

    let router = prepared.router.clone();
    let projection_router_ids = prepared.projection_router_ids.clone();
    let (_, observations) =
        protocol_state_from_provider_set_receipt_bundles(receipt_claim.bundles());
    let mut transaction = prepared
        .transaction
        .ok_or_else(|| "codex_provider_set_batch_blocked".to_string())?;
    transaction.observations = observations;
    state
        .db
        .apply_provider_set_database_transaction(transaction)
        .map_err(|error| error.to_string())?;
    receipt_claim.consume_after_database_commit();
    let (projections, status, projection_error_code) =
        classify_codex_provider_set_projection_commit(
            crate::codex_multirouter::mutation::finalize_codex_provider_set_projections_with_publisher(
                state.db.as_ref(),
                projection_router_ids,
                publish,
            ),
        );
    let source_snapshots = preview
        .source_previews
        .iter()
        .map(|source| {
            crate::commands::provider::get_codex_provider_editor_snapshot_internal(
                state,
                &source.source_provider_id,
                now,
            )
            .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CodexProviderSetBatchCommitOutcome {
        preview,
        router,
        source_snapshots,
        projections,
        status,
        projection_error_code,
    })
}

#[tauri::command]
pub fn prepare_codex_provider_set(
    state: State<'_, AppState>,
    request: PrepareCodexProviderSetRequest,
) -> Result<CodexProviderSetPreview, String> {
    prepare_codex_provider_set_internal(state.inner(), request, chrono::Utc::now().timestamp())
}

#[tauri::command]
pub fn commit_codex_provider_set(
    state: State<'_, AppState>,
    request: CommitCodexProviderSetRequest,
) -> Result<CodexProviderSetCommitOutcome, String> {
    let mut outcome = commit_codex_provider_set_internal_with_publisher(
        state.inner(),
        request,
        chrono::Utc::now().timestamp(),
        |artifact| {
            crate::codex_config::publish_codex_multirouter_projection_for_database(
                state.db.as_ref(),
                &artifact.projection_settings,
            )
            .map_err(|error| error.to_string())
        },
    )?;
    let source_is_current = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some(outcome.preview.source_provider_id.as_str());
    if source_is_current {
        let local_result = crate::settings::set_current_provider(
            &AppType::Codex,
            Some(outcome.preview.source_provider_id.as_str()),
        );
        let live_result = if matches!(outcome.preview.plan, CodexProviderSetPlan::Single { .. }) {
            ProviderService::sync_current_provider_for_app(state.inner(), AppType::Codex)
        } else {
            Ok(())
        };
        if local_result.is_err() || live_result.is_err() {
            outcome.status = CodexProviderSetCommitStatus::CommittedWithProjectionError;
            outcome.projection_error_code =
                Some("codex_provider_set_live_projection_failed".to_string());
        }
    }
    Ok(outcome)
}

pub(crate) fn prepare_codex_provider_set_internal(
    state: &AppState,
    request: PrepareCodexProviderSetRequest,
    now: i64,
) -> Result<CodexProviderSetPreview, String> {
    let provider =
        ProviderService::prepare_provider_for_mutation(state, &AppType::Codex, request.provider)
            .map_err(|error| error.to_string())?;
    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashMap<_, _>>();
    if provider.uses_manual_codex_protocol() {
        let records = if request.receipt_ids.is_empty() {
            Vec::new()
        } else {
            let (records, _) = load_codex_provider_set_probe_receipts(state, &request.receipt_ids)?;
            validate_codex_provider_set_probe_records(&provider, &records)?;
            records
        };
        return if provider.has_codex_protocol_overrides() {
            plan_codex_provider_set(&provider, &records, &providers, now)
        } else {
            let transport = manual_codex_provider_transport(&provider)?;
            plan_manual_codex_provider_set(&provider, transport, &providers, now)
        }
        .map(|prepared| prepared.preview)
        .map_err(|error| error.to_string());
    }
    if request.receipt_ids.is_empty() {
        return Err("codex_provider_set_probe_required".to_string());
    }
    let (records, _) = load_codex_provider_set_probe_receipts(state, &request.receipt_ids)?;
    validate_codex_provider_set_probe_records(&provider, &records)?;
    plan_codex_provider_set(&provider, &records, &providers, now)
        .map(|prepared| prepared.preview)
        .map_err(|error| error.to_string())
}

pub(crate) fn commit_codex_provider_set_internal_with_publisher<F>(
    state: &AppState,
    request: CommitCodexProviderSetRequest,
    now: i64,
    publish: F,
) -> Result<CodexProviderSetCommitOutcome, String>
where
    F: FnMut(
        &crate::codex_multirouter::projection::CodexRoutingProjectionArtifact,
    ) -> Result<crate::codex_multirouter::projection::ProjectionReadBack, String>,
{
    let receipt_claim = state.claim_codex_provider_set_probe_receipts(&request.receipt_ids)?;
    let (claimed_records, observations) =
        protocol_state_from_provider_set_receipt_bundles(receipt_claim.bundles());
    let provider =
        ProviderService::prepare_provider_for_mutation(state, &AppType::Codex, request.provider)
            .map_err(|error| error.to_string())?;
    let existing = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let is_manual = provider.uses_manual_codex_protocol();
    let records = if is_manual {
        if request.receipt_ids.is_empty() {
            Vec::new()
        } else {
            validate_codex_provider_set_probe_records(&provider, &claimed_records)?;
            claimed_records
        }
    } else {
        if request.receipt_ids.is_empty() {
            return Err("codex_provider_set_probe_required".to_string());
        }
        validate_codex_provider_set_probe_records(&provider, &claimed_records)?;
        claimed_records
    };
    let prepared = if is_manual && provider.has_codex_protocol_overrides() {
        plan_codex_provider_set(&provider, &records, &existing, now)
    } else if is_manual {
        let transport = manual_codex_provider_transport(&provider)?;
        plan_manual_codex_provider_set(&provider, transport, &existing, now)
    } else {
        plan_codex_provider_set(&provider, &records, &existing, now)
    }
    .map_err(|error| error.to_string())?;
    if prepared.preview.digest != request.digest {
        return Err("codex_provider_set_dependency_changed".to_string());
    }
    match (is_manual, &prepared.preview.plan, request.intent) {
        (
            true,
            CodexProviderSetPlan::Single { .. } | CodexProviderSetPlan::Split { .. },
            CodexProviderSetCommitIntent::ConfirmManual,
        ) => {}
        (true, _, _) => return Err("codex_provider_set_manual_intent_required".to_string()),
        (false, CodexProviderSetPlan::Single { .. }, CodexProviderSetCommitIntent::AcceptAuto)
        | (false, CodexProviderSetPlan::Split { .. }, CodexProviderSetCommitIntent::AcceptAuto) => {
        }
        (false, CodexProviderSetPlan::Single { .. } | CodexProviderSetPlan::Split { .. }, _) => {
            return Err("codex_provider_set_auto_intent_required".to_string())
        }
        (false, CodexProviderSetPlan::Blocked { .. }, _) => {
            return Err("codex_provider_set_model_blocked".to_string())
        }
    }
    let preview = prepared.preview.clone();
    let mut commit = crate::codex_multirouter::mutation::prepare_codex_provider_set_commit(
        state.db.as_ref(),
        prepared,
    )
    .map_err(|error| error.to_string())?;
    commit.transaction.observations = observations;
    state
        .db
        .apply_provider_set_database_transaction(commit.transaction)
        .map_err(|error| error.to_string())?;
    receipt_claim.consume_after_database_commit();
    let (projections, status, projection_error_code) =
        classify_codex_provider_set_projection_commit(
            crate::codex_multirouter::mutation::finalize_codex_provider_set_projections_with_publisher(
                state.db.as_ref(),
                commit.projection_router_ids,
                publish,
            ),
        );
    let snapshot = crate::commands::provider::get_codex_provider_editor_snapshot_internal(
        state,
        &preview.source_provider_id,
        now,
    )
    .map_err(|error| error.to_string())?;
    Ok(CodexProviderSetCommitOutcome {
        preview,
        snapshot,
        projections,
        status,
        projection_error_code,
    })
}

fn manual_codex_provider_transport(provider: &Provider) -> Result<TransportKind, String> {
    let api_format = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .and_then(Value::as_str)
        });
    match api_format {
        Some("openai_responses") => Ok(TransportKind::OpenAiResponses),
        Some("openai_chat") => Ok(TransportKind::OpenAiChat),
        _ => Err("codex_provider_set_manual_intent_required".to_string()),
    }
}

fn validate_codex_provider_set_probe_records(
    provider: &Provider,
    records: &[ProtocolCompatibilityRecord],
) -> Result<(), String> {
    let candidates = compile_provider_probe_candidates(provider)?;
    for record in records {
        let matches = candidates
            .iter()
            .filter(|candidate| {
                candidate.public_model == record.target.public_model
                    && candidate.upstream_model == record.target.upstream_model
            })
            .collect::<Vec<_>>();
        let [candidate] = matches.as_slice() else {
            return Err("codex_provider_set_probe_target_mismatch".to_string());
        };
        let expected = candidate.target_key(record.target.transport)?;
        if expected != record.target {
            return Err("codex_provider_set_probe_target_mismatch".to_string());
        }
    }
    Ok(())
}

pub(crate) fn load_codex_provider_set_probe_receipts(
    state: &AppState,
    receipt_ids: &[String],
) -> Result<
    (
        Vec<ProtocolCompatibilityRecord>,
        Vec<ProtocolCompatibilityRecord>,
    ),
    String,
> {
    let bundles = state.get_codex_provider_set_probe_receipts(receipt_ids)?;
    Ok(protocol_state_from_provider_set_receipt_bundles(&bundles))
}

pub(crate) fn protocol_state_from_provider_set_receipt_bundles(
    bundles: &[crate::store::CodexProviderSetProbeReceiptBundle],
) -> (
    Vec<ProtocolCompatibilityRecord>,
    Vec<ProtocolCompatibilityRecord>,
) {
    let mut records = Vec::with_capacity(bundles.len());
    let mut observations = Vec::with_capacity(bundles.len() * 2);
    for bundle in bundles {
        records.push(bundle.record.clone());
        observations.extend(bundle.observations.clone());
    }
    (records, observations)
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
    scope: Option<CodexProtocolProbeScope>,
) -> Result<CodexProviderProtocolPreflightOutcome, String> {
    run_provider_preflight(
        state.inner(),
        provider,
        scope.unwrap_or_default(),
        move |event| {
            let _ = on_event.send(event);
        },
    )
    .await
}

#[tauri::command]
pub async fn preflight_universal_codex_protocol_compatibility(
    state: State<'_, AppState>,
    provider: UniversalProvider,
    on_event: Channel<ProtocolProbeProgressEvent>,
) -> Result<Option<CodexProviderProtocolPreflightOutcome>, String> {
    let Some(codex_provider) =
        ProviderService::prepare_universal_codex_provider_from_definition(state.inner(), &provider)
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    run_provider_preflight(
        state.inner(),
        codex_provider,
        CodexProtocolProbeScope::AutomaticModels,
        move |event| {
            let _ = on_event.send(event);
        },
    )
    .await
    .map(Some)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_codex_provider_with_protocol_preflight(
    state: State<'_, AppState>,
    provider: Provider,
    originalId: Option<String>,
    addToLive: Option<bool>,
) -> Result<CodexProviderProtocolSaveOutcome, String> {
    let (provider, records, observations, protocol_applied, probe_error) =
        match automatic_codex_provider_preflight(state.inner(), provider.clone()).await {
            Ok((provider, records, observations)) => (provider, records, observations, false, None),
            Err(error) => (provider, Vec::new(), Vec::new(), false, Some(error)),
        };
    let record = records.first().cloned();

    if let Some(original_id) = originalId.as_deref() {
        ProviderService::update_with_protocol_state(
            state.inner(),
            AppType::Codex,
            Some(original_id),
            provider.clone(),
            &records,
            &observations,
        )
    } else {
        ProviderService::add_with_protocol_state(
            state.inner(),
            AppType::Codex,
            provider.clone(),
            addToLive.unwrap_or(true),
            &records,
            &observations,
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
    provider: Provider,
    scope: CodexProtocolProbeScope,
    reporter: F,
) -> Result<CodexProviderProtocolPreflightOutcome, String>
where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let (candidates, _is_router) = compile_preflight_candidates(&provider, &providers, scope)?;
    if scope == CodexProtocolProbeScope::AllEnabledModels && candidates.is_empty() {
        return Err("codex_protocol_probe_no_enabled_models".to_string());
    }
    let total = candidates.len();
    let batch = run_candidate_batch_with_observations(candidates, |candidate| {
        run_explicit_candidate_result_with(state, candidate, |candidate| {
            run_candidate_result_with_reporter(state, candidate, &reporter)
        })
    })
    .await?;
    let records = batch.records;
    let receipt_ids = remember_candidate_batch_receipts(state, &records, &batch.observations)?;
    let adaptation_preview = crate::commands::provider::build_codex_provider_adaptation_preview(
        &provider,
        &records,
        &batch.observations,
        &providers,
        chrono::Utc::now().timestamp(),
    )
    .map_err(|error| error.to_string())?;
    reporter(batch_finished_event(total, &records));
    Ok(CodexProviderProtocolPreflightOutcome {
        provider,
        adaptation_preview,
        records,
        observations: batch.observations,
        receipt_ids,
        protocol_applied: false,
    })
}

fn remember_candidate_batch_receipts(
    state: &AppState,
    records: &[ProtocolCompatibilityRecord],
    observations: &[ProtocolCompatibilityRecord],
) -> Result<Vec<String>, String> {
    if observations.len() != records.len() * 2 {
        return Err("codex_provider_set_probe_observation_mismatch".to_string());
    }
    records
        .iter()
        .cloned()
        .zip(observations.chunks_exact(2))
        .map(|(record, observations)| {
            state.remember_codex_provider_set_probe_receipt(record, observations.to_vec())
        })
        .collect()
}

pub(crate) async fn automatic_codex_provider_preflight(
    state: &AppState,
    provider: Provider,
) -> Result<
    (
        Provider,
        Vec<ProtocolCompatibilityRecord>,
        Vec<ProtocolCompatibilityRecord>,
    ),
    String,
> {
    if provider.uses_fixed_codex_responses_transport() {
        return Ok((provider, Vec::new(), Vec::new()));
    }

    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashMap<_, _>>();
    let (candidates, _is_router) = compile_preflight_candidates(
        &provider,
        &providers,
        CodexProtocolProbeScope::AutomaticModels,
    )?;
    if candidates.is_empty() {
        return Ok((provider, Vec::new(), Vec::new()));
    }
    let batch = run_candidate_batch_with_observations(candidates, |candidate| {
        run_automatic_candidate_result(state, candidate)
    })
    .await?;
    Ok((provider, batch.records, batch.observations))
}

fn compile_preflight_candidates(
    provider: &Provider,
    providers: &HashMap<String, Provider>,
    scope: CodexProtocolProbeScope,
) -> Result<(Vec<ProbeCandidate>, bool), String> {
    if provider.settings_config.get("codexRouting").is_some() {
        return compile_codex_router_probe_candidates(provider, providers)
            .map(|candidates| (candidates, true));
    }
    if scope == CodexProtocolProbeScope::AllEnabledModels {
        return compile_provider_probe_candidates(provider).map(|candidates| (candidates, false));
    }
    if provider.uses_manual_codex_protocol() && !provider.has_codex_protocol_overrides() {
        return Ok((Vec::new(), false));
    }
    compile_provider_probe_candidates(provider).map(|candidates| {
        (
            candidates
                .into_iter()
                .filter(|candidate| {
                    crate::codex_multirouter::provider_set::codex_model_follows_automatic_protocol(
                        provider,
                        &candidate.public_model,
                    )
                })
                .collect(),
            false,
        )
    })
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
    let observation_candidate = candidate.clone();
    let record = run_candidate(state, candidate).await?;
    let observations =
        build_observation_records_for_result(&observation_candidate, &record.result)?;
    state
        .db
        .save_protocol_probe_bundle(&record, &observations)
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
    run_automatic_candidate_result_with(state, candidate, |candidate| {
        run_candidate_result(state, candidate)
    })
    .await
}

async fn run_explicit_candidate_result_with<F, Fut>(
    state: &AppState,
    candidate: ProbeCandidate,
    execute: F,
) -> Result<ProtocolCompatibilityProbeResult, String>
where
    F: FnOnce(ProbeCandidate) -> Fut,
    Fut: Future<Output = Result<ProtocolCompatibilityProbeResult, String>>,
{
    let receipt_key = candidate.lease_key();
    let result = execute(candidate).await?;
    state.remember_protocol_probe_receipt(receipt_key, result.clone())?;
    Ok(result)
}

async fn run_automatic_candidate_result_with<F, Fut>(
    state: &AppState,
    candidate: ProbeCandidate,
    execute: F,
) -> Result<ProtocolCompatibilityProbeResult, String>
where
    F: FnOnce(ProbeCandidate) -> Fut,
    Fut: Future<Output = Result<ProtocolCompatibilityProbeResult, String>>,
{
    if let Some(result) = state.consume_protocol_probe_receipt(&candidate.lease_key())? {
        return Ok(result);
    }
    if let Some(result) = find_cached_candidate_result(state, &candidate, Utc::now().timestamp())? {
        return Ok(result);
    }
    execute(candidate).await
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
            || record.result.readiness != ProbeReadiness::Verified
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
    execute: F,
) -> Result<Vec<ProtocolCompatibilityRecord>, String>
where
    F: FnMut(ProbeCandidate) -> Fut,
    Fut: Future<Output = Result<ProtocolCompatibilityProbeResult, String>>,
{
    Ok(run_candidate_batch_with_observations(candidates, execute)
        .await?
        .records)
}

#[derive(Debug)]
struct CandidateBatchRecords {
    records: Vec<ProtocolCompatibilityRecord>,
    observations: Vec<ProtocolCompatibilityRecord>,
}

async fn run_candidate_batch_with_observations<F, Fut>(
    candidates: Vec<ProbeCandidate>,
    mut execute: F,
) -> Result<CandidateBatchRecords, String>
where
    F: FnMut(ProbeCandidate) -> Fut,
    Fut: Future<Output = Result<ProtocolCompatibilityProbeResult, String>>,
{
    let mut results_by_target: HashMap<String, ProtocolCompatibilityProbeResult> = HashMap::new();
    let mut records = Vec::with_capacity(candidates.len());
    let mut observations = Vec::with_capacity(candidates.len() * 2);
    for candidate in candidates {
        let execution_key = candidate.lease_key();
        let result = if let Some(result) = results_by_target.get(&execution_key) {
            result.clone()
        } else {
            let result = execute(candidate.clone()).await?;
            results_by_target.insert(execution_key, result.clone());
            result
        };
        observations.extend(build_observation_records_for_result(&candidate, &result)?);
        records.push(build_record_for_result(&candidate, result)?);
    }
    Ok(CandidateBatchRecords {
        records,
        observations,
    })
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

fn build_observation_records_for_result(
    candidate: &ProbeCandidate,
    result: &ProtocolCompatibilityProbeResult,
) -> Result<Vec<ProtocolCompatibilityRecord>, String> {
    let tested_at = Utc::now().timestamp();
    [TransportKind::OpenAiResponses, TransportKind::OpenAiChat]
        .into_iter()
        .map(|transport| {
            let branch = result
                .branches
                .iter()
                .find(|branch| branch.assessment.transport == transport)
                .cloned();
            let readiness = branch
                .as_ref()
                .map(|branch| {
                    if branch.assessment.is_complete() {
                        ProbeReadiness::Verified
                    } else if branch.assessment.baseline == ProbeStageStatus::Passed {
                        ProbeReadiness::Partial
                    } else {
                        ProbeReadiness::Unverified
                    }
                })
                .unwrap_or(ProbeReadiness::Unverified);
            let ttl = if readiness == ProbeReadiness::Verified {
                VERIFIED_TTL_SECONDS
            } else {
                UNVERIFIED_TTL_SECONDS
            };
            Ok(ProtocolCompatibilityRecord::new(
                target_for_candidate(candidate, transport)?,
                ProtocolCompatibilityProbeResult {
                    selected_transport: result.selected_transport,
                    readiness,
                    branches: branch.into_iter().collect(),
                },
                tested_at,
                tested_at + ttl,
            ))
        })
        .collect()
}

fn target_for_candidate(
    candidate: &ProbeCandidate,
    transport: TransportKind,
) -> Result<ProbeTargetKey, String> {
    candidate.target_key(transport)
}

#[tauri::command]
pub fn get_codex_protocol_compatibility(
    state: State<'_, AppState>,
    target: ProbeTargetKey,
) -> Result<ReasoningCompatibilityInspection, String> {
    inspect_reasoning_compatibility(state.inner(), target)
}

#[tauri::command]
pub fn list_codex_protocol_probe_observations(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<ProtocolCompatibilityRecord>, String> {
    let provider_id = required("providerId", &provider_id)?;
    ProviderService::list_codex_protocol_probe_observations(state.inner(), provider_id)
        .map_err(|error| error.to_string())
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
        apply_reasoning_override, batch_finished_event, build_observation_records_for_result,
        clear_reasoning_override, commit_codex_provider_set_batch_internal_with_publisher,
        commit_codex_provider_set_internal_with_publisher, compile_preflight_candidates,
        find_cached_candidate_result, inspect_reasoning_compatibility, plan_reasoning_override,
        prepare_codex_provider_set_batch_internal, prepare_codex_provider_set_internal,
        remember_candidate_batch_receipts, run_automatic_candidate_result_with,
        run_candidate_batch_with, run_candidate_batch_with_observations,
        run_explicit_candidate_result_with, ApplyReasoningOverrideRequest,
        ClearReasoningOverrideRequest, CodexProtocolProbeScope, CodexProviderSetBatchSourceRequest,
        CodexProviderSetCommitIntent, CodexProviderSetCommitStatus,
        CommitCodexProviderSetBatchRequest, CommitCodexProviderSetRequest,
        PlanReasoningOverrideRequest, PrepareCodexProviderSetBatchRequest,
        PrepareCodexProviderSetRequest,
    };
    use crate::protocol_compatibility::{
        HistoryReplay, ManualReasoningOverride, ProbeCandidate, ProbeReadiness, ProbeTargetKey,
        ProtocolCompatibilityProbeResult, ProtocolCompatibilityRecord, ProtocolProbeProgressEvent,
        ReasoningProjection, ReasoningSemantic, ReasoningSource, TransportKind,
    };
    use crate::provider::{CodexProtocolMode, CodexProtocolOverride, Provider, ProviderMeta};
    use crate::{database::Database, store::AppState};
    use serde_json::json;
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
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
                    {"model": "model-a", "upstreamModel": "model-a"},
                    {"model": "model-b", "upstreamModel": "model-b"}
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

    fn router_provider(target_provider_id: &str) -> Provider {
        Provider::with_id(
            "router-provider".to_string(),
            "Router Provider".to_string(),
            json!({
                "auth": {},
                "baseUrl": "http://127.0.0.1:15721/v1",
                "config": "model = \"model-a\"\nbase_url = \"http://127.0.0.1:15721/v1\"\nwire_api = \"responses\"\n",
                "codexRouting": {
                    "schemaVersion": 2,
                    "enabled": true,
                    "defaultRouteId": "target-route",
                    "routes": [{
                        "id": "target-route",
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

    fn provider_set_record(
        provider: &Provider,
        public_model: &str,
        upstream_model: &str,
        transport: TransportKind,
        tested_at: i64,
    ) -> ProtocolCompatibilityRecord {
        let target = crate::protocol_compatibility::compile_provider_probe_candidate_for_model(
            provider,
            public_model.to_string(),
            upstream_model.to_string(),
        )
        .expect("compile Provider Set candidate")
        .target_key(transport)
        .expect("provider set target");
        ProtocolCompatibilityRecord::new(
            target,
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(transport),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            tested_at,
            tested_at + 600,
        )
    }

    fn remember_provider_set_receipt(
        state: &AppState,
        record: ProtocolCompatibilityRecord,
    ) -> String {
        let observations = [TransportKind::OpenAiResponses, TransportKind::OpenAiChat]
            .into_iter()
            .map(|transport| {
                let mut observation = record.clone();
                observation.target.transport = transport;
                observation
            })
            .collect();
        state
            .remember_codex_provider_set_probe_receipt(record, observations)
            .expect("remember Provider Set receipt bundle")
    }

    #[test]
    fn provider_set_prepare_with_mixed_verified_receipts_is_zero_write() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let receipt_ids = [
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                100,
            ),
            provider_set_record(
                &provider,
                "model-b",
                "model-b",
                TransportKind::OpenAiChat,
                100,
            ),
        ]
        .into_iter()
        .map(|record| remember_provider_set_receipt(&state, record))
        .collect::<Vec<_>>();

        let preview = prepare_codex_provider_set_internal(
            &state,
            PrepareCodexProviderSetRequest {
                provider,
                receipt_ids,
            },
            150,
        )
        .expect("prepare mixed Provider Set");

        assert!(matches!(
            preview.plan,
            crate::codex_multirouter::provider_set::CodexProviderSetPlan::Split { .. }
        ));
        assert_eq!(preview.responses_models, vec!["model-a"]);
        assert_eq!(preview.chat_models, vec!["model-b"]);
        assert!(db
            .get_all_providers("codex")
            .expect("read providers")
            .is_empty());
        assert!(db
            .list_protocol_probe_observations("provider-a")
            .expect("read observations after prepare")
            .is_empty());
    }

    #[test]
    fn duplicate_logical_targets_keep_their_own_observation_pair() {
        let state = AppState::new(Arc::new(Database::memory().expect("memory database")));
        let provider = ordinary_provider();
        let first = provider_set_record(
            &provider,
            "model-a",
            "model-a",
            TransportKind::OpenAiResponses,
            100,
        );
        let mut second = first.clone();
        second.tested_at = 101;
        second.expires_at = 701;
        let observations_for = |record: &ProtocolCompatibilityRecord| {
            [TransportKind::OpenAiResponses, TransportKind::OpenAiChat]
                .into_iter()
                .map(|transport| {
                    let mut observation = record.clone();
                    observation.target.transport = transport;
                    observation
                })
                .collect::<Vec<_>>()
        };
        let records = vec![first.clone(), second.clone()];
        let observations = observations_for(&first)
            .into_iter()
            .chain(observations_for(&second))
            .collect::<Vec<_>>();

        let receipt_ids = remember_candidate_batch_receipts(&state, &records, &observations)
            .expect("remember duplicate logical target receipts");
        let bundles = state
            .get_codex_provider_set_probe_receipts(&receipt_ids)
            .expect("read duplicate logical target receipts");

        assert_eq!(bundles.len(), 2);
        assert_eq!(bundles[0].record.tested_at, 100);
        assert!(bundles[0]
            .observations
            .iter()
            .all(|observation| observation.tested_at == 100));
        assert_eq!(bundles[1].record.tested_at, 101);
        assert!(bundles[1]
            .observations
            .iter()
            .all(|observation| observation.tested_at == 101));
    }

    #[test]
    fn provider_set_prepare_rejects_receipt_bound_to_another_provider() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let mut foreign = provider_set_record(
            &provider,
            "model-a",
            "model-a",
            TransportKind::OpenAiResponses,
            100,
        );
        foreign.target.provider_id = "another-provider".to_string();
        let receipt_ids = vec![
            remember_provider_set_receipt(&state, foreign),
            remember_provider_set_receipt(
                &state,
                provider_set_record(
                    &provider,
                    "model-b",
                    "model-b",
                    TransportKind::OpenAiResponses,
                    100,
                ),
            ),
        ];

        let error = prepare_codex_provider_set_internal(
            &state,
            PrepareCodexProviderSetRequest {
                provider,
                receipt_ids,
            },
            150,
        )
        .expect_err("a receipt from another Provider must not authorize this draft");

        assert!(error.contains("codex_provider_set_probe_target_mismatch"));
        assert!(db
            .get_all_providers("codex")
            .expect("read providers")
            .is_empty());
        assert!(db
            .list_protocol_probe_observations("provider-a")
            .expect("read observations after rejected intent")
            .is_empty());
    }

    #[test]
    fn mixed_provider_set_accepts_the_automatic_plan_in_one_atomic_commit() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = [
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                now,
            ),
            provider_set_record(
                &provider,
                "model-b",
                "model-b",
                TransportKind::OpenAiChat,
                now,
            ),
        ]
        .into_iter()
        .map(|record| remember_provider_set_receipt(&state, record))
        .collect::<Vec<_>>();
        let preview = prepare_codex_provider_set_internal(
            &state,
            PrepareCodexProviderSetRequest {
                provider: provider.clone(),
                receipt_ids: receipt_ids.clone(),
            },
            now,
        )
        .expect("prepare mixed Provider Set");

        let outcome = commit_codex_provider_set_internal_with_publisher(
            &state,
            CommitCodexProviderSetRequest {
                provider,
                receipt_ids: receipt_ids.clone(),
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |artifact| {
                Ok(
                    crate::codex_multirouter::projection::ProjectionReadBack::verified(
                        artifact.dependency_fingerprint.clone(),
                    ),
                )
            },
        )
        .expect("commit the automatic Split plan without a second confirmation");

        assert_eq!(
            outcome.snapshot.adaptation.persistence,
            crate::commands::provider::CodexProviderPersistence::Split
        );
        assert_eq!(
            outcome.snapshot.adaptation.effective_transport,
            Some(crate::commands::provider::CodexEffectiveTransport::Mixed)
        );
        assert_eq!(outcome.snapshot.adaptation.models.len(), 2);

        let providers = db
            .get_all_providers("codex")
            .expect("read committed Provider Set");
        assert!(providers.iter().any(|(id, _)| id == "provider-a"));
        assert!(providers
            .iter()
            .any(|(id, _)| id == "provider-a--ccsm-responses"));
        assert!(providers
            .iter()
            .any(|(id, _)| id == "provider-a--ccsm-chat"));
        assert_eq!(
            db.get_current_provider("codex")
                .expect("read current Provider"),
            Some("provider-a".to_string())
        );
        assert_eq!(
            db.list_protocol_probe_observations("provider-a")
                .expect("read committed observations")
                .len(),
            4
        );
        let consumed = match state.get_codex_provider_set_probe_receipts(&receipt_ids) {
            Ok(_) => panic!("successful commit must consume every receipt"),
            Err(error) => error,
        };
        assert!(consumed.contains("codex_provider_set_probe_required"));
    }

    #[test]
    fn manual_per_model_override_prepares_a_split_with_receipts_only_for_auto_models() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db);
        let mut provider = ordinary_provider();
        let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
        meta.codex_protocol_mode = Some(crate::provider::CodexProtocolMode::Manual);
        meta.codex_protocol_overrides.insert(
            "model-b".to_string(),
            crate::provider::CodexProtocolOverride::OpenaiChat,
        );
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = vec![remember_provider_set_receipt(
            &state,
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                now,
            ),
        )];

        let preview = prepare_codex_provider_set_internal(
            &state,
            PrepareCodexProviderSetRequest {
                provider,
                receipt_ids,
            },
            now,
        )
        .expect("prepare mixed automatic/manual Provider Set");

        assert!(matches!(
            preview.plan,
            crate::codex_multirouter::provider_set::CodexProviderSetPlan::Split { .. }
        ));
        assert_eq!(preview.responses_models, vec!["model-a"]);
        assert_eq!(preview.chat_models, vec!["model-b"]);
    }

    #[test]
    fn wizard_batch_uses_the_same_per_model_manual_override_planner() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db);
        let mut provider = ordinary_provider();
        let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
        meta.codex_protocol_mode = Some(crate::provider::CodexProtocolMode::Manual);
        meta.codex_protocol_overrides.insert(
            "model-b".to_string(),
            crate::provider::CodexProtocolOverride::OpenaiChat,
        );
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = vec![remember_provider_set_receipt(
            &state,
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                now,
            ),
        )];

        let (_, preview) = prepare_codex_provider_set_batch_internal(
            &state,
            PrepareCodexProviderSetBatchRequest {
                sources: vec![CodexProviderSetBatchSourceRequest {
                    provider,
                    receipt_ids,
                }],
                router: router_provider("provider-a"),
            },
            now,
        )
        .expect("prepare wizard batch with mixed automatic/manual selection");

        assert!(matches!(
            preview.source_previews[0].plan,
            crate::codex_multirouter::provider_set::CodexProviderSetPlan::Split { .. }
        ));
        assert_eq!(preview.source_previews[0].responses_models, vec!["model-a"]);
        assert_eq!(preview.source_previews[0].chat_models, vec!["model-b"]);
    }

    #[test]
    fn provider_set_commit_rejects_receipts_claimed_by_another_commit() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = ["model-a", "model-b"]
            .into_iter()
            .map(|model| {
                remember_provider_set_receipt(
                    &state,
                    provider_set_record(
                        &provider,
                        model,
                        model,
                        TransportKind::OpenAiResponses,
                        now,
                    ),
                )
            })
            .collect::<Vec<_>>();
        let preview = prepare_codex_provider_set_internal(
            &state,
            PrepareCodexProviderSetRequest {
                provider: provider.clone(),
                receipt_ids: receipt_ids.clone(),
            },
            now,
        )
        .expect("prepare Provider Set");
        let competing_claim = state
            .claim_codex_provider_set_probe_receipts(&receipt_ids)
            .expect("simulate another commit holding the receipts");

        let conflict = match commit_codex_provider_set_internal_with_publisher(
            &state,
            CommitCodexProviderSetRequest {
                provider: provider.clone(),
                receipt_ids: receipt_ids.clone(),
                digest: preview.digest.clone(),
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |_| panic!("a conflicting commit must not publish"),
        ) {
            Ok(_) => panic!("a second commit must not reuse an in-flight receipt claim"),
            Err(error) => error,
        };
        assert!(conflict.contains("codex_provider_set_probe_receipt_in_use"));
        assert!(db
            .get_all_providers("codex")
            .expect("read Providers after conflict")
            .is_empty());

        drop(competing_claim);
        commit_codex_provider_set_internal_with_publisher(
            &state,
            CommitCodexProviderSetRequest {
                provider,
                receipt_ids,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |_| panic!("a Single Provider has no Router projection"),
        )
        .expect("released claim allows the original commit to retry");
    }

    #[test]
    fn provider_set_projection_failure_keeps_database_commit_and_consumes_receipts() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let router = router_provider(&provider.id);
        db.save_provider("codex", &provider)
            .expect("seed source Provider before its dependent Router");
        db.save_provider("codex", &router)
            .expect("seed dependent Router");
        db.set_current_provider("codex", &router.id)
            .expect("activate dependent Router");
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = [
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                now,
            ),
            provider_set_record(
                &provider,
                "model-b",
                "model-b",
                TransportKind::OpenAiChat,
                now,
            ),
        ]
        .into_iter()
        .map(|record| remember_provider_set_receipt(&state, record))
        .collect::<Vec<_>>();
        let preview = prepare_codex_provider_set_internal(
            &state,
            PrepareCodexProviderSetRequest {
                provider: provider.clone(),
                receipt_ids: receipt_ids.clone(),
            },
            now,
        )
        .expect("prepare Provider Set with dependent Router");

        let outcome = commit_codex_provider_set_internal_with_publisher(
            &state,
            CommitCodexProviderSetRequest {
                provider,
                receipt_ids: receipt_ids.clone(),
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |_| Err("injected projection failure".to_string()),
        )
        .expect("derived projection failure must not report the database commit as rolled back");

        assert_eq!(
            outcome.status,
            CodexProviderSetCommitStatus::CommittedWithProjectionError
        );
        assert_eq!(
            outcome.projection_error_code.as_deref(),
            Some("codex_provider_set_live_projection_failed")
        );
        assert!(db
            .get_provider_by_id("provider-a", "codex")
            .expect("read committed facade")
            .is_some());
        assert_eq!(
            db.list_protocol_probe_observations("provider-a")
                .expect("read committed observations")
                .len(),
            4
        );
        let consumed = match state.get_codex_provider_set_probe_receipts(&receipt_ids) {
            Ok(_) => panic!("committed database state must consume the receipt claim"),
            Err(error) => error,
        };
        assert!(consumed.contains("codex_provider_set_probe_required"));
    }

    #[test]
    fn provider_set_database_failure_rolls_back_observations_and_keeps_receipts_for_retry() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = [
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                now,
            ),
            provider_set_record(
                &provider,
                "model-b",
                "model-b",
                TransportKind::OpenAiResponses,
                now,
            ),
        ]
        .into_iter()
        .map(|record| remember_provider_set_receipt(&state, record))
        .collect::<Vec<_>>();
        let preview = prepare_codex_provider_set_internal(
            &state,
            PrepareCodexProviderSetRequest {
                provider: provider.clone(),
                receipt_ids: receipt_ids.clone(),
            },
            now,
        )
        .expect("prepare single-protocol Provider Set");
        {
            let conn = db.conn.lock().expect("lock database");
            conn.execute_batch(
                "CREATE TRIGGER fail_provider_set_commit
                 BEFORE INSERT ON providers
                 WHEN NEW.id = 'provider-a'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected Provider Set commit failure');
                 END;",
            )
            .expect("install Provider Set failure trigger");
        }

        let failed = commit_codex_provider_set_internal_with_publisher(
            &state,
            CommitCodexProviderSetRequest {
                provider: provider.clone(),
                receipt_ids: receipt_ids.clone(),
                digest: preview.digest.clone(),
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |_| panic!("a failed database transaction must not publish"),
        )
        .expect_err("injected database failure must abort Provider Set commit");

        assert!(failed.contains("injected Provider Set commit failure"));
        assert!(db
            .get_all_providers("codex")
            .expect("read Providers after failed commit")
            .is_empty());
        assert!(db
            .list_protocol_probe_observations("provider-a")
            .expect("read observations after failed commit")
            .is_empty());
        assert_eq!(
            state
                .get_codex_provider_set_probe_receipts(&receipt_ids)
                .expect("failed commit retains retry receipts")
                .len(),
            2
        );

        {
            let conn = db.conn.lock().expect("lock database");
            conn.execute_batch("DROP TRIGGER fail_provider_set_commit;")
                .expect("remove Provider Set failure trigger");
        }
        commit_codex_provider_set_internal_with_publisher(
            &state,
            CommitCodexProviderSetRequest {
                provider,
                receipt_ids: receipt_ids.clone(),
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |artifact| {
                Ok(
                    crate::codex_multirouter::projection::ProjectionReadBack::verified(
                        artifact.dependency_fingerprint.clone(),
                    ),
                )
            },
        )
        .expect("retry succeeds with the original receipts");

        assert_eq!(
            db.list_protocol_probe_observations("provider-a")
                .expect("read observations after retry")
                .len(),
            4
        );
        let consumed = match state.get_codex_provider_set_probe_receipts(&receipt_ids) {
            Ok(_) => panic!("successful retry must consume every receipt"),
            Err(error) => error,
        };
        assert!(consumed.contains("codex_provider_set_probe_required"));
    }

    #[test]
    fn wizard_batch_prepare_is_zero_write_and_commit_persists_sources_and_router_once() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = [
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                now,
            ),
            provider_set_record(
                &provider,
                "model-b",
                "model-b",
                TransportKind::OpenAiChat,
                now,
            ),
        ]
        .into_iter()
        .map(|record| remember_provider_set_receipt(&state, record))
        .collect::<Vec<_>>();
        let sources = vec![CodexProviderSetBatchSourceRequest {
            provider: provider.clone(),
            receipt_ids: receipt_ids.clone(),
        }];
        let router = router_provider("provider-a");
        let (_, preview) = prepare_codex_provider_set_batch_internal(
            &state,
            PrepareCodexProviderSetBatchRequest {
                sources: sources.clone(),
                router: router.clone(),
            },
            now,
        )
        .expect("prepare batch");

        assert!(db
            .get_all_providers("codex")
            .expect("read providers after prepare")
            .is_empty());
        assert!(db
            .list_protocol_probe_observations("provider-a")
            .expect("read wizard observations after prepare")
            .is_empty());

        let outcome = commit_codex_provider_set_batch_internal_with_publisher(
            &state,
            CommitCodexProviderSetBatchRequest {
                sources,
                router,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |artifact| {
                Ok(
                    crate::codex_multirouter::projection::ProjectionReadBack::verified(
                        artifact.dependency_fingerprint.clone(),
                    ),
                )
            },
        )
        .expect("commit batch");

        assert_eq!(outcome.router.id, "router-provider");
        assert_eq!(outcome.source_snapshots.len(), 1);
        assert_eq!(
            outcome.source_snapshots[0].adaptation.persistence,
            crate::commands::provider::CodexProviderPersistence::Split
        );
        let providers = db.get_all_providers("codex").expect("read committed batch");
        for expected in [
            "provider-a",
            "provider-a--ccsm-responses",
            "provider-a--ccsm-chat",
            "router-provider",
        ] {
            assert!(providers.contains_key(expected), "missing {expected}");
        }
        let routes = providers["router-provider"].settings_config["codexRouting"]["routes"]
            .as_array()
            .expect("routes");
        assert_eq!(routes.len(), 2);
        assert!(routes
            .iter()
            .all(|route| route["targetProviderId"] != "provider-a"));
        assert_eq!(
            db.list_protocol_probe_observations("provider-a")
                .expect("read wizard observations")
                .len(),
            4
        );
    }

    #[test]
    fn wizard_batch_projection_failure_keeps_database_commit_and_consumes_receipts() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let router = router_provider("provider-a");
        db.save_provider("codex", &provider)
            .expect("seed source Provider before editing the active Router");
        db.save_provider("codex", &router)
            .expect("seed active Router before the wizard batch edit");
        db.set_current_provider("codex", &router.id)
            .expect("activate Router before the wizard batch edit");
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = [
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                now,
            ),
            provider_set_record(
                &provider,
                "model-b",
                "model-b",
                TransportKind::OpenAiChat,
                now,
            ),
        ]
        .into_iter()
        .map(|record| remember_provider_set_receipt(&state, record))
        .collect::<Vec<_>>();
        let sources = vec![CodexProviderSetBatchSourceRequest {
            provider,
            receipt_ids: receipt_ids.clone(),
        }];
        let (_, preview) = prepare_codex_provider_set_batch_internal(
            &state,
            PrepareCodexProviderSetBatchRequest {
                sources: sources.clone(),
                router: router.clone(),
            },
            now,
        )
        .expect("prepare batch with an active Router projection");

        let outcome = commit_codex_provider_set_batch_internal_with_publisher(
            &state,
            CommitCodexProviderSetBatchRequest {
                sources,
                router,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |_| Err("injected projection failure".to_string()),
        )
        .expect("derived projection failure must not report the database commit as rolled back");

        assert_eq!(
            outcome.status,
            CodexProviderSetCommitStatus::CommittedWithProjectionError
        );
        assert_eq!(
            outcome.projection_error_code.as_deref(),
            Some("codex_provider_set_live_projection_failed")
        );
        assert!(db
            .get_provider_by_id("router-provider", "codex")
            .expect("read committed Router")
            .is_some());
        assert_eq!(
            db.list_protocol_probe_observations("provider-a")
                .expect("read committed wizard observations")
                .len(),
            4
        );
        let consumed = match state.get_codex_provider_set_probe_receipts(&receipt_ids) {
            Ok(_) => panic!("committed batch must consume every receipt"),
            Err(error) => error,
        };
        assert!(consumed.contains("codex_provider_set_probe_required"));
    }

    #[test]
    fn wizard_batch_commit_rejects_receipts_claimed_by_another_commit() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = ["model-a", "model-b"]
            .into_iter()
            .map(|model| {
                remember_provider_set_receipt(
                    &state,
                    provider_set_record(
                        &provider,
                        model,
                        model,
                        TransportKind::OpenAiResponses,
                        now,
                    ),
                )
            })
            .collect::<Vec<_>>();
        let sources = vec![CodexProviderSetBatchSourceRequest {
            provider,
            receipt_ids: receipt_ids.clone(),
        }];
        let router = router_provider("provider-a");
        let (_, preview) = prepare_codex_provider_set_batch_internal(
            &state,
            PrepareCodexProviderSetBatchRequest {
                sources: sources.clone(),
                router: router.clone(),
            },
            now,
        )
        .expect("prepare wizard batch");
        let competing_claim = state
            .claim_codex_provider_set_probe_receipts(&receipt_ids)
            .expect("simulate another wizard commit holding the receipts");

        let conflict = match commit_codex_provider_set_batch_internal_with_publisher(
            &state,
            CommitCodexProviderSetBatchRequest {
                sources: sources.clone(),
                router: router.clone(),
                digest: preview.digest.clone(),
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |_| panic!("a conflicting wizard commit must not publish"),
        ) {
            Ok(_) => panic!("a second wizard commit must not reuse in-flight receipts"),
            Err(error) => error,
        };
        assert!(conflict.contains("codex_provider_set_probe_receipt_in_use"));
        assert!(db
            .get_all_providers("codex")
            .expect("read Providers after wizard conflict")
            .is_empty());

        drop(competing_claim);
        commit_codex_provider_set_batch_internal_with_publisher(
            &state,
            CommitCodexProviderSetBatchRequest {
                sources,
                router,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |artifact| {
                Ok(
                    crate::codex_multirouter::projection::ProjectionReadBack::verified(
                        artifact.dependency_fingerprint.clone(),
                    ),
                )
            },
        )
        .expect("released claim allows the wizard commit to retry");
    }

    #[test]
    fn wizard_batch_commits_official_and_managed_sources_without_probe_receipts() {
        for (provider_id, configure) in [
            ("official-source", "official"),
            ("managed-source", "managed"),
        ] {
            let db = Arc::new(Database::memory().expect("memory database"));
            let state = AppState::new(db.clone());
            let mut provider = ordinary_provider();
            provider.id = provider_id.to_string();
            provider.name = provider_id.to_string();
            if configure == "official" {
                provider.category = Some("official".to_string());
            } else {
                provider
                    .meta
                    .get_or_insert_with(ProviderMeta::default)
                    .provider_type = Some("codex_oauth".to_string());
            }
            let mut router = router_provider(provider_id);
            router.id = format!("router-{provider_id}");
            let sources = vec![CodexProviderSetBatchSourceRequest {
                provider: provider.clone(),
                receipt_ids: Vec::new(),
            }];
            let now = chrono::Utc::now().timestamp();

            let (_, preview) = prepare_codex_provider_set_batch_internal(
                &state,
                PrepareCodexProviderSetBatchRequest {
                    sources: sources.clone(),
                    router: router.clone(),
                },
                now,
            )
            .expect("fixed Responses sources do not require probe receipts");

            assert!(!preview.blocked);
            assert!(matches!(
                preview.source_previews[0].plan,
                crate::codex_multirouter::provider_set::CodexProviderSetPlan::Single {
                    transport: TransportKind::OpenAiResponses
                }
            ));
            assert!(db
                .get_all_providers("codex")
                .expect("prepare is zero-write")
                .is_empty());

            commit_codex_provider_set_batch_internal_with_publisher(
                &state,
                CommitCodexProviderSetBatchRequest {
                    sources,
                    router: router.clone(),
                    digest: preview.digest,
                    intent: CodexProviderSetCommitIntent::AcceptAuto,
                },
                now,
                |artifact| {
                    Ok(
                        crate::codex_multirouter::projection::ProjectionReadBack::verified(
                            artifact.dependency_fingerprint.clone(),
                        ),
                    )
                },
            )
            .expect("commit source and router atomically");

            let saved = db
                .get_all_providers("codex")
                .expect("read committed Providers");
            assert!(saved.contains_key(provider_id));
            assert!(saved.contains_key(&router.id));
        }
    }

    #[test]
    fn wizard_batch_returns_structured_blocked_source_without_a_transaction() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = ordinary_provider();
        let now = chrono::Utc::now().timestamp();
        let verified = provider_set_record(
            &provider,
            "model-a",
            "model-a",
            TransportKind::OpenAiResponses,
            now,
        );
        let mut partial = provider_set_record(
            &provider,
            "model-b",
            "model-b",
            TransportKind::OpenAiChat,
            now,
        );
        partial.result.readiness = ProbeReadiness::Partial;
        let receipt_ids = [verified, partial]
            .into_iter()
            .map(|record| remember_provider_set_receipt(&state, record))
            .collect::<Vec<_>>();
        let router = router_provider("provider-a");

        let (prepared, preview) = prepare_codex_provider_set_batch_internal(
            &state,
            PrepareCodexProviderSetBatchRequest {
                sources: vec![CodexProviderSetBatchSourceRequest {
                    provider: provider.clone(),
                    receipt_ids: receipt_ids.clone(),
                }],
                router: router.clone(),
            },
            now,
        )
        .expect("blocked is a structured preview, not a partial commit");

        assert!(preview.blocked);
        assert!(prepared.transaction.is_none());
        assert!(matches!(
            preview.source_previews[0].plan,
            crate::codex_multirouter::provider_set::CodexProviderSetPlan::Blocked { .. }
        ));
        assert!(db
            .get_all_providers("codex")
            .expect("read Providers")
            .is_empty());
        assert!(db
            .list_protocol_probe_observations("provider-a")
            .expect("read observations after blocked prepare")
            .is_empty());

        let blocked = commit_codex_provider_set_batch_internal_with_publisher(
            &state,
            CommitCodexProviderSetBatchRequest {
                sources: vec![CodexProviderSetBatchSourceRequest {
                    provider,
                    receipt_ids: receipt_ids.clone(),
                }],
                router,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |_| panic!("blocked batch must not publish"),
        )
        .expect_err("blocked batch must not commit");
        assert!(blocked.contains("codex_provider_set_batch_blocked"));
        assert!(db
            .get_all_providers("codex")
            .expect("read Providers after blocked commit")
            .is_empty());
        assert!(db
            .list_protocol_probe_observations("provider-a")
            .expect("read observations after blocked commit")
            .is_empty());
        assert_eq!(
            state
                .get_codex_provider_set_probe_receipts(&receipt_ids)
                .expect("blocked or cancelled flow retains unconsumed receipts")
                .len(),
            2
        );
    }

    #[test]
    fn manual_provider_set_requires_explicit_whole_provider_intent() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut provider = ordinary_provider();
        provider
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .codex_protocol_mode = Some(crate::provider::CodexProtocolMode::Manual);
        for model in provider.settings_config["modelCatalog"]["models"]
            .as_array_mut()
            .expect("catalog models")
        {
            model
                .as_object_mut()
                .expect("catalog model")
                .remove("apiFormat");
        }
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = [
            provider_set_record(
                &provider,
                "model-a",
                "model-a",
                TransportKind::OpenAiResponses,
                now,
            ),
            provider_set_record(
                &provider,
                "model-b",
                "model-b",
                TransportKind::OpenAiResponses,
                now,
            ),
        ]
        .into_iter()
        .map(|record| remember_provider_set_receipt(&state, record))
        .collect::<Vec<_>>();
        let preview = prepare_codex_provider_set_internal(
            &state,
            PrepareCodexProviderSetRequest {
                provider: provider.clone(),
                receipt_ids: receipt_ids.clone(),
            },
            now,
        )
        .expect("prepare manual whole-Provider protocol");
        assert!(matches!(
            preview.plan,
            crate::codex_multirouter::provider_set::CodexProviderSetPlan::Single {
                transport: TransportKind::OpenAiResponses
            }
        ));

        let error = commit_codex_provider_set_internal_with_publisher(
            &state,
            CommitCodexProviderSetRequest {
                provider: provider.clone(),
                receipt_ids: receipt_ids.clone(),
                digest: preview.digest.clone(),
                intent: CodexProviderSetCommitIntent::AcceptAuto,
            },
            now,
            |_| panic!("wrong manual intent must not publish"),
        )
        .expect_err("manual mode requires explicit manual confirmation");
        assert!(
            error.contains("codex_provider_set_manual_intent_required"),
            "unexpected commit error: {error}"
        );
        assert!(db
            .get_all_providers("codex")
            .expect("read Providers")
            .is_empty());
        assert!(db
            .list_protocol_probe_observations("provider-a")
            .expect("read observations after rejected manual intent")
            .is_empty());

        commit_codex_provider_set_internal_with_publisher(
            &state,
            CommitCodexProviderSetRequest {
                provider,
                receipt_ids,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::ConfirmManual,
            },
            now,
            |_| panic!("a Single manual Provider has no Router projection"),
        )
        .expect("commit manual whole-Provider protocol");
        assert!(db
            .get_provider_by_id("provider-a", "codex")
            .expect("read Provider")
            .is_some());
        assert_eq!(
            db.list_protocol_probe_observations("provider-a")
                .expect("read observations after manual override")
                .len(),
            4
        );
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

    #[test]
    fn explicit_preflight_compiles_multirouter_routes_instead_of_the_local_router_endpoint() {
        let target = ordinary_provider();
        let router = router_provider(&target.id);
        let providers = HashMap::from([(target.id.clone(), target)]);

        let (candidates, is_router) = compile_preflight_candidates(
            &router,
            &providers,
            CodexProtocolProbeScope::AllEnabledModels,
        )
        .expect("compile routed preflight");

        assert!(is_router);
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|candidate| {
            candidate.provider_id.as_deref() == Some("router-provider")
                && candidate.route_id.as_deref() == Some("target-route")
                && candidate.canonical_endpoint() == "https://example.test/v1"
        }));
    }

    #[test]
    fn automatic_preflight_compiles_only_models_that_follow_automatic_protocol_selection() {
        let mut provider = ordinary_provider();
        let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
        meta.codex_protocol_mode = Some(CodexProtocolMode::Manual);
        meta.codex_protocol_overrides
            .insert("model-a".to_string(), CodexProtocolOverride::OpenaiChat);
        let original = provider.clone();

        let (candidates, is_router) = compile_preflight_candidates(
            &provider,
            &HashMap::new(),
            CodexProtocolProbeScope::AutomaticModels,
        )
        .expect("compile ordinary Provider preflight");

        assert!(!is_router);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.public_model.as_str())
                .collect::<Vec<_>>(),
            vec!["model-b"]
        );
        assert_eq!(
            serde_json::to_value(provider).expect("serialize Provider after compile"),
            serde_json::to_value(original).expect("serialize original Provider"),
            "preflight candidate compilation must not mutate the Provider draft"
        );
    }

    #[test]
    fn explicit_preflight_compiles_every_enabled_model_despite_manual_overrides() {
        let mut provider = ordinary_provider();
        let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
        meta.codex_protocol_mode = Some(CodexProtocolMode::Manual);
        meta.codex_protocol_overrides
            .insert("model-a".to_string(), CodexProtocolOverride::OpenaiChat);

        let (candidates, is_router) = compile_preflight_candidates(
            &provider,
            &HashMap::new(),
            CodexProtocolProbeScope::AllEnabledModels,
        )
        .expect("compile explicit Provider validation");

        assert!(!is_router);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.public_model.as_str())
                .collect::<Vec<_>>(),
            vec!["model-a", "model-b"]
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

    #[tokio::test]
    async fn batch_keeps_one_selection_receipt_and_two_transport_observations() {
        let candidate = alias_candidate("provider-a", "route-a", "qwen3.8");
        let batch = run_candidate_batch_with_observations(vec![candidate], |_| {
            std::future::ready(Ok(ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiResponses),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            }))
        })
        .await
        .expect("run batch");

        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.observations.len(), 2);
        assert_eq!(
            batch.records[0].result.selected_transport,
            Some(TransportKind::OpenAiResponses)
        );
        assert!(batch
            .observations
            .iter()
            .any(|record| record.target.transport == TransportKind::OpenAiResponses));
        assert!(batch
            .observations
            .iter()
            .any(|record| record.target.transport == TransportKind::OpenAiChat));
    }

    #[test]
    fn responses_recommendation_materializes_one_verified_observation_per_transport() {
        let candidate = alias_candidate("provider-a", "route-a", "qwen3.8");
        let result: ProtocolCompatibilityProbeResult = serde_json::from_value(json!({
            "selected_transport": "open_ai_responses",
            "readiness": "verified",
            "branches": [
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
                    "evidence": []
                },
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
                    "evidence": []
                }
            ]
        }))
        .expect("deserialize probe result");

        let observations = build_observation_records_for_result(&candidate, &result)
            .expect("materialize observations");

        assert_eq!(observations.len(), 2);
        for transport in [TransportKind::OpenAiResponses, TransportKind::OpenAiChat] {
            let observation = observations
                .iter()
                .find(|record| record.target.transport == transport)
                .expect("transport observation");
            assert_eq!(observation.target, candidate.target_key(transport).unwrap());
            assert_eq!(
                observation.result.selected_transport,
                result.selected_transport
            );
            assert_eq!(observation.result.readiness, ProbeReadiness::Verified);
            assert_eq!(observation.result.branches.len(), 1);
            assert_eq!(
                observation.result.branches[0].assessment.transport,
                transport
            );
        }
    }

    #[test]
    fn partial_chat_branch_is_saved_without_downgrading_responses_observation() {
        let candidate = alias_candidate("provider-a", "route-a", "qwen3.8");
        let result: ProtocolCompatibilityProbeResult = serde_json::from_value(json!({
            "selected_transport": "open_ai_responses",
            "readiness": "verified",
            "branches": [
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
                    "evidence": []
                },
                {
                    "assessment": {
                        "transport": "open_ai_chat",
                        "baseline": "passed",
                        "streaming": "failed",
                        "forced_tool": "passed",
                        "continuation": "unsupported"
                    },
                    "reasoning_shape": {
                        "semantic": "readable",
                        "source": "reasoning_content",
                        "pre_tool_visible_content": "absent"
                    },
                    "evidence": []
                }
            ]
        }))
        .expect("deserialize probe result");

        let observations = build_observation_records_for_result(&candidate, &result)
            .expect("materialize observations");
        let responses = observations
            .iter()
            .find(|record| record.target.transport == TransportKind::OpenAiResponses)
            .unwrap();
        let chat = observations
            .iter()
            .find(|record| record.target.transport == TransportKind::OpenAiChat)
            .unwrap();

        assert_eq!(responses.result.readiness, ProbeReadiness::Verified);
        assert_eq!(chat.result.readiness, ProbeReadiness::Partial);
        assert!(responses.expires_at > chat.expires_at);
        assert_eq!(
            responses.result.selected_transport,
            chat.result.selected_transport
        );
    }

    #[test]
    fn automatic_probe_cache_reuses_only_unexpired_current_version_evidence() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let candidate = alias_candidate("router", "qwen", "qwen3.8");
        let result = ProtocolCompatibilityProbeResult {
            selected_transport: Some(TransportKind::OpenAiChat),
            readiness: ProbeReadiness::Verified,
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

    #[tokio::test]
    async fn explicit_preflight_receipt_is_consumed_by_save_after_provider_id_changes() {
        let state = AppState::new(Arc::new(Database::memory().expect("memory database")));
        let draft = alias_candidate("codex-draft", "draft", "qwen3.8");
        let persisted = alias_candidate("real-provider", "saved-route", "qwen3.8");
        assert_eq!(draft.lease_key(), persisted.lease_key());

        let expected = ProtocolCompatibilityProbeResult {
            selected_transport: Some(TransportKind::OpenAiResponses),
            readiness: ProbeReadiness::Verified,
            branches: Vec::new(),
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let explicit_calls = calls.clone();
        let explicit_result = expected.clone();
        let observed = run_explicit_candidate_result_with(&state, draft, move |_| {
            explicit_calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(explicit_result.clone()))
        })
        .await
        .expect("explicit preflight");
        assert_eq!(observed, expected);

        let automatic_calls = calls.clone();
        let fallback_result = ProtocolCompatibilityProbeResult {
            selected_transport: Some(TransportKind::OpenAiChat),
            readiness: ProbeReadiness::Partial,
            branches: Vec::new(),
        };
        let reused = run_automatic_candidate_result_with(&state, persisted.clone(), move |_| {
            automatic_calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(fallback_result.clone()))
        })
        .await
        .expect("save reuses explicit preflight");
        assert_eq!(reused, expected);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let second_save_calls = calls.clone();
        let second_save_result = ProtocolCompatibilityProbeResult {
            selected_transport: Some(TransportKind::OpenAiChat),
            readiness: ProbeReadiness::Partial,
            branches: Vec::new(),
        };
        let after_consumption = run_automatic_candidate_result_with(&state, persisted, move |_| {
            second_save_calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(second_save_result.clone()))
        })
        .await
        .expect("one-shot receipt is consumed");
        assert_eq!(
            after_consumption.selected_transport,
            Some(TransportKind::OpenAiChat)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
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
