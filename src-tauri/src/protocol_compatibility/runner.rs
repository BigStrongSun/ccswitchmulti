use std::{collections::BTreeMap, fmt, time::Duration};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::proxy::providers::codex_chat_common::split_leading_think_block;
use crate::proxy::providers::codex_request::CodexRequestOptions;
use crate::proxy::providers::codex_terminal::{
    classify_chat_terminal, classify_native_responses_terminal,
    streamed_tool_arguments_are_complete, ChatTerminalEvidence, NativeResponsesEvidence,
    NativeResponsesTerminalDisposition, TerminalDisposition,
};

use super::{
    build_logical_probe_request,
    capture::{capture_transport_probe, CapturedProbeExchange, ProbeCaptureError},
    classify::ClassifiedReasoningShape,
    classify_captured_reasoning_shape,
    redaction::RedactedProbeEvidence,
    selection::select_transport_outcome_with_reasoning,
    HistoryReplay, PreToolVisibleContent, ProbeCandidate, ProbeCase, ProbeReadiness,
    ProbeStageStatus, ReasoningSemantic, ReasoningSource, ToolSchemaDialect, TransportKind,
    TransportProbeAssessment,
};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeProgressStage {
    Baseline,
    Streaming,
    Reasoning,
    ForcedTool,
    Continuation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeFailureKind {
    HttpStatus,
    Timeout,
    Network,
    ResponseTooLarge,
    InvalidResponse,
    InvalidRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedProbeFailure {
    pub stage: ProbeProgressStage,
    pub kind: ProbeFailureKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
}

impl RedactedProbeFailure {
    fn from_capture(stage: ProbeProgressStage, error: ProbeCaptureError) -> Self {
        let (kind, status_code) = match error {
            ProbeCaptureError::Timeout => (ProbeFailureKind::Timeout, None),
            ProbeCaptureError::Network => (ProbeFailureKind::Network, None),
            ProbeCaptureError::HttpStatus { status_code }
            | ProbeCaptureError::ToolSchemaRejected { status_code }
            | ProbeCaptureError::ReasoningReplayRejected { status_code } => {
                (ProbeFailureKind::HttpStatus, Some(status_code))
            }
            ProbeCaptureError::ResponseTooLarge => (ProbeFailureKind::ResponseTooLarge, None),
            ProbeCaptureError::InvalidPayload => (ProbeFailureKind::InvalidResponse, None),
        };
        Self {
            stage,
            kind,
            status_code,
        }
    }

    fn invalid_response(stage: ProbeProgressStage) -> Self {
        Self {
            stage,
            kind: ProbeFailureKind::InvalidResponse,
            status_code: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProtocolProbeProgressEvent {
    CandidateStarted {
        model: String,
    },
    StageStarted {
        model: String,
        transport: TransportKind,
        stage: ProbeProgressStage,
    },
    StageFinished {
        model: String,
        transport: TransportKind,
        stage: ProbeProgressStage,
        stage_status: ProbeStageStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        failure: Option<RedactedProbeFailure>,
    },
    ReasoningClassified {
        model: String,
        transport: TransportKind,
        stage: ProbeProgressStage,
        reasoning_semantic: ReasoningSemantic,
        reasoning_source: ReasoningSource,
    },
    BranchFinished {
        model: String,
        transport: TransportKind,
        readiness: ProbeReadiness,
    },
    CandidateFinished {
        model: String,
        selected_transport: Option<TransportKind>,
        readiness: ProbeReadiness,
    },
    BatchFinished {
        total: usize,
        verified: usize,
        partial: usize,
        failed: usize,
    },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportBranchResult {
    pub assessment: TransportProbeAssessment,
    pub reasoning_shape: ClassifiedReasoningShape,
    #[serde(default)]
    pub tool_schema_dialect: ToolSchemaDialect,
    #[serde(default)]
    pub history_replay: HistoryReplay,
    evidence: Vec<RedactedProbeEvidence>,
    #[serde(default)]
    pub failures: Vec<RedactedProbeFailure>,
}

impl fmt::Debug for TransportBranchResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransportBranchResult")
            .field("assessment", &self.assessment)
            .field("reasoning_shape", &self.reasoning_shape)
            .field("tool_schema_dialect", &self.tool_schema_dialect)
            .field("history_replay", &self.history_replay)
            .field("evidence_count", &self.evidence.len())
            .field("failures", &self.failures)
            .finish()
    }
}

impl TransportBranchResult {
    pub(crate) fn evidence(&self) -> &[RedactedProbeEvidence] {
        &self.evidence
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolCompatibilityProbeResult {
    pub selected_transport: Option<TransportKind>,
    pub readiness: ProbeReadiness,
    pub branches: Vec<TransportBranchResult>,
}

#[derive(Debug)]
struct CapturedToolCall {
    call_id: String,
    name: String,
    arguments: String,
    reasoning_content: String,
}

pub async fn run_protocol_compatibility_probe(
    candidate: ProbeCandidate,
    client: &Client,
) -> ProtocolCompatibilityProbeResult {
    run_protocol_compatibility_probe_with_reporter(candidate, client, |_| {}).await
}

pub async fn run_protocol_compatibility_probe_with_reporter<F>(
    candidate: ProbeCandidate,
    client: &Client,
    reporter: F,
) -> ProtocolCompatibilityProbeResult
where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    let model = candidate.public_model.clone();
    match tokio::time::timeout(TRANSACTION_TIMEOUT, run_probe(candidate, client, &reporter)).await {
        Ok(result) => result,
        Err(_) => {
            reporter(ProtocolProbeProgressEvent::CandidateFinished {
                model,
                selected_transport: None,
                readiness: ProbeReadiness::Unverified,
            });
            ProtocolCompatibilityProbeResult {
                selected_transport: None,
                readiness: ProbeReadiness::Unverified,
                branches: Vec::new(),
            }
        }
    }
}

async fn run_probe<F>(
    candidate: ProbeCandidate,
    client: &Client,
    reporter: &F,
) -> ProtocolCompatibilityProbeResult
where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    reporter(ProtocolProbeProgressEvent::CandidateStarted {
        model: candidate.public_model.clone(),
    });
    let nonce = Uuid::new_v4().simple().to_string();
    let mut branches = Vec::with_capacity(2);

    for transport in [TransportKind::OpenAiResponses, TransportKind::OpenAiChat] {
        branches.push(run_branch(&candidate, client, transport, &nonce, reporter).await);
    }

    let candidates = branches
        .iter()
        .map(|branch| (branch.assessment, branch.reasoning_shape.semantic))
        .collect::<Vec<_>>();
    let selection = select_transport_outcome_with_reasoning(&candidates);
    let result = ProtocolCompatibilityProbeResult {
        selected_transport: selection.map(|selected| selected.transport),
        readiness: selection
            .map(|selected| selected.readiness)
            .unwrap_or(ProbeReadiness::Unverified),
        branches,
    };
    reporter(ProtocolProbeProgressEvent::CandidateFinished {
        model: candidate.public_model,
        selected_transport: result.selected_transport,
        readiness: result.readiness,
    });
    result
}

async fn run_branch<F>(
    candidate: &ProbeCandidate,
    client: &Client,
    transport: TransportKind,
    nonce: &str,
    reporter: &F,
) -> TransportBranchResult
where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    let mut assessment = TransportProbeAssessment {
        transport,
        baseline: ProbeStageStatus::Skipped,
        streaming: ProbeStageStatus::Skipped,
        forced_tool: ProbeStageStatus::Skipped,
        continuation: ProbeStageStatus::Skipped,
    };
    let mut evidence = Vec::new();
    let mut failures = Vec::new();
    let mut reasoning_shape = empty_reasoning_shape();
    let mut tool_schema_dialect = ToolSchemaDialect::OpenAi;
    let mut history_replay = match transport {
        TransportKind::OpenAiChat => HistoryReplay::ChatReasoningContent,
        TransportKind::OpenAiResponses => HistoryReplay::NativeOnly,
    };
    report_stage_started(reporter, candidate, transport, ProbeProgressStage::Baseline);
    let baseline = send_case(
        candidate,
        client,
        transport,
        ProbeCase::BaselineJson,
        nonce,
        None,
        CodexRequestOptions::default(),
    )
    .await;
    let baseline_exchange = match baseline {
        Ok(exchange) if has_completed_assistant_turn(transport, &exchange) => {
            assessment.baseline = ProbeStageStatus::Passed;
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::Baseline,
                assessment.baseline,
                None,
            );
            exchange
        }
        Ok(_) => {
            assessment.baseline = ProbeStageStatus::Failed;
            let failure = RedactedProbeFailure::invalid_response(ProbeProgressStage::Baseline);
            failures.push(failure.clone());
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::Baseline,
                assessment.baseline,
                Some(failure),
            );
            return finish_branch(
                reporter,
                candidate,
                TransportBranchResult {
                    assessment,
                    reasoning_shape,
                    tool_schema_dialect,
                    history_replay,
                    evidence,
                    failures,
                },
            );
        }
        Err(error) => {
            assessment.baseline = baseline_failure_status(error);
            let failure = RedactedProbeFailure::from_capture(ProbeProgressStage::Baseline, error);
            failures.push(failure.clone());
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::Baseline,
                assessment.baseline,
                Some(failure),
            );
            return finish_branch(
                reporter,
                candidate,
                TransportBranchResult {
                    assessment,
                    reasoning_shape,
                    tool_schema_dialect,
                    history_replay,
                    evidence,
                    failures,
                },
            );
        }
    };
    update_shape(
        &mut reasoning_shape,
        classify_captured_reasoning_shape(&baseline_exchange),
    );
    evidence.push(baseline_exchange.evidence().clone());

    report_stage_started(
        reporter,
        candidate,
        transport,
        ProbeProgressStage::Streaming,
    );
    match send_case(
        candidate,
        client,
        transport,
        ProbeCase::BaselineSse,
        nonce,
        None,
        CodexRequestOptions::default(),
    )
    .await
    {
        Ok(exchange) => {
            assessment.streaming = if has_completed_assistant_turn(transport, &exchange) {
                ProbeStageStatus::Passed
            } else {
                ProbeStageStatus::Failed
            };
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(&exchange),
            );
            evidence.push(exchange.evidence().clone());
            if assessment.streaming == ProbeStageStatus::Failed {
                failures.push(RedactedProbeFailure::invalid_response(
                    ProbeProgressStage::Streaming,
                ));
            }
        }
        Err(error) => {
            assessment.streaming = stage_failure_status(error);
            failures.push(RedactedProbeFailure::from_capture(
                ProbeProgressStage::Streaming,
                error,
            ));
        }
    }
    let streaming_failure = failures
        .iter()
        .rev()
        .find(|failure| failure.stage == ProbeProgressStage::Streaming)
        .cloned();
    report_stage_finished(
        reporter,
        candidate,
        transport,
        ProbeProgressStage::Streaming,
        assessment.streaming,
        streaming_failure,
    );

    report_stage_started(
        reporter,
        candidate,
        transport,
        ProbeProgressStage::ForcedTool,
    );
    match send_case(
        candidate,
        client,
        transport,
        ProbeCase::CustomToolAdmissionJson,
        nonce,
        None,
        probe_request_options(tool_schema_dialect, history_replay),
    )
    .await
    {
        Ok(exchange) if has_completed_assistant_turn(transport, &exchange) => {
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(&exchange),
            );
            evidence.push(exchange.evidence().clone());
        }
        Ok(exchange) => {
            evidence.push(exchange.evidence().clone());
            assessment.forced_tool = ProbeStageStatus::Failed;
            let failure = RedactedProbeFailure::invalid_response(ProbeProgressStage::ForcedTool);
            failures.push(failure.clone());
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::ForcedTool,
                assessment.forced_tool,
                Some(failure),
            );
            return finish_branch(
                reporter,
                candidate,
                TransportBranchResult {
                    assessment,
                    reasoning_shape,
                    tool_schema_dialect,
                    history_replay,
                    evidence,
                    failures,
                },
            );
        }
        Err(error) => {
            assessment.forced_tool = forced_tool_failure_status(error);
            let failure = RedactedProbeFailure::from_capture(ProbeProgressStage::ForcedTool, error);
            failures.push(failure.clone());
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::ForcedTool,
                assessment.forced_tool,
                Some(failure),
            );
            return finish_branch(
                reporter,
                candidate,
                TransportBranchResult {
                    assessment,
                    reasoning_shape,
                    tool_schema_dialect,
                    history_replay,
                    evidence,
                    failures,
                },
            );
        }
    }
    let mut forced_case = ProbeCase::ForcedToolSse;
    let mut forced = send_case(
        candidate,
        client,
        transport,
        ProbeCase::ForcedToolSse,
        nonce,
        None,
        probe_request_options(tool_schema_dialect, history_replay),
    )
    .await;
    if matches!(
        &forced,
        Err(ProbeCaptureError::ToolSchemaRejected {
            status_code: 400 | 422
        }) | Err(ProbeCaptureError::HttpStatus {
            status_code: 400 | 422
        })
    ) {
        tool_schema_dialect = ToolSchemaDialect::MoonshotMfjs;
        forced = send_case(
            candidate,
            client,
            transport,
            ProbeCase::ForcedToolSse,
            nonce,
            None,
            probe_request_options(tool_schema_dialect, history_replay),
        )
        .await;
    }
    let should_retry_with_required = forced.as_ref().is_ok_and(|exchange| {
        classify_probe_terminal(transport, exchange).is_complete
            && extract_tool_call(transport, exchange).is_none()
    });
    if should_retry_with_required {
        if let Ok(exchange) = &forced {
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(exchange),
            );
            evidence.push(exchange.evidence().clone());
        }
        forced_case = ProbeCase::ForcedToolRequiredSse;
        forced = send_case(
            candidate,
            client,
            transport,
            forced_case,
            nonce,
            None,
            probe_request_options(tool_schema_dialect, history_replay),
        )
        .await;
    }
    let should_retry_with_moonshot = tool_schema_dialect == ToolSchemaDialect::OpenAi
        && forced.as_ref().is_ok_and(|exchange| {
            classify_probe_terminal(transport, exchange).is_complete
                && extract_tool_call(transport, exchange)
                    .as_ref()
                    .is_none_or(|call| !valid_probe_tool_call(call, nonce))
        });
    if should_retry_with_moonshot {
        if let Ok(exchange) = &forced {
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(exchange),
            );
            evidence.push(exchange.evidence().clone());
        }
        tool_schema_dialect = ToolSchemaDialect::MoonshotMfjs;
        forced = send_case(
            candidate,
            client,
            transport,
            forced_case,
            nonce,
            None,
            probe_request_options(tool_schema_dialect, history_replay),
        )
        .await;
    }
    let (tool_call, forced_exchange) = match forced {
        Ok(exchange) => {
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(&exchange),
            );
            let terminal = classify_probe_terminal(transport, &exchange);
            let call = extract_tool_call(transport, &exchange);
            evidence.push(exchange.evidence().clone());
            if !terminal.is_complete {
                assessment.forced_tool = ProbeStageStatus::Failed;
                let failure =
                    RedactedProbeFailure::invalid_response(ProbeProgressStage::ForcedTool);
                failures.push(failure.clone());
                report_stage_finished(
                    reporter,
                    candidate,
                    transport,
                    ProbeProgressStage::ForcedTool,
                    assessment.forced_tool,
                    Some(failure),
                );
                return finish_branch(
                    reporter,
                    candidate,
                    TransportBranchResult {
                        assessment,
                        reasoning_shape,
                        tool_schema_dialect,
                        history_replay,
                        evidence,
                        failures,
                    },
                );
            }
            match call.filter(|call| valid_probe_tool_call(call, nonce)) {
                Some(call) => {
                    assessment.forced_tool = ProbeStageStatus::Passed;
                    report_stage_finished(
                        reporter,
                        candidate,
                        transport,
                        ProbeProgressStage::ForcedTool,
                        assessment.forced_tool,
                        None,
                    );
                    (call, exchange)
                }
                None => {
                    assessment.forced_tool = ProbeStageStatus::Unsupported;
                    report_stage_finished(
                        reporter,
                        candidate,
                        transport,
                        ProbeProgressStage::ForcedTool,
                        assessment.forced_tool,
                        None,
                    );
                    return finish_branch(
                        reporter,
                        candidate,
                        TransportBranchResult {
                            assessment,
                            reasoning_shape,
                            tool_schema_dialect,
                            history_replay,
                            evidence,
                            failures,
                        },
                    );
                }
            }
        }
        Err(error) => {
            assessment.forced_tool = forced_tool_failure_status(error);
            let failure = RedactedProbeFailure::from_capture(ProbeProgressStage::ForcedTool, error);
            failures.push(failure.clone());
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::ForcedTool,
                assessment.forced_tool,
                Some(failure),
            );
            return finish_branch(
                reporter,
                candidate,
                TransportBranchResult {
                    assessment,
                    reasoning_shape,
                    tool_schema_dialect,
                    history_replay,
                    evidence,
                    failures,
                },
            );
        }
    };

    report_stage_started(
        reporter,
        candidate,
        transport,
        ProbeProgressStage::Continuation,
    );
    let mut continuation = send_case(
        candidate,
        client,
        transport,
        ProbeCase::ToolContinuationJson,
        nonce,
        Some((&tool_call, &forced_exchange)),
        probe_request_options(tool_schema_dialect, history_replay),
    )
    .await;
    if transport == TransportKind::OpenAiResponses
        && is_bounded_replay_shape_rejection(&continuation)
    {
        history_replay = HistoryReplay::ResponsesReasoningTextContent;
        continuation = send_case(
            candidate,
            client,
            transport,
            ProbeCase::ToolContinuationJson,
            nonce,
            Some((&tool_call, &forced_exchange)),
            probe_request_options(tool_schema_dialect, history_replay),
        )
        .await;
        if is_bounded_replay_shape_rejection(&continuation) {
            history_replay = HistoryReplay::Omit;
            continuation = send_case(
                candidate,
                client,
                transport,
                ProbeCase::ToolContinuationJson,
                nonce,
                Some((&tool_call, &forced_exchange)),
                probe_request_options(tool_schema_dialect, history_replay),
            )
            .await;
        }
    }
    match continuation {
        Ok(exchange) => {
            assessment.continuation = if has_completed_tool_continuation(transport, &exchange) {
                ProbeStageStatus::Passed
            } else {
                ProbeStageStatus::Failed
            };
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(&exchange),
            );
            evidence.push(exchange.evidence().clone());
            if assessment.continuation == ProbeStageStatus::Failed {
                failures.push(RedactedProbeFailure::invalid_response(
                    ProbeProgressStage::Continuation,
                ));
            }
        }
        Err(error) => {
            assessment.continuation = stage_failure_status(error);
            failures.push(RedactedProbeFailure::from_capture(
                ProbeProgressStage::Continuation,
                error,
            ));
        }
    }
    let continuation_failure = failures
        .iter()
        .rev()
        .find(|failure| failure.stage == ProbeProgressStage::Continuation)
        .cloned();
    report_stage_finished(
        reporter,
        candidate,
        transport,
        ProbeProgressStage::Continuation,
        assessment.continuation,
        continuation_failure,
    );

    finish_branch(
        reporter,
        candidate,
        TransportBranchResult {
            assessment,
            reasoning_shape,
            tool_schema_dialect,
            history_replay,
            evidence,
            failures,
        },
    )
}

