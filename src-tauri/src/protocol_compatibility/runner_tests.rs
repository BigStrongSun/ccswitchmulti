use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use bytes::Bytes;
use futures::{stream, StreamExt};
use serde_json::{json, Value};
use tokio::{net::TcpListener, task::JoinHandle};

use super::{
    run_protocol_compatibility_probe, run_protocol_compatibility_probe_with_reporter,
    PreToolVisibleContent, ProbeCandidate, ProbeProgressStage, ProbeReadiness, ProbeStageStatus,
    ProbeTargetKey, ProtocolCompatibilityRecord, ProtocolProbeProgressEvent, ReasoningProjection,
    ReasoningSemantic, ReasoningSource, TransportKind,
};
use crate::proxy::providers::{
    streaming_codex_chat::create_responses_sse_stream_from_chat_with_context_and_projection,
    transform_codex_chat::{
        chat_completion_to_response_with_context_and_projection, CodexToolContext,
    },
};

#[derive(Clone, Copy)]
enum ResponsesMode {
    Complete,
    OpaqueReasoning,
    BaselineOpaqueSseReadable,
    BaselineUnsupported,
    UpstreamUnavailable,
    ToolUnsupported,
    InvalidSuccessfulJson,
    IncompleteContinuation,
    MarkerMismatch,
    ChatDoneWithoutFinishReason,
    ChatUnknownFinishReason,
    ChatLengthTerminal,
    ChatReasoningOnlyStop,
    ResponsesCompletedWithoutStatus,
    StreamingFinalOutputMissing,
    ForcedToolTerminalMissing,
    ForcedToolIncomplete,
    ForcedToolFailed,
    ForcedToolMissingName,
    ForcedToolMalformedArguments,
    ResponsesToolAddedBeforeDone,
    AutoToolIgnoredRequiredSucceeds,
    AcceptedComplexSchemaIgnoresTools,
    AcceptedComplexSchemaReturnsEmptyArguments,
    MoonshotToolSchemaOnly,
    GenericMoonshotToolSchemaOnly,
    ResponsesCustomToolUnsupported,
    SummaryReplayOnly,
    ReasoningTextReplayOnly,
    OmitReasoningReplayOnly,
    GenericOmitReasoningReplayOnly,
}

#[derive(Clone)]
struct FixtureState {
    responses_mode: ResponsesMode,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
}

struct FixtureServer {
    base_url: String,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    task: JoinHandle<()>,
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn spawn_fixture(responses_mode: ResponsesMode) -> FixtureServer {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let state = FixtureState {
        responses_mode,
        requests: requests.clone(),
    };
    let app = Router::new()
        .route("/v1/responses", post(upstream))
        .route("/v1/chat/completions", post(upstream))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    FixtureServer {
        base_url: format!("http://{address}"),
        requests,
        task,
    }
}

async fn upstream(
    State(state): State<FixtureState>,
    uri: Uri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    assert_eq!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer fixture-secret")
    );
    let path = uri.path().to_string();
    state
        .requests
        .lock()
        .unwrap()
        .push((path.clone(), body.clone()));

    let is_responses = path.ends_with("/responses");
    if matches!(state.responses_mode, ResponsesMode::UpstreamUnavailable) {
        return Response::builder()
            .status(521)
            .body(Body::from(
                "private upstream response body must never leave the backend",
            ))
            .unwrap();
    }
    if is_responses && matches!(state.responses_mode, ResponsesMode::BaselineUnsupported) {
        return StatusCode::NOT_FOUND.into_response();
    }

