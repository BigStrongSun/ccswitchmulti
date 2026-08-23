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
    match tokio::time::timeout(TRANSACTION_TIMEOUT, run_probe(candidate, client)).await {
        Ok(result) => result,
        Err(_) => ProtocolCompatibilityProbeResult {
            selected_transport: None,
            readiness: ProbeReadiness::Unverified,
            branches: Vec::new(),
        },
    }
}

async fn run_probe(candidate: ProbeCandidate, client: &Client) -> ProtocolCompatibilityProbeResult {
    let nonce = Uuid::new_v4().simple().to_string();
    let mut branches = Vec::with_capacity(2);

    for transport in [TransportKind::OpenAiResponses, TransportKind::OpenAiChat] {
        branches.push(run_branch(&candidate, client, transport, &nonce).await);
    }

    let candidates = branches
        .iter()
        .map(|branch| (branch.assessment, branch.reasoning_shape.semantic))
        .collect::<Vec<_>>();
    let selection = select_transport_outcome_with_reasoning(&candidates);
    ProtocolCompatibilityProbeResult {
        selected_transport: selection.map(|selected| selected.transport),
        readiness: selection
            .map(|selected| selected.readiness)
            .unwrap_or(ProbeReadiness::Unverified),
        branches,
    }
}

async fn run_branch(
    candidate: &ProbeCandidate,
    client: &Client,
    transport: TransportKind,
    nonce: &str,
) -> TransportBranchResult {
    let mut assessment = TransportProbeAssessment {
        transport,
        baseline: ProbeStageStatus::Skipped,
        streaming: ProbeStageStatus::Skipped,
        forced_tool: ProbeStageStatus::Skipped,
        continuation: ProbeStageStatus::Skipped,
    };
    let mut evidence = Vec::new();
    let mut reasoning_shape = empty_reasoning_shape();
    let Ok(endpoint) = build_probe_url(
        &candidate.canonical_endpoint(),
        transport,
        candidate.is_full_url(),
    ) else {
        assessment.baseline = ProbeStageStatus::Failed;
        return TransportBranchResult {
            assessment,
            reasoning_shape,
            evidence,
        };
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
        Ok(exchange) => {
            assessment.baseline = ProbeStageStatus::Passed;
            exchange
        }
        Err(error) => {
            assessment.baseline = baseline_failure_status(error);
            return TransportBranchResult {
                assessment,
                reasoning_shape,
                evidence,
            };
        }
    };
    update_shape(
        &mut reasoning_shape,
        classify_captured_reasoning_shape(&baseline_exchange),
    );
    evidence.push(baseline_exchange.evidence().clone());

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
    let tool_call = match forced {
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
                    call
                }
                None => {
                    assessment.forced_tool = ProbeStageStatus::Unsupported;
                    return TransportBranchResult {
                        assessment,
                        reasoning_shape,
                        evidence,
                    };
                }
            }
        }
        Err(error) => {
            assessment.forced_tool = forced_tool_failure_status(error);
            return TransportBranchResult {
                assessment,
                reasoning_shape,
                evidence,
            };
        }
    };

    match send_case(
        candidate,
        client,
        transport,
        &endpoint,
        ProbeCase::ToolContinuationJson,
        nonce,
        Some(&tool_call),
    )
    .await
    {
        Ok(exchange) => {
            assessment.continuation = ProbeStageStatus::Passed;
            update_shape(
                &mut reasoning_shape,
                classify_captured_reasoning_shape(&exchange),
            );
            evidence.push(exchange.evidence().clone());
        }
        Err(error) => assessment.continuation = stage_failure_status(error),
    }

    TransportBranchResult {
        assessment,
        reasoning_shape,
        evidence,
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
    tool_call: Option<&CapturedToolCall>,
) -> Result<CapturedProbeExchange, ProbeCaptureError> {
    let logical = match tool_call {
        Some(tool_call) => build_continuation_request(&candidate.upstream_model, nonce, tool_call),
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

fn build_continuation_request(model: &str, nonce: &str, tool_call: &CapturedToolCall) -> Value {
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
    input.push(json!({
        "type": "function_call_output",
        "call_id": tool_call.call_id,
        "output": "CCSM_PROTOCOL_TOOL_RESULT_OK"
    }));
    request["input"] = Value::Array(input);
    request
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