fn is_bounded_replay_shape_rejection(
    result: &Result<CapturedProbeExchange, ProbeCaptureError>,
) -> bool {
    matches!(
        result,
        Err(ProbeCaptureError::ReasoningReplayRejected {
            status_code: 400 | 422
        }) | Err(ProbeCaptureError::HttpStatus {
            status_code: 400 | 422
        })
    )
}

fn report_stage_started<F>(
    reporter: &F,
    candidate: &ProbeCandidate,
    transport: TransportKind,
    stage: ProbeProgressStage,
) where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    reporter(ProtocolProbeProgressEvent::StageStarted {
        model: candidate.public_model.clone(),
        transport,
        stage,
    });
}

fn report_stage_finished<F>(
    reporter: &F,
    candidate: &ProbeCandidate,
    transport: TransportKind,
    stage: ProbeProgressStage,
    stage_status: ProbeStageStatus,
    failure: Option<RedactedProbeFailure>,
) where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    reporter(ProtocolProbeProgressEvent::StageFinished {
        model: candidate.public_model.clone(),
        transport,
        stage,
        stage_status,
        failure,
    });
}

fn finish_branch<F>(
    reporter: &F,
    candidate: &ProbeCandidate,
    result: TransportBranchResult,
) -> TransportBranchResult
where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    let reasoning_status = if result.reasoning_shape.semantic == ReasoningSemantic::None {
        if result.assessment.baseline == ProbeStageStatus::Passed {
            ProbeStageStatus::Unsupported
        } else {
            ProbeStageStatus::Skipped
        }
    } else {
        ProbeStageStatus::Passed
    };
    if result.assessment.baseline == ProbeStageStatus::Passed {
        report_stage_started(
            reporter,
            candidate,
            result.assessment.transport,
            ProbeProgressStage::Reasoning,
        );
    }
    reporter(ProtocolProbeProgressEvent::ReasoningClassified {
        model: candidate.public_model.clone(),
        transport: result.assessment.transport,
        stage: ProbeProgressStage::Reasoning,
        reasoning_semantic: result.reasoning_shape.semantic,
        reasoning_source: result.reasoning_shape.source,
    });
    report_stage_finished(
        reporter,
        candidate,
        result.assessment.transport,
        ProbeProgressStage::Reasoning,
        reasoning_status,
        None,
    );
    reporter(ProtocolProbeProgressEvent::BranchFinished {
        model: candidate.public_model.clone(),
        transport: result.assessment.transport,
        readiness: branch_readiness(result.assessment),
    });
    result
}