    let is_stream = body.get("stream").and_then(Value::as_bool) == Some(true);
    let is_forced_tool = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    let is_continuation = if is_responses {
        body.get("input")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|item| {
                    item.get("type").and_then(Value::as_str) == Some("function_call_output")
                })
            })
    } else {
        body.get("messages")
            .and_then(Value::as_array)
            .is_some_and(|messages| {
                messages
                    .iter()
                    .any(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
            })
    };

    if is_forced_tool
        && matches!(
            state.responses_mode,
            ResponsesMode::MoonshotToolSchemaOnly | ResponsesMode::GenericMoonshotToolSchemaOnly
        )
        && tool_parameter_schemas_contain_keyword(
            &body,
            &["$defs", "$ref", "oneOf", "const", "format"],
        )
    {
        return match state.responses_mode {
            ResponsesMode::GenericMoonshotToolSchemaOnly => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "Invalid request Error"}})),
            )
                .into_response(),
            _ => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": {"message": "tools.function.parameters is not a valid moonshot flavored json schema"}})),
            )
                .into_response(),
        };
    }

    if is_responses
        && is_forced_tool
        && matches!(
            state.responses_mode,
            ResponsesMode::ResponsesCustomToolUnsupported
        )
        && body
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| {
                tools
                    .iter()
                    .any(|tool| tool.get("type").and_then(Value::as_str) == Some("custom"))
            })
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": {
                    "message": "tools[1].type: unknown variant `custom`, expected one of `function`, `web_search_preview`, `code_interpreter`, `mcp`"
                }
            })),
        )
            .into_response();
    }

    if is_responses && is_continuation {
        let reasoning = body
            .get("input")
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
            });
        let has_summary = reasoning
            .and_then(|item| item.get("summary"))
            .and_then(Value::as_array)
            .is_some_and(|summary| !summary.is_empty());
        let has_reasoning_text = reasoning
            .and_then(|item| item.get("content"))
            .and_then(Value::as_array)
            .is_some_and(|content| {
                content.iter().any(|part| {
                    part.get("type").and_then(Value::as_str) == Some("reasoning_text")
                        && part
                            .get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.is_empty())
                })
            });
        let rejects = match state.responses_mode {
            ResponsesMode::SummaryReplayOnly => !has_summary || has_reasoning_text,
            ResponsesMode::ReasoningTextReplayOnly => !has_reasoning_text,
            ResponsesMode::OmitReasoningReplayOnly
            | ResponsesMode::GenericOmitReasoningReplayOnly => reasoning.is_some(),
            _ => false,
        };
        if rejects {
            if matches!(
                state.responses_mode,
                ResponsesMode::GenericOmitReasoningReplayOnly
            ) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": {"message": "Invalid request Error"}})),
                )
                    .into_response();
            }
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "Invalid reasoning replay content must be passed back in the supported shape"}})),
            )
                .into_response();
        }
    }

    if is_responses && matches!(state.responses_mode, ResponsesMode::InvalidSuccessfulJson) {
        return Json(json!({})).into_response();
    }

    if is_responses
        && is_continuation
        && matches!(state.responses_mode, ResponsesMode::IncompleteContinuation)
    {
        return Json(json!({
            "id": "resp_incomplete",
            "object": "response",
            "status": "in_progress",
            "output": []
        }))
        .into_response();
    }

    if is_responses
        && is_forced_tool
        && matches!(state.responses_mode, ResponsesMode::ToolUnsupported)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }

    if is_forced_tool
        && is_stream
        && matches!(
            state.responses_mode,
            ResponsesMode::AutoToolIgnoredRequiredSucceeds
        )
        && body.get("tool_choice").and_then(Value::as_str) != Some("required")
    {
        if is_responses {
            return sse(
                "event: response.output_text.delta\ndata: {\"delta\":\"normal answer without a tool call\"}\n\nevent: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"
                    .to_string(),
            );
        }
        return sse(
            "data: {\"choices\":[{\"delta\":{\"content\":\"normal answer without a tool call\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
                .to_string(),
        );
    }

    let has_complex_tool_schema =
        body_contains_schema_keyword(&body, &["$defs", "$ref", "oneOf", "const", "format"]);
    if is_forced_tool
        && is_stream
        && has_complex_tool_schema
        && matches!(
            state.responses_mode,
            ResponsesMode::AcceptedComplexSchemaIgnoresTools
        )
    {
        if is_responses {
            return sse(
                "event: response.output_text.delta\ndata: {\"delta\":\"normal answer without a tool call\"}\n\nevent: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"
                    .to_string(),
            );
        }
        return sse(
            "data: {\"choices\":[{\"delta\":{\"content\":\"normal answer without a tool call\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
                .to_string(),
        );
    }

    if is_forced_tool && is_stream {
        let nonce = extract_nonce(&body).unwrap();
        let tool_name = if matches!(state.responses_mode, ResponsesMode::ForcedToolMissingName) {
            ""
        } else {
            "ccsm_protocol_compatibility_probe"
        };
        let tool_arguments = if matches!(
            state.responses_mode,
            ResponsesMode::ForcedToolMalformedArguments
        ) {
            "{".to_string()
        } else if has_complex_tool_schema
            && matches!(
                state.responses_mode,
                ResponsesMode::AcceptedComplexSchemaReturnsEmptyArguments
            )
        {
            "{}".to_string()
        } else {
            json!({"nonce": nonce}).to_string()
        };
        if is_responses {
            let reasoning_item = if matches!(
                state.responses_mode,
                ResponsesMode::SummaryReplayOnly
                    | ResponsesMode::ReasoningTextReplayOnly
                    | ResponsesMode::OmitReasoningReplayOnly
                    | ResponsesMode::GenericOmitReasoningReplayOnly
            ) {
                json!({
                    "id": "rs_fixture",
                    "type": "reasoning",
                    "summary": [{
                        "type": "summary_text",
                        "text": "private tool reasoning"
                    }]
                })
            } else {
                json!({
                    "id": "rs_fixture",
                    "type": "reasoning",
                    "content": [{
                        "type": "reasoning_text",
                        "text": "private tool reasoning"
                    }]
                })
            };
            let terminal = match state.responses_mode {
                ResponsesMode::ForcedToolTerminalMissing => "",
                ResponsesMode::ForcedToolIncomplete => {
                    "event: response.incomplete\ndata: {\"response\":{\"status\":\"incomplete\"}}\n\n"
                }
                ResponsesMode::ForcedToolFailed => {
                    "event: response.failed\ndata: {\"response\":{\"status\":\"failed\"}}\n\n"
                }
                _ => {
                    "event: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"
                }
            };
            let added = if matches!(
                state.responses_mode,
                ResponsesMode::ResponsesToolAddedBeforeDone
            ) {
                format!(
                    "event: response.created\ndata: {}\n\nevent: response.in_progress\ndata: {}\n\nevent: response.output_item.added\ndata: {}\n\n",
                    json!({
                        "type": "response.created",
                        "response": {
                            "status": "in_progress",
                            "output": []
                        }
                    }),
                    json!({
                        "type": "response.in_progress",
                        "response": {
                            "status": "in_progress",
                            "output": []
                        }
                    }),
                    json!({
                        "item": {
                            "id": "fc_fixture",
                            "type": "function_call",
                            "call_id": "call_responses",
                            "name": tool_name,
                            "arguments": "",
                            "status": "in_progress"
                        }
                    })
                )
            } else {
                String::new()
            };
            return sse(format!(
                "{added}event: response.output_item.done\ndata: {}\n\nevent: response.output_item.done\ndata: {}\n\n{terminal}",
                json!({"item": reasoning_item}),
                json!({
                    "item": {
                        "id": "fc_fixture",
                        "type": "function_call",
                        "call_id": "call_responses",
                        "name": tool_name,
                        "arguments": tool_arguments
                    }
                })
            ));
        }
        let finish_reason = if matches!(
            state.responses_mode,
            ResponsesMode::ForcedToolTerminalMissing
        ) {
            ""
        } else {
            ",\"finish_reason\":\"tool_calls\""
        };
        return sse(format!(
            "data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":\"private tool reasoning\"}}}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{\"content\":\"visible before tool\"}}}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{}{finish_reason}}}]}}\n\ndata: [DONE]\n\n",
            json!({
                "tool_calls": [{
                    "index": 0,
                    "id": "call_chat",
                    "type": "function",
                    "function": {
                        "name": tool_name,
                        "arguments": tool_arguments
                    }
                }]
            })
        ));
    }

    if is_stream {
        if is_responses {
            let reasoning_event = match state.responses_mode {
                ResponsesMode::OpaqueReasoning => {
                    "event: response.reasoning_summary_text.delta\ndata: {\"delta\":\"opaque summary\"}\n\n"
                }
                _ => "event: response.reasoning_text.delta\ndata: {\"delta\":\"readable Responses reasoning\"}\n\n",
            };
            let terminal = match state.responses_mode {
                ResponsesMode::ResponsesCompletedWithoutStatus => {
                    "event: response.completed\ndata: {\"response\":{}}\n\n"
                }
                ResponsesMode::StreamingFinalOutputMissing => {
                    "event: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"
                }
                _ => {
                    "event: response.output_text.delta\ndata: {\"delta\":\"CCSM_PROTOCOL_BASELINE_OK\"}\n\nevent: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"
                }
            };
            return sse(format!("{reasoning_event}{terminal}"));
        }
        let chat_terminal = match state.responses_mode {
            ResponsesMode::ChatDoneWithoutFinishReason => "",
            ResponsesMode::ChatUnknownFinishReason => ",\"finish_reason\":\"mystery\"",
            ResponsesMode::ChatLengthTerminal => ",\"finish_reason\":\"length\"",
            ResponsesMode::StreamingFinalOutputMissing => ",\"finish_reason\":\"stop\"",
            _ => ",\"finish_reason\":\"stop\"",
        };
        let chat_delta = if matches!(
            state.responses_mode,
            ResponsesMode::StreamingFinalOutputMissing
        ) {
            "{}"
        } else if matches!(state.responses_mode, ResponsesMode::ChatReasoningOnlyStop) {
            "{\"content\":\"<think>private reasoning only</think>\"}"
        } else {
            "{\"content\":\"CCSM_PROTOCOL_BASELINE_OK\"}"
        };
        return sse(format!(
            "data: {{\"choices\":[{{\"delta\":{chat_delta}{chat_terminal}}}]}}\n\ndata: [DONE]\n\n"
        ));
    }

    if is_responses {
        let reasoning = match state.responses_mode {
            ResponsesMode::OpaqueReasoning | ResponsesMode::BaselineOpaqueSseReadable => {
                vec![json!({
                    "type": "reasoning",
                    "encrypted_content": "fixture-encrypted-reasoning"
                })]
            }
            _ => vec![json!({
                "type": "reasoning",
                "content": [{
                    "type": "reasoning_text",
                    "text": "readable Responses reasoning"
                }]
            })],
        };
        let mut output = reasoning;
        let completion_text = if matches!(state.responses_mode, ResponsesMode::MarkerMismatch) {
            "MODEL_IGNORED_REQUESTED_MARKER"
        } else if is_continuation {
            "CCSM_PROTOCOL_TOOL_DONE"
        } else {
            "CCSM_PROTOCOL_BASELINE_OK"
        };
        output.push(json!({
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": completion_text
            }]
        }));
        Json(json!({
            "id": "resp_fixture",
            "object": "response",
            "status": "completed",
            "output": output
        }))
        .into_response()
    } else {
        let completion_text = if matches!(state.responses_mode, ResponsesMode::MarkerMismatch) {
            "MODEL_IGNORED_REQUESTED_MARKER"
        } else if is_continuation {
            "CCSM_PROTOCOL_TOOL_DONE"
        } else {
            "CCSM_PROTOCOL_BASELINE_OK"
        };
        Json(json!({
            "id": "chatcmpl_fixture",
            "object": "chat.completion",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": completion_text
                },
                "finish_reason": "stop"
            }]
        }))
        .into_response()
    }
}

