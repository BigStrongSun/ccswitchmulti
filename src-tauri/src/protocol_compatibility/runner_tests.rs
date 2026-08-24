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
    run_protocol_compatibility_probe, PreToolVisibleContent, ProbeCandidate, ProbeReadiness,
    ProbeStageStatus, ProbeTargetKey, ProtocolCompatibilityRecord, ReasoningProjection,
    TransportKind,
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
    BaselineUnsupported,
    ToolUnsupported,
    InvalidSuccessfulJson,
    IncompleteContinuation,
    MarkerMismatch,
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

    if is_forced_tool && is_stream {
        let nonce = extract_nonce(&body).unwrap();
        if is_responses {
            return sse(format!(
                "event: response.output_item.done\ndata: {}\n\nevent: response.output_item.done\ndata: {}\n\nevent: response.completed\ndata: {{\"response\":{{\"status\":\"completed\"}}}}\n\n",
                json!({
                    "item": {
                        "id": "rs_fixture",
                        "type": "reasoning",
                        "content": [{
                            "type": "reasoning_text",
                            "text": "private tool reasoning"
                        }]
                    }
                }),
                json!({
                    "item": {
                        "id": "fc_fixture",
                        "type": "function_call",
                        "call_id": "call_responses",
                        "name": "ccsm_protocol_compatibility_probe",
                        "arguments": json!({"nonce": nonce}).to_string()
                    }
                })
            ));
        }
        return sse(format!(
            "data: {{\"choices\":[{{\"delta\":{{\"reasoning_content\":\"private tool reasoning\"}}}}]}}\n\ndata: {{\"choices\":[{{\"delta\":{{\"content\":\"visible before tool\"}}}}]}}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({
                "choices": [{
                    "delta": {
                        "tool_calls": [{
                            "index": 0,
                            "id": "call_chat",
                            "type": "function",
                            "function": {
                                "name": "ccsm_protocol_compatibility_probe",
                                "arguments": json!({"nonce": nonce}).to_string()
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
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
            return sse(
                format!(
                    "{reasoning_event}{}",
                    "event: response.output_text.delta\ndata: {\"delta\":\"CCSM_PROTOCOL_BASELINE_OK\"}\n\nevent: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"
                ),
            );
        }
        return sse(
            "data: {\"choices\":[{\"delta\":{\"content\":\"CCSM_PROTOCOL_BASELINE_OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
                .to_string(),
        );
    }

    if is_responses {
        let reasoning = match state.responses_mode {
            ResponsesMode::OpaqueReasoning => vec![json!({
                "type": "reasoning",
                "encrypted_content": "fixture-encrypted-reasoning"
            })],
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
    assert_eq!(requests.len(), 8);
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path.ends_with("/responses"))
            .count(),
        4
    );
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path.ends_with("/chat/completions"))
            .count(),
        4
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
async fn marker_mismatch_is_diagnostic_and_does_not_fail_protocol_capabilities() {
    let fixture = spawn_fixture(ResponsesMode::MarkerMismatch).await;
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &reqwest::Client::new(),
    )
    .await;

    assert_eq!(result.readiness, ProbeReadiness::Verified);
    assert!(result.branches.iter().all(|branch| {
        branch.assessment.baseline == ProbeStageStatus::Passed
            && branch.assessment.continuation == ProbeStageStatus::Passed
    }));
}

#[tokio::test]
async fn runner_selects_readable_chat_from_real_branch_shapes_when_responses_is_opaque() {
    let fixture = spawn_fixture(ResponsesMode::OpaqueReasoning).await;
    let client = reqwest::Client::new();
    let result = run_protocol_compatibility_probe(
        candidate(&fixture.base_url, TransportKind::OpenAiResponses),
        &client,
    )
    .await;

    assert_eq!(result.selected_transport, Some(TransportKind::OpenAiChat));
    assert_eq!(result.readiness, ProbeReadiness::Verified);
    assert_eq!(
        result
            .branches
            .iter()
            .find(|branch| branch.assessment.transport == TransportKind::OpenAiResponses)
            .unwrap()
            .reasoning_shape
            .semantic,
        super::ReasoningSemantic::Opaque
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
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _)| path.ends_with("/responses"))
            .count(),
        1
    );
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