fn branch_readiness(assessment: TransportProbeAssessment) -> ProbeReadiness {
    if assessment.is_complete() {
        ProbeReadiness::Verified
    } else if assessment.baseline == ProbeStageStatus::Passed {
        ProbeReadiness::Partial
    } else {
        ProbeReadiness::Unverified
    }
}

#[allow(clippy::too_many_arguments)]
async fn send_case(
    candidate: &ProbeCandidate,
    client: &Client,
    transport: TransportKind,
    case: ProbeCase,
    nonce: &str,
    continuation: Option<(&CapturedToolCall, &CapturedProbeExchange)>,
    options: CodexRequestOptions,
) -> Result<CapturedProbeExchange, ProbeCaptureError> {
    let logical = match continuation {
        Some((tool_call, exchange)) => build_continuation_request(
            &candidate.upstream_model,
            nonce,
            transport,
            tool_call,
            exchange,
        ),
        None => build_logical_probe_request(case, &candidate.upstream_model, nonce),
    };
    let prepared = candidate
        .prepare_request_with_options(transport, logical, options)
        .map_err(|_| ProbeCaptureError::InvalidPayload)?;
    let request = client
        .post(prepared.url)
        .headers(prepared.headers)
        .json(&prepared.body);
    capture_transport_probe(request, RESPONSE_TIMEOUT).await
}