fn sse(body: String) -> Response {
    let mut response = Response::new(Body::from(body));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    response
}

fn extract_nonce(body: &Value) -> Option<String> {
    fn find(value: &Value) -> Option<String> {
        match value {
            Value::String(text) => text
                .split_once("nonce ")
                .and_then(|(_, tail)| tail.split_once('.'))
                .map(|(nonce, _)| nonce.to_string()),
            Value::Array(values) => values.iter().find_map(find),
            Value::Object(values) => values.values().find_map(find),
            _ => None,
        }
    }
    find(body)
}

fn body_contains_schema_keyword(value: &Value, keywords: &[&str]) -> bool {
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| body_contains_schema_keyword(value, keywords)),
        Value::Object(values) => {
            values.keys().any(|key| keywords.contains(&key.as_str()))
                || values
                    .values()
                    .any(|value| body_contains_schema_keyword(value, keywords))
        }
        _ => false,
    }
}

fn tool_parameter_schemas_contain_keyword(body: &Value, keywords: &[&str]) -> bool {
    body.get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools.iter().any(|tool| {
                tool.get("parameters")
                    .or_else(|| tool.pointer("/function/parameters"))
                    .is_some_and(|schema| body_contains_schema_keyword(schema, keywords))
            })
        })
}

fn candidate(base_url: &str, configured_hint: TransportKind) -> ProbeCandidate {
    ProbeCandidate::new(
        None::<String>,
        None::<String>,
        "qwen3.8",
        "qwen3.8",
        configured_hint,
        base_url,
        "bearer",
    )
    .unwrap()
    .with_bearer_token("fixture-secret")
    .unwrap()
}

