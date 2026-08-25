use std::{collections::BTreeMap, fmt, time::Duration};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning;

use super::{
    build_logical_probe_request,
    capture::{capture_transport_probe, CapturedProbeExchange, ProbeCaptureError},
    classify::ClassifiedReasoningShape,
    classify_captured_reasoning_shape,
    endpoint::build_probe_url,
    redaction::RedactedProbeEvidence,
    selection::select_transport_outcome_with_reasoning,
    PreToolVisibleContent, ProbeCandidate, ProbeCase, ProbeReadiness, ProbeStageStatus,
    ReasoningSemantic, ReasoningSource, TransportKind, TransportProbeAssessment,
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
    evidence: Vec<RedactedProbeEvidence>,
}

impl fmt::Debug for TransportBranchResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TransportBranchResult")
            .field("assessment", &self.assessment)
            .field("reasoning_shape", &self.reasoning_shape)
            .field("evidence_count", &self.evidence.len())
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
    let mut reasoning_shape = empty_reasoning_shape();
    report_stage_started(reporter, candidate, transport, ProbeProgressStage::Baseline);
    let Ok(endpoint) = build_probe_url(
        &candidate.canonical_endpoint(),
        transport,
        candidate.is_full_url(),
    ) else {
        assessment.baseline = ProbeStageStatus::Failed;
        report_stage_finished(
            reporter,
            candidate,
            transport,
            ProbeProgressStage::Baseline,
            assessment.baseline,
        );
        return finish_branch(
            reporter,
            candidate,
            TransportBranchResult {
                assessment,
                reasoning_shape,
                evidence,
            },
        );
    };

    let baseline = send_case(
        candidate,
        client,
        transport,
        &endpoint,
        ProbeCase::BaselineJson,
        nonce,
        None,
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
            );
            exchange
        }
        Ok(_) => {
            assessment.baseline = ProbeStageStatus::Failed;
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::Baseline,
                assessment.baseline,
            );
            return finish_branch(
                reporter,
                candidate,
                TransportBranchResult {
                    assessment,
                    reasoning_shape,
                    evidence,
                },
            );
        }
        Err(error) => {
            assessment.baseline = baseline_failure_status(error);
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::Baseline,
                assessment.baseline,
            );
            return finish_branch(
                reporter,
                candidate,
                TransportBranchResult {
                    assessment,
                    reasoning_shape,
                    evidence,
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
        &endpoint,
        ProbeCase::BaselineSse,
        nonce,
        None,
    )
    .await
    {
        Ok(exchange) => {
            assessment.streaming = if has_stream_terminal(transport, &exchange) {
                ProbeStageStatus::Passed
            } else {
                ProbeStageStatus::Failed
            };
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(&exchange),
            );
            evidence.push(exchange.evidence().clone());
        }
        Err(error) => assessment.streaming = stage_failure_status(error),
    }
    report_stage_finished(
        reporter,
        candidate,
        transport,
        ProbeProgressStage::Streaming,
        assessment.streaming,
    );

    report_stage_started(
        reporter,
        candidate,
        transport,
        ProbeProgressStage::ForcedTool,
    );
    let forced = send_case(
        candidate,
        client,
        transport,
        &endpoint,
        ProbeCase::ForcedToolSse,
        nonce,
        None,
    )
    .await;
    let (tool_call, forced_exchange) = match forced {
        Ok(exchange) => {
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(&exchange),
            );
            let call = extract_tool_call(transport, &exchange);
            evidence.push(exchange.evidence().clone());
            match call.filter(|call| valid_probe_tool_call(call, nonce)) {
                Some(call) => {
                    assessment.forced_tool = ProbeStageStatus::Passed;
                    report_stage_finished(
                        reporter,
                        candidate,
                        transport,
                        ProbeProgressStage::ForcedTool,
                        assessment.forced_tool,
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
                    );
                    return finish_branch(
                        reporter,
                        candidate,
                        TransportBranchResult {
                            assessment,
                            reasoning_shape,
                            evidence,
                        },
                    );
                }
            }
        }
        Err(error) => {
            assessment.forced_tool = forced_tool_failure_status(error);
            report_stage_finished(
                reporter,
                candidate,
                transport,
                ProbeProgressStage::ForcedTool,
                assessment.forced_tool,
            );
            return finish_branch(
                reporter,
                candidate,
                TransportBranchResult {
                    assessment,
                    reasoning_shape,
                    evidence,
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
    match send_case(
        candidate,
        client,
        transport,
        &endpoint,
        ProbeCase::ToolContinuationJson,
        nonce,
        Some((&tool_call, &forced_exchange)),
    )
    .await
    {
        Ok(exchange) => {
            assessment.continuation = if has_completed_assistant_turn(transport, &exchange) {
                ProbeStageStatus::Passed
            } else {
                ProbeStageStatus::Failed
            };
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(&exchange),
            );
            evidence.push(exchange.evidence().clone());
        }
        Err(error) => assessment.continuation = stage_failure_status(error),
    }
    report_stage_finished(
        reporter,
        candidate,
        transport,
        ProbeProgressStage::Continuation,
        assessment.continuation,
    );

    finish_branch(
        reporter,
        candidate,
        TransportBranchResult {
            assessment,
            reasoning_shape,
            evidence,
        },
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
) where
    F: Fn(ProtocolProbeProgressEvent) + Send + Sync,
{
    reporter(ProtocolProbeProgressEvent::StageFinished {
        model: candidate.public_model.clone(),
        transport,
        stage,
        stage_status,
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
    endpoint: &str,
    case: ProbeCase,
    nonce: &str,
    continuation: Option<(&CapturedToolCall, &CapturedProbeExchange)>,
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
    let wire_body = match transport {
        TransportKind::OpenAiResponses => logical,
        TransportKind::OpenAiChat => responses_to_chat_completions_with_reasoning(logical, None)
            .map_err(|_| ProbeCaptureError::InvalidPayload)?,
    };
    let mut request = client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(&wire_body);
    if let Some(authorization) = candidate.bearer_token() {
        request = request.header(reqwest::header::AUTHORIZATION, authorization.clone());
    }
    capture_transport_probe(request, RESPONSE_TIMEOUT).await
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
        input.extend(extract_responses_output_items(exchange));
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

fn has_stream_terminal(transport: TransportKind, exchange: &CapturedProbeExchange) -> bool {
    match transport {
        TransportKind::OpenAiChat => exchange.saw_done(),
        TransportKind::OpenAiResponses => exchange
            .evidence()
            .event_types
            .iter()
            .any(|event| event == "response.completed"),
    }
}

fn has_completed_assistant_turn(
    transport: TransportKind,
    exchange: &CapturedProbeExchange,
) -> bool {
    match transport {
        TransportKind::OpenAiChat => {
            let mut text = String::new();
            let mut completed = exchange.saw_done();
            for payload in exchange.payloads() {
                let choices = payload
                    .value
                    .get("choices")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten();
                for choice in choices {
                    completed |=
                        choice.get("finish_reason").and_then(Value::as_str) == Some("stop");
                    append_text_field(
                        choice
                            .get("message")
                            .and_then(|message| message.get("content")),
                        &mut text,
                    );
                    append_text_field(
                        choice.get("delta").and_then(|delta| delta.get("content")),
                        &mut text,
                    );
                }
            }
            completed && !text.trim().is_empty()
        }
        TransportKind::OpenAiResponses => {
            let mut text = String::new();
            let mut completed = false;
            for payload in exchange.payloads() {
                completed |= payload.value.get("status").and_then(Value::as_str)
                    == Some("completed")
                    || payload
                        .value
                        .pointer("/response/status")
                        .and_then(Value::as_str)
                        == Some("completed");
                if payload.event_type.as_deref() == Some("response.output_text.delta") {
                    append_text_field(payload.value.get("delta"), &mut text);
                }
                if let Some(output) = payload
                    .value
                    .get("output")
                    .and_then(Value::as_array)
                    .or_else(|| {
                        payload
                            .value
                            .pointer("/response/output")
                            .and_then(Value::as_array)
                    })
                {
                    for item in output {
                        if item.get("type").and_then(Value::as_str) != Some("message")
                            || item.get("role").and_then(Value::as_str) != Some("assistant")
                        {
                            continue;
                        }
                        if let Some(content) = item.get("content").and_then(Value::as_array) {
                            for part in content {
                                append_text_field(part.get("text"), &mut text);
                            }
                        }
                    }
                }
            }
            completed && !text.trim().is_empty()
        }
    }
}

fn baseline_failure_status(error: ProbeCaptureError) -> ProbeStageStatus {
    match error {
        ProbeCaptureError::HttpStatus {
            status_code: 404 | 405 | 415,
        } => ProbeStageStatus::Unsupported,
        _ => ProbeStageStatus::Failed,
    }
}

fn forced_tool_failure_status(error: ProbeCaptureError) -> ProbeStageStatus {
    match error {
        ProbeCaptureError::HttpStatus {
            status_code: 400 | 404 | 405 | 415 | 422,
        } => ProbeStageStatus::Unsupported,
        _ => ProbeStageStatus::Failed,
    }
}

fn stage_failure_status(error: ProbeCaptureError) -> ProbeStageStatus {
    match error {
        ProbeCaptureError::HttpStatus {
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

    let observed_rank = semantic_safety_rank(observed.semantic);
    let current_rank = semantic_safety_rank(current.semantic);
    if observed_rank > current_rank {
        current.semantic = observed.semantic;
        current.source = observed.source;
    }
}

fn semantic_safety_rank(semantic: ReasoningSemantic) -> u8 {
    match semantic {
        ReasoningSemantic::None => 0,
        ReasoningSemantic::Readable => 1,
        ReasoningSemantic::Summary => 2,
        ReasoningSemantic::Opaque => 3,
    }
}