fn probe_request_options(
    tool_schema_dialect: ToolSchemaDialect,
    history_replay: HistoryReplay,
) -> CodexRequestOptions {
    CodexRequestOptions {
        tool_schema_dialect: Some(tool_schema_dialect),
        history_replay: Some(history_replay),
        ..CodexRequestOptions::default()
    }
}

fn build_continuation_request(
    model: &str,
    nonce: &str,
    transport: TransportKind,
    tool_call: &CapturedToolCall,
    exchange: &CapturedProbeExchange,
) -> Value {
    let mut request = build_logical_probe_request(ProbeCase::ForcedToolSse, model, nonce);
    request["stream"] = Value::Bool(false);
    if let Some(object) = request.as_object_mut() {
        object.remove("tool_choice");
    }
    let mut input = request
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if transport == TransportKind::OpenAiResponses {
        input.extend(
            extract_responses_output_items(exchange)
                .into_iter()
                .map(canonicalize_reasoning_item_for_summary_replay),
        );
    } else {
        let mut function_call = json!({
            "type": "function_call",
            "call_id": tool_call.call_id,
            "name": tool_call.name,
            "arguments": tool_call.arguments
        });
        if !tool_call.reasoning_content.is_empty() {
            function_call["reasoning_content"] = Value::String(tool_call.reasoning_content.clone());
        }
        input.push(function_call);
    }
    input.push(json!({
        "type": "function_call_output",
        "call_id": tool_call.call_id,
        "output": "CCSM_PROTOCOL_TOOL_RESULT_OK"
    }));
    request["input"] = Value::Array(input);
    request
}