#[tokio::test]
async fn probes_all_four_stages_on_both_protocols_and_selects_responses_on_a_tie() {
    let fixture = spawn_fixture(ResponsesMode::Complete).await;
    let client = reqwest::Client::new();
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiChat),
        &client,
    )
    .await;

    assert_eq!(
        result.selected_transport,
        Some(TransportKind::OpenAiResponses)
    );
    assert_eq!(result.readiness, ProbeReadiness::Verified);
    assert_eq!(result.branches.len(), 2);
    assert!(result.branches.iter().all(|branch| {
        branch.assessment.baseline == ProbeStageStatus::Passed
            && branch.assessment.streaming == ProbeStageStatus::Passed
            && branch.assessment.forced_tool == ProbeStageStatus::Passed
            && branch.assessment.continuation == ProbeStageStatus::Passed
    }));

    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 10);
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path.ends_with("/responses"))
            .count(),
        5
    );
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path.ends_with("/chat/completions"))
            .count(),
        5
    );

    let chat_forced = requests
        .iter()
        .find(|(path, body)| {
            path.ends_with("/chat/completions")
                && body.get("stream").and_then(Value::as_bool) == Some(true)
                && body.get("tools").is_some()
        })
        .unwrap();
    assert_eq!(
        chat_forced.1.pointer("/tools/0/function/name"),
        Some(&json!("ccsm_protocol_compatibility_probe"))
    );
    assert!(chat_forced.1.pointer("/tools/0/name").is_none());

    let chat_continuation = requests
        .iter()
        .find(|(path, body)| {
            path.ends_with("/chat/completions")
                && body
                    .get("messages")
                    .and_then(Value::as_array)
                    .is_some_and(|messages| {
                        messages.iter().any(|message| {
                            message.get("role").and_then(Value::as_str) == Some("tool")
                        })
                    })
        })
        .unwrap();
    assert!(chat_continuation
        .1
        .get("messages")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .any(|message| message.get("tool_calls").is_some()));

    let responses_continuation = requests
        .iter()
        .find(|(path, body)| {
            path.ends_with("/responses")
                && body
                    .get("input")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                        })
                    })
        })
        .unwrap();
    let responses_input = responses_continuation.1["input"].as_array().unwrap();
    assert!(responses_input
        .iter()
        .any(|item| item["id"] == "rs_fixture"));
    assert!(responses_input
        .iter()
        .any(|item| item["id"] == "fc_fixture"));
}

#[tokio::test]
async fn retries_only_explicit_schema_rejections_with_moonshot_dialect_and_records_it() {
    let fixture = spawn_fixture(ResponsesMode::MoonshotToolSchemaOnly).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(
        result.readiness,
        ProbeReadiness::Verified,
        "probe result: {result:#?}"
    );
    assert!(result.branches.iter().all(|branch| {
        branch.tool_schema_dialect == super::ToolSchemaDialect::MoonshotMfjs
            && branch.assessment.forced_tool == ProbeStageStatus::Passed
    }));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 12);
    assert!(
        requests
            .iter()
            .filter(|(_, body)| {
                body.get("tools").is_some()
                    && !body_contains_schema_keyword(
                        body,
                        &["$defs", "$ref", "oneOf", "const", "format"],
                    )
            })
            .count()
            >= 2
    );
}

#[tokio::test]
async fn generic_forced_tool_400_negotiates_the_moonshot_schema_dialect_once() {
    let fixture = spawn_fixture(ResponsesMode::GenericMoonshotToolSchemaOnly).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(result.readiness, ProbeReadiness::Verified);
    assert!(result.branches.iter().all(|branch| {
        branch.tool_schema_dialect == super::ToolSchemaDialect::MoonshotMfjs
            && branch.assessment.forced_tool == ProbeStageStatus::Passed
    }));
    assert_eq!(fixture.requests.lock().unwrap().len(), 12);
}

#[tokio::test]
async fn selects_chat_when_responses_rejects_codex_custom_tools() {
    let fixture = spawn_fixture(ResponsesMode::ResponsesCustomToolUnsupported).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(result.selected_transport, Some(TransportKind::OpenAiChat));
    assert_eq!(result.readiness, ProbeReadiness::Verified);
    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .expect("Responses branch");
    assert_eq!(
        responses.assessment.forced_tool,
        ProbeStageStatus::Unsupported
    );
    let chat = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiChat)
        .expect("Chat branch");
    assert_eq!(chat.assessment.forced_tool, ProbeStageStatus::Passed);
    assert_eq!(chat.assessment.continuation, ProbeStageStatus::Passed);
}