fn canonicalize_reasoning_item_for_summary_replay(mut item: Value) -> Value {
    if item.get("type").and_then(Value::as_str) != Some("reasoning") {
        return item;
    }
    let has_summary = item
        .get("summary")
        .and_then(Value::as_array)
        .is_some_and(|summary| !summary.is_empty());
    if !has_summary {
        let text = item
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            item["summary"] = json!([{"type": "summary_text", "text": text}]);
        }
    }
    if let Some(object) = item.as_object_mut() {
        object.remove("content");
    }
    item
}

fn extract_responses_output_items(exchange: &CapturedProbeExchange) -> Vec<Value> {
    if let Some(output) = exchange.payloads().iter().find_map(|payload| {
        payload
            .value
            .pointer("/response/output")
            .and_then(Value::as_array)
    }) {
        return output.clone();
    }

    exchange
        .payloads()
        .iter()
        .filter(|payload| payload.event_type.as_deref() == Some("response.output_item.done"))
        .filter_map(|payload| payload.value.get("item").cloned())
        .collect()
}

fn extract_tool_call(
    transport: TransportKind,
    exchange: &CapturedProbeExchange,
) -> Option<CapturedToolCall> {
    match transport {
        TransportKind::OpenAiResponses => extract_responses_tool_call(exchange),
        TransportKind::OpenAiChat => extract_chat_tool_call(exchange),
    }
}