#[tokio::test]
async fn responses_continuation_records_native_summary_replay_when_accepted() {
    let fixture = spawn_fixture(ResponsesMode::SummaryReplayOnly).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(responses.history_replay, super::HistoryReplay::NativeOnly);
    assert_eq!(responses.assessment.continuation, ProbeStageStatus::Passed);
}

#[tokio::test]
async fn responses_native_replay_preserves_the_upstream_reasoning_item_unchanged() {
    let fixture = spawn_fixture(ResponsesMode::Complete).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .expect("Responses branch");
    assert_eq!(responses.history_replay, super::HistoryReplay::NativeOnly);
    assert_eq!(responses.assessment.continuation, ProbeStageStatus::Passed);
    let requests = fixture.requests.lock().unwrap();
    let continuation = requests
        .iter()
        .find(|(path, body)| {
            path.ends_with("/responses")
                && body
                    .get("input")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                        })
                    })
        })
        .map(|(_, body)| body)
        .expect("Responses continuation request");
    let reasoning = continuation["input"]
        .as_array()
        .expect("continuation input")
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"))
        .expect("native reasoning item");

    assert_eq!(
        reasoning["content"],
        json!([{"type": "reasoning_text", "text": "private tool reasoning"}])
    );
    assert!(reasoning.get("summary").is_none());
}

#[tokio::test]
async fn responses_continuation_falls_back_to_reasoning_text_replay_when_required() {
    let fixture = spawn_fixture(ResponsesMode::ReasoningTextReplayOnly).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(
        responses.history_replay,
        super::HistoryReplay::ResponsesReasoningTextContent
    );
    assert_eq!(responses.assessment.continuation, ProbeStageStatus::Passed);
    let requests = fixture.requests.lock().unwrap();
    let continuations = requests
        .iter()
        .filter(|(path, body)| {
            path.ends_with("/responses")
                && body
                    .get("input")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                        })
                    })
        })
        .map(|(_, body)| body)
        .collect::<Vec<_>>();
    assert_eq!(continuations.len(), 2);
    assert_eq!(
        continuations[0]["input"][1]["summary"],
        json!([{"type": "summary_text", "text": "private tool reasoning"}])
    );
    assert!(continuations[0]["input"][1].get("content").is_none());
    assert_eq!(
        continuations[1]["input"][1]["content"],
        json!([{"type": "reasoning_text", "text": "private tool reasoning"}])
    );
    assert_eq!(continuations[1]["input"][1]["summary"], json!([]));
}

#[tokio::test]
async fn responses_continuation_omits_reasoning_only_after_both_replay_shapes_are_rejected() {
    let fixture = spawn_fixture(ResponsesMode::OmitReasoningReplayOnly).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(responses.history_replay, super::HistoryReplay::Omit);
    assert_eq!(responses.assessment.continuation, ProbeStageStatus::Passed);
    let requests = fixture.requests.lock().unwrap();
    let final_continuation = requests
        .iter()
        .filter(|(path, body)| {
            path.ends_with("/responses")
                && body
                    .get("input")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                        })
                    })
        })
        .map(|(_, body)| body)
        .last()
        .expect("final omit continuation");
    let input = final_continuation["input"]
        .as_array()
        .expect("continuation input");
    assert!(!input
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("reasoning")));
    assert!(input
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("function_call")));
    assert!(input
        .iter()
        .any(|item| { item.get("type").and_then(Value::as_str) == Some("function_call_output") }));
}

#[tokio::test]
async fn generic_continuation_400_does_not_downgrade_reasoning_replay_shape() {
    let fixture = spawn_fixture(ResponsesMode::GenericOmitReasoningReplayOnly).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(responses.history_replay, super::HistoryReplay::NativeOnly);
    assert_eq!(responses.assessment.continuation, ProbeStageStatus::Failed);
    let requests = fixture.requests.lock().unwrap();
    let responses_continuations = requests
        .iter()
        .filter(|(path, body)| {
            path.ends_with("/responses")
                && body
                    .get("input")
                    .and_then(Value::as_array)
                    .is_some_and(|items| {
                        items.iter().any(|item| {
                            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                        })
                    })
        })
        .count();
    assert_eq!(responses_continuations, 1);
}

#[tokio::test]
async fn reports_ordered_redacted_progress_for_every_deep_probe_stage() {
    let fixture = spawn_fixture(ResponsesMode::Complete).await;
    let client = reqwest::Client::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let reported = events.clone();

    let result = run_protocol_compatibility_probe_with_reporter(
        candidate(&fixture.base_url, TransportKind::OpenAiChat),
        &client,
        move |event| reported.lock().unwrap().push(event),
    )
    .await;

    assert_eq!(result.readiness, ProbeReadiness::Verified);
    let events = events.lock().unwrap();
    for transport in [TransportKind::OpenAiResponses, TransportKind::OpenAiChat] {
        for stage in [
            ProbeProgressStage::Baseline,
            ProbeProgressStage::Streaming,
            ProbeProgressStage::ForcedTool,
            ProbeProgressStage::Continuation,
        ] {
            assert!(events.iter().any(|event| matches!(
                event,
                ProtocolProbeProgressEvent::StageStarted {
                    model,
                    transport: event_transport,
                    stage: event_stage,
                } if model == "qwen3.8" && *event_transport == transport && *event_stage == stage
            )));
            assert!(events.iter().any(|event| matches!(
                event,
                ProtocolProbeProgressEvent::StageFinished {
                    model,
                    transport: event_transport,
                    stage: event_stage,
                    ..
                } if model == "qwen3.8" && *event_transport == transport && *event_stage == stage
            )));
        }
        assert!(events.iter().any(|event| matches!(
            event,
            ProtocolProbeProgressEvent::ReasoningClassified {
                model,
                transport: event_transport,
                stage: ProbeProgressStage::Reasoning,
                reasoning_semantic: ReasoningSemantic::Readable,
                reasoning_source,
            } if model == "qwen3.8"
                && *event_transport == transport
                && *reasoning_source != ReasoningSource::None
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ProtocolProbeProgressEvent::BranchFinished {
                model,
                transport: event_transport,
                ..
            } if model == "qwen3.8" && *event_transport == transport
        )));
    }

    let serialized = serde_json::to_string(&*events).unwrap();
    assert!(!serialized.contains("fixture-secret"));
    assert!(!serialized.contains("private tool reasoning"));
    assert!(!serialized.contains("CCSM_PROTOCOL"));
    assert!(!serialized.contains(&fixture.base_url));
}

#[tokio::test]
async fn parseable_but_invalid_success_json_does_not_pass_responses_baseline() {
    let fixture = spawn_fixture(ResponsesMode::InvalidSuccessfulJson).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(responses.assessment.baseline, ProbeStageStatus::Failed);
}

#[tokio::test]
async fn incomplete_success_json_does_not_pass_responses_continuation() {
    let fixture = spawn_fixture(ResponsesMode::IncompleteContinuation).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(responses.assessment.continuation, ProbeStageStatus::Failed);
}

#[tokio::test]
async fn chat_done_without_finish_reason_does_not_pass_streaming() {
    let fixture = spawn_fixture(ResponsesMode::ChatDoneWithoutFinishReason).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiChat),
        &reqwest::Client::new(),
    )
    .await;

    let chat = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiChat)
        .unwrap();
    assert_eq!(chat.assessment.streaming, ProbeStageStatus::Failed);
}

#[tokio::test]
async fn chat_unknown_or_incomplete_finish_reason_does_not_pass_streaming() {
    for mode in [
        ResponsesMode::ChatUnknownFinishReason,
        ResponsesMode::ChatLengthTerminal,
    ] {
        let fixture = spawn_fixture(mode).await;
        let result = run_protocol_compatibility_probe(
            candidate(&fixture.base_url, TransportKind::OpenAiChat),
            &reqwest::Client::new(),
        )
        .await;

        let chat = result
            .branches
            .iter()
            .find(|branch| branch.assessment.transport == TransportKind::OpenAiChat)
            .unwrap();
        assert_eq!(chat.assessment.streaming, ProbeStageStatus::Failed);
    }
}

#[tokio::test]
async fn chat_reasoning_only_stop_without_visible_answer_does_not_pass_streaming() {
    let fixture = spawn_fixture(ResponsesMode::ChatReasoningOnlyStop).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiChat),
        &reqwest::Client::new(),
    )
    .await;

    let chat = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiChat)
        .unwrap();
    assert_eq!(chat.assessment.streaming, ProbeStageStatus::Failed);
}

#[tokio::test]
async fn responses_completed_without_completed_status_does_not_pass_streaming() {
    let fixture = spawn_fixture(ResponsesMode::ResponsesCompletedWithoutStatus).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(responses.assessment.streaming, ProbeStageStatus::Failed);
}

#[tokio::test]
async fn terminal_without_final_output_does_not_pass_streaming() {
    let fixture = spawn_fixture(ResponsesMode::StreamingFinalOutputMissing).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert!(result
        .branches
        .iter()
        .all(|branch| { branch.assessment.streaming == ProbeStageStatus::Failed }));
}

#[tokio::test]
async fn forced_tool_call_without_a_terminal_does_not_pass_either_protocol() {
    let fixture = spawn_fixture(ResponsesMode::ForcedToolTerminalMissing).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert!(result.branches.iter().all(|branch| {
        branch.assessment.forced_tool == ProbeStageStatus::Failed
            && branch.assessment.continuation == ProbeStageStatus::Skipped
    }));
}

#[tokio::test]
async fn forced_tool_incomplete_or_failed_terminal_never_passes_responses() {
    for mode in [
        ResponsesMode::ForcedToolIncomplete,
        ResponsesMode::ForcedToolFailed,
    ] {
        let fixture = spawn_fixture(mode).await;
        let result = run_protocol_compatibility_probe(
            candidate(&fixture.base_url, TransportKind::OpenAiResponses),
            &reqwest::Client::new(),
        )
        .await;

        let responses = result
            .branches
            .iter()
            .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
            .unwrap();
        assert_eq!(responses.assessment.forced_tool, ProbeStageStatus::Failed);
        assert_eq!(responses.assessment.continuation, ProbeStageStatus::Skipped);
    }
}

#[tokio::test]
async fn structurally_incomplete_forced_tool_call_is_failed_not_unsupported() {
    let fixture = spawn_fixture(ResponsesMode::ForcedToolMissingName).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert!(result.branches.iter().all(|branch| {
        branch.assessment.forced_tool == ProbeStageStatus::Failed
            && branch.assessment.continuation == ProbeStageStatus::Skipped
    }));
}