fn extract_responses_tool_call(exchange: &CapturedProbeExchange) -> Option<CapturedToolCall> {
    exchange.payloads().iter().find_map(|payload| {
        let item = payload.value.get("item").unwrap_or(&payload.value);
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            return None;
        }
        Some(CapturedToolCall {
            call_id: item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)?
                .to_string(),
            name: item.get("name").and_then(Value::as_str)?.to_string(),
            arguments: item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string(),
            reasoning_content: item
                .get("reasoning_content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
    })
}

#[derive(Default)]
struct ChatToolAccumulator {
    call_id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ProbeTerminalOutcome {
    is_complete: bool,
    has_final_message: bool,
}

fn classify_probe_terminal(
    transport: TransportKind,
    exchange: &CapturedProbeExchange,
) -> ProbeTerminalOutcome {
    match transport {
        TransportKind::OpenAiChat => classify_chat_probe_terminal(exchange),
        TransportKind::OpenAiResponses => classify_responses_probe_terminal(exchange),
    }
}

fn classify_chat_probe_terminal(exchange: &CapturedProbeExchange) -> ProbeTerminalOutcome {
    let mut finish_reason = None;
    let mut has_final_message = false;
    let mut tools = BTreeMap::<usize, ChatToolAccumulator>::new();

    for payload in exchange.payloads() {
        let Some(choice) = payload
            .value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            continue;
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            finish_reason = Some(reason);
        }
        for message in [choice.get("message"), choice.get("delta")]
            .into_iter()
            .flatten()
        {
            has_final_message |= chat_message_has_final_content(message);
            observe_chat_tool_calls(message, &mut tools);
        }
    }

    let valid_tool_calls = tools
        .values()
        .filter(|tool| {
            !tool.name.trim().is_empty() && streamed_tool_arguments_are_complete(&tool.arguments)
        })
        .count();
    let evidence = ChatTerminalEvidence {
        has_final_message,
        valid_tool_calls,
        dropped_tool_calls: tools.len().saturating_sub(valid_tool_calls),
    };
    ProbeTerminalOutcome {
        is_complete: matches!(
            classify_chat_terminal(finish_reason, evidence),
            TerminalDisposition::Completed
        ),
        has_final_message,
    }
}

fn chat_message_has_final_content(message: &Value) -> bool {
    if message
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|text| {
            split_leading_think_block(text)
                .map(|(_, answer)| !answer.trim().is_empty())
                .unwrap_or_else(|| !text.trim().is_empty())
        })
        || message
            .get("refusal")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.trim().is_empty())
    {
        return true;
    }

    message
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|parts| {
            parts.iter().any(|part| {
                part.get("text")
                    .or_else(|| part.get("refusal"))
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty())
            })
        })
}