#[tokio::test]
async fn responses_tool_extraction_waits_for_done_after_in_progress_added() {
    let fixture = spawn_fixture(ResponsesMode::ResponsesToolAddedBeforeDone).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(responses.assessment.forced_tool, ProbeStageStatus::Passed);
    assert_eq!(responses.assessment.continuation, ProbeStageStatus::Passed);
}

#[tokio::test]
async fn complete_auto_response_without_a_tool_retries_required_once_per_protocol() {
    let fixture = spawn_fixture(ResponsesMode::AutoToolIgnoredRequiredSucceeds).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(result.readiness, ProbeReadiness::Verified);
    assert!(result.branches.iter().all(|branch| {
        branch.assessment.forced_tool == ProbeStageStatus::Passed
            && branch.assessment.continuation == ProbeStageStatus::Passed
    }));

    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 12);
    assert_eq!(
        requests
            .iter()
            .filter(|(_, body)| body.get("tool_choice").and_then(Value::as_str) == Some("required"))
            .count(),
        2
    );
}

#[tokio::test]
async fn accepted_complex_schema_without_a_tool_negotiates_moonshot_after_required() {
    let fixture = spawn_fixture(ResponsesMode::AcceptedComplexSchemaIgnoresTools).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(result.readiness, ProbeReadiness::Verified, "{result:#?}");
    assert!(result.branches.iter().all(|branch| {
        branch.tool_schema_dialect == super::ToolSchemaDialect::MoonshotMfjs
            && branch.assessment.forced_tool == ProbeStageStatus::Passed
            && branch.assessment.continuation == ProbeStageStatus::Passed
    }));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 14);
    assert_eq!(
        requests
            .iter()
            .filter(|(_, body)| body.get("tool_choice").and_then(Value::as_str) == Some("required"))
            .count(),
        4
    );
}

#[tokio::test]
async fn accepted_complex_schema_with_invalid_arguments_negotiates_without_accepting_it() {
    let fixture = spawn_fixture(ResponsesMode::AcceptedComplexSchemaReturnsEmptyArguments).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(result.readiness, ProbeReadiness::Verified, "{result:#?}");
    assert!(result.branches.iter().all(|branch| {
        branch.tool_schema_dialect == super::ToolSchemaDialect::MoonshotMfjs
            && branch.assessment.forced_tool == ProbeStageStatus::Passed
            && branch.assessment.continuation == ProbeStageStatus::Passed
    }));
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 12);
    assert_eq!(
        requests
            .iter()
            .filter(|(_, body)| body.get("tool_choice").and_then(Value::as_str) == Some("required"))
            .count(),
        0
    );
}

#[tokio::test]
async fn malformed_forced_tool_arguments_are_failed_not_verified() {
    let fixture = spawn_fixture(ResponsesMode::ForcedToolMalformedArguments).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert!(result.branches.iter().all(|branch| {
        branch.assessment.forced_tool == ProbeStageStatus::Failed
            && branch.assessment.continuation == ProbeStageStatus::Skipped
    }));
}

#[tokio::test]
async fn continuation_marker_mismatch_does_not_verify_tool_result_consumption() {
    let fixture = spawn_fixture(ResponsesMode::MarkerMismatch).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(result.readiness, ProbeReadiness::Partial);
    assert!(result.branches.iter().all(|branch| {
        branch.assessment.baseline == ProbeStageStatus::Passed
            && branch.assessment.continuation == ProbeStageStatus::Failed
    }));
}

#[tokio::test]
async fn native_responses_summary_overrides_opaque_baseline_evidence() {
    let fixture = spawn_fixture(ResponsesMode::OpaqueReasoning).await;
    let client = reqwest::Client::new();
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &client,
    )
    .await;

    assert_eq!(
        result.selected_transport,
        Some(TransportKind::OpenAiResponses)
    );
    assert_eq!(result.readiness, ProbeReadiness::Verified);
    assert_eq!(
        result
            .branches
            .iter()
            .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
            .unwrap()
            .reasoning_shape
            .semantic,
        super::ReasoningSemantic::Summary
    );
    assert_eq!(
        result
            .branches
            .iter()
            .find(|branch| branch.assessment.transport == TransportKind::OpenAiChat)
            .unwrap()
            .reasoning_shape
            .semantic,
        super::ReasoningSemantic::Readable
    );
}

#[tokio::test]
async fn native_responses_raw_reasoning_overrides_opaque_baseline_evidence() {
    let fixture = spawn_fixture(ResponsesMode::BaselineOpaqueSseReadable).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(
        result.selected_transport,
        Some(TransportKind::OpenAiResponses)
    );
    assert_eq!(result.readiness, ProbeReadiness::Verified);
    assert_eq!(
        result
            .branches
            .iter()
            .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
            .unwrap()
            .reasoning_shape
            .semantic,
        super::ReasoningSemantic::Readable
    );
}

#[tokio::test]
async fn responses_baseline_rejection_stops_only_that_branch_and_chat_still_verifies() {
    let fixture = spawn_fixture(ResponsesMode::BaselineUnsupported).await;
    let client = reqwest::Client::new();
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &client,
    )
    .await;

    assert_eq!(result.selected_transport, Some(TransportKind::OpenAiChat));
    assert_eq!(result.readiness, ProbeReadiness::Verified);
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path.ends_with("/responses"))
            .count(),
        1
    );
}

#[tokio::test]
async fn http_521_is_redacted_and_skips_reasoning_instead_of_marking_it_unsupported() {
    let fixture = spawn_fixture(ResponsesMode::UpstreamUnavailable).await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let reported = events.clone();
    let result = run_protocol_compatibility_probe_with_reporter(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
        move |event| reported.lock().unwrap().push(event),
    )
    .await;

    assert_eq!(result.selected_transport, None);
    assert_eq!(result.readiness, ProbeReadiness::Unverified);
    let result_json = serde_json::to_value(&result).unwrap();
    for branch in result_json["branches"].as_array().unwrap() {
        assert_eq!(branch["assessment"]["baseline"], json!("failed"));
        assert_eq!(
            branch["failures"],
            json!([{
                "stage": "baseline",
                "kind": "http_status",
                "status_code": 521
            }])
        );
    }

    let events = events.lock().unwrap();
    let event_json = serde_json::to_value(&*events).unwrap();
    assert!(event_json.as_array().unwrap().iter().any(|event| {
        event["kind"] == "stage_finished"
            && event["stage"] == "baseline"
            && event["stageStatus"] == "failed"
            && event["failure"]
                == json!({
                    "stage": "baseline",
                    "kind": "http_status",
                    "status_code": 521
                })
    }));
    assert!(event_json.as_array().unwrap().iter().any(|event| {
        event["kind"] == "stage_finished"
            && event["stage"] == "reasoning"
            && event["stageStatus"] == "skipped"
    }));

    let serialized = serde_json::to_string(&(&result, &*events)).unwrap();
    assert!(!serialized.contains("private upstream response body"));
    assert!(!serialized.contains("fixture-secret"));
    assert!(!serialized.contains(&fixture.base_url));
}

#[tokio::test]
async fn complete_chat_beats_responses_when_responses_cannot_force_tools() {
    let fixture = spawn_fixture(ResponsesMode::ToolUnsupported).await;
    let client = reqwest::Client::new();
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &client,
    )
    .await;

    assert_eq!(result.selected_transport, Some(TransportKind::OpenAiChat));
    assert_eq!(result.readiness, ProbeReadiness::Verified);
    let responses = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
        .unwrap();
    assert_eq!(
        responses.assessment.forced_tool,
        ProbeStageStatus::Unsupported
    );
    assert_eq!(responses.assessment.continuation, ProbeStageStatus::Skipped);
    let chat = result
        .branches
        .iter()
        .find(|branch| branch.assessment.transport == TransportKind::OpenAiChat)
        .unwrap();
    assert_eq!(
        chat.reasoning_shape.pre_tool_visible_content,
        PreToolVisibleContent::Present
    );
}

#[tokio::test]
async fn verified_chat_probe_projects_qwen_reasoning_as_raw_for_streaming_and_json() {
    let fixture = spawn_fixture(ResponsesMode::BaselineUnsupported).await;
    let client = reqwest::Client::new();
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &client,
    )
    .await;

    assert_eq!(result.selected_transport, Some(TransportKind::OpenAiChat));
    assert_eq!(result.readiness, ProbeReadiness::Verified);

    let target = ProbeTargetKey::new(
        "fixture-provider",
        None::<String>,
        "qwen3.8",
        "qwen3.8",
        TransportKind::OpenAiChat,
        &format!("{}/v1/chat/completions", fixture.base_url),
        "bearer",
    )
    .unwrap();
    let record = ProtocolCompatibilityRecord::new(target, result, 100, 200);
    let projection = record.automatic_reasoning_projection(150);
    assert_eq!(projection, ReasoningProjection::RawReasoningText);

    let non_streaming = chat_completion_to_response_with_context_and_projection(
        json!({
            "id": "chatcmpl_fixture",
            "model": "qwen3.8",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "fixture reasoning",
                    "content": "fixture answer"
                },
                "finish_reason": "stop"
            }]
        }),
        &CodexToolContext::default(),
        projection,
    )
    .unwrap();
    assert_eq!(non_streaming["output"][0]["type"], "reasoning");
    assert_eq!(
        non_streaming["output"][0]["content"][0]["type"],
        "reasoning_text"
    );
    assert!(non_streaming["output"][0]
        .get("summary")
        .is_some_and(Value::is_array));
    assert_eq!(non_streaming["output"][0]["summary"], json!([]));
    assert_eq!(non_streaming["output"][1]["type"], "message");
    assert_eq!(
        non_streaming["output"][1]["content"][0]["type"],
        "output_text"
    );

    let upstream = stream::iter(vec![
        Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: {\"id\":\"chatcmpl_fixture\",\"model\":\"qwen3.8\",\"choices\":[{\"delta\":{\"reasoning_content\":\"fixture reasoning\"}}]}\n\n")),
        Ok(Bytes::from_static(b"data: {\"id\":\"chatcmpl_fixture\",\"model\":\"qwen3.8\",\"choices\":[{\"delta\":{\"content\":\"fixture answer\"},\"finish_reason\":\"stop\"}]}\n\n")),
        Ok(Bytes::from_static(b"data: [DONE]\n\n")),
    ]);
    let streaming = create_responses_sse_stream_from_chat_with_context_and_projection(
        upstream,
        CodexToolContext::default(),
        projection,
    )
    .map(|chunk| chunk.unwrap())
    .collect::<Vec<_>>()
    .await;
    let streaming = String::from_utf8(streaming.concat()).unwrap();
    assert!(streaming.contains("event: response.reasoning_text.delta"));
    assert!(streaming.contains("event: response.output_text.delta"));
    assert!(!streaming.contains("response.reasoning_summary"));
}