fn observe_chat_tool_calls(message: &Value, tools: &mut BTreeMap<usize, ChatToolAccumulator>) {
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for (position, call) in calls.iter().enumerate() {
            let index = call
                .get("index")
                .and_then(Value::as_u64)
                .map(|index| index as usize)
                .unwrap_or(position);
            let accumulator = tools.entry(index).or_default();
            if let Some(call_id) = call.get("id").and_then(Value::as_str) {
                if !call_id.is_empty() {
                    accumulator.call_id = call_id.to_string();
                }
            }
            if let Some(function) = call.get("function") {
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    if !name.is_empty() {
                        accumulator.name = name.to_string();
                    }
                }
                append_text_field(function.get("arguments"), &mut accumulator.arguments);
            }
        }
    }

    if let Some(function) = message
        .get("function_call")
        .filter(|value| value.is_object())
    {
        let accumulator = tools.entry(0).or_default();
        if let Some(name) = function.get("name").and_then(Value::as_str) {
            if !name.is_empty() {
                accumulator.name = name.to_string();
            }
        }
        append_text_field(function.get("arguments"), &mut accumulator.arguments);
    }
}

fn classify_responses_probe_terminal(exchange: &CapturedProbeExchange) -> ProbeTerminalOutcome {
    let mut evidence = NativeResponsesEvidence::default();
    for payload in exchange.payloads() {
        if let Some(event_name) = payload
            .event_type
            .as_deref()
            .or_else(|| payload.value.get("type").and_then(Value::as_str))
        {
            evidence.observe_event(event_name, &payload.value);
            if let Some(disposition) =
                classify_native_responses_terminal(event_name, &payload.value, evidence)
            {
                return ProbeTerminalOutcome {
                    is_complete: matches!(
                        disposition,
                        NativeResponsesTerminalDisposition::Completed
                    ),
                    has_final_message: evidence.has_final_message,
                };
            }
            continue;
        }

        let event_name = match payload.value.get("status").and_then(Value::as_str) {
            Some("incomplete") => "response.incomplete",
            Some("failed") => "response.failed",
            _ => "response.completed",
        };
        let terminal_payload = json!({"response": payload.value.clone()});
        evidence.observe_event(event_name, &terminal_payload);
        let disposition =
            classify_native_responses_terminal(event_name, &terminal_payload, evidence);
        return ProbeTerminalOutcome {
            is_complete: matches!(
                disposition,
                Some(NativeResponsesTerminalDisposition::Completed)
            ),
            has_final_message: evidence.has_final_message,
        };
    }

    ProbeTerminalOutcome::default()
}

fn extract_chat_tool_call(exchange: &CapturedProbeExchange) -> Option<CapturedToolCall> {
    let mut tools = BTreeMap::<usize, ChatToolAccumulator>::new();
    let mut reasoning_content = String::new();
    for payload in exchange.payloads() {
        let Some(choices) = payload.value.get("choices").and_then(Value::as_array) else {
            continue;
        };
        for choice in choices {
            let Some(delta) = choice.get("delta") else {
                continue;
            };
            append_text_field(delta.get("reasoning_content"), &mut reasoning_content);
            let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) else {
                continue;
            };
            for call in calls {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                let accumulator = tools.entry(index).or_default();
                append_text_field(call.get("id"), &mut accumulator.call_id);
                if let Some(function) = call.get("function") {
                    append_text_field(function.get("name"), &mut accumulator.name);
                    append_text_field(function.get("arguments"), &mut accumulator.arguments);
                }
            }
        }
    }
    let (_, tool) = tools.into_iter().next()?;
    Some(CapturedToolCall {
        call_id: tool.call_id,
        name: tool.name,
        arguments: tool.arguments,
        reasoning_content,
    })
}

fn append_text_field(value: Option<&Value>, target: &mut String) {
    if let Some(text) = value.and_then(Value::as_str) {
        target.push_str(text);
    }
}

fn valid_probe_tool_call(call: &CapturedToolCall, nonce: &str) -> bool {
    call.name == super::TOOL_NAME
        && serde_json::from_str::<Value>(&call.arguments)
            .ok()
            .and_then(|arguments| {
                arguments
                    .get("nonce")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            == Some(nonce)
}

fn has_completed_assistant_turn(
    transport: TransportKind,
    exchange: &CapturedProbeExchange,
) -> bool {
    let terminal = classify_probe_terminal(transport, exchange);
    terminal.is_complete && terminal.has_final_message
}

fn has_completed_tool_continuation(
    transport: TransportKind,
    exchange: &CapturedProbeExchange,
) -> bool {
    has_completed_assistant_turn(transport, exchange)
        && collected_assistant_text(transport, exchange).contains(super::TOOL_DONE_MARKER)
}

fn collected_assistant_text(transport: TransportKind, exchange: &CapturedProbeExchange) -> String {
    let mut text = String::new();
    for payload in exchange.payloads() {
        match transport {
            TransportKind::OpenAiChat => {
                let Some(choice) = payload
                    .value
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|choices| choices.first())
                else {
                    continue;
                };
                for message in [choice.get("message"), choice.get("delta")]
                    .into_iter()
                    .flatten()
                {
                    append_visible_content(message.get("content"), &mut text);
                }
            }
            TransportKind::OpenAiResponses => {
                for output in [
                    payload.value.get("output"),
                    payload.value.pointer("/response/output"),
                ]
                .into_iter()
                .flatten()
                .filter_map(Value::as_array)
                {
                    for item in output
                        .iter()
                        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
                    {
                        append_visible_content(item.get("content"), &mut text);
                    }
                }
                if payload.event_type.as_deref() == Some("response.output_text.delta") {
                    append_text_field(payload.value.get("delta"), &mut text);
                }
                let item = payload.value.get("item");
                if item
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    == Some("message")
                {
                    append_visible_content(item.and_then(|item| item.get("content")), &mut text);
                }
            }
        }
    }
    text
}

fn append_visible_content(content: Option<&Value>, target: &mut String) {
    match content {
        Some(Value::String(value)) => target.push_str(value),
        Some(Value::Array(parts)) => {
            for part in parts {
                append_text_field(part.get("text").or_else(|| part.get("content")), target);
            }
        }
        _ => {}
    }
}

fn baseline_failure_status(error: ProbeCaptureError) -> ProbeStageStatus {
    match error {
        ProbeCaptureError::HttpStatus {
            status_code: 404 | 405 | 415,
        }
        | ProbeCaptureError::ToolSchemaRejected {
            status_code: 404 | 405 | 415,
        }
        | ProbeCaptureError::ReasoningReplayRejected {
            status_code: 404 | 405 | 415,
        } => ProbeStageStatus::Unsupported,
        _ => ProbeStageStatus::Failed,
    }
}

fn forced_tool_failure_status(error: ProbeCaptureError) -> ProbeStageStatus {
    match error {
        ProbeCaptureError::HttpStatus {
            status_code: 400 | 404 | 405 | 415 | 422,
        }
        | ProbeCaptureError::ToolSchemaRejected {
            status_code: 400 | 404 | 405 | 415 | 422,
        }
        | ProbeCaptureError::ReasoningReplayRejected {
            status_code: 400 | 404 | 405 | 415 | 422,
        } => ProbeStageStatus::Unsupported,
        _ => ProbeStageStatus::Failed,
    }
}

fn stage_failure_status(error: ProbeCaptureError) -> ProbeStageStatus {
    match error {
        ProbeCaptureError::HttpStatus {
            status_code: 404 | 405 | 415,
        }
        | ProbeCaptureError::ToolSchemaRejected {
            status_code: 404 | 405 | 415,
        }
        | ProbeCaptureError::ReasoningReplayRejected {
            status_code: 404 | 405 | 415,
        } => ProbeStageStatus::Unsupported,
        _ => ProbeStageStatus::Failed,
    }
}

fn empty_reasoning_shape() -> ClassifiedReasoningShape {
    ClassifiedReasoningShape {
        semantic: ReasoningSemantic::None,
        source: ReasoningSource::None,
        pre_tool_visible_content: PreToolVisibleContent::Absent,
    }
}

fn update_shape(current: &mut ClassifiedReasoningShape, observed: ClassifiedReasoningShape) {
    current.pre_tool_visible_content = match (
        current.pre_tool_visible_content,
        observed.pre_tool_visible_content,
    ) {
        (PreToolVisibleContent::Present, _) | (_, PreToolVisibleContent::Present) => {
            PreToolVisibleContent::Present
        }
        _ => PreToolVisibleContent::Absent,
    };

    let observed_rank = semantic_information_rank(observed.semantic);
    let current_rank = semantic_information_rank(current.semantic);
    if observed_rank > current_rank {
        current.semantic = observed.semantic;
        current.source = observed.source;
    }
}

fn semantic_information_rank(semantic: ReasoningSemantic) -> u8 {
    match semantic {
        ReasoningSemantic::None => 0,
        ReasoningSemantic::Opaque => 1,
        ReasoningSemantic::Readable => 2,
        ReasoningSemantic::Summary => 3,
    }
}
