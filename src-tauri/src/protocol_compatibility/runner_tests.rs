use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::State,
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use tokio::{net::TcpListener, task::JoinHandle};

use super::{
    run_protocol_compatibility_probe, PreToolVisibleContent, ProbeCandidate, ProbeReadiness,
    ProbeStageStatus, TransportKind,
};

#[derive(Clone, Copy)]
enum ResponsesMode {
    Complete,
    BaselineUnsupported,
    ToolUnsupported,
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
                "event: response.output_item.done\ndata: {}\n\nevent: response.completed\ndata: {{\"response\":{{\"status\":\"completed\"}}}}\n\n",
                json!({
                    "item": {
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
            return sse(
                "event: response.output_text.delta\ndata: {\"delta\":\"baseline\"}\n\nevent: response.completed\ndata: {\"response\":{\"status\":\"completed\"}}\n\n"
                    .to_string(),
            );
        }
        return sse(
            "data: {\"choices\":[{\"delta\":{\"content\":\"baseline\"}}]}\n\ndata: [DONE]\n\n"
                .to_string(),
        );
    }

    if is_responses {
        Json(json!({
            "id": "resp_fixture",
            "object": "response",
            "status": "completed",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{
                    "type": "output_text",
                    "text": if is_continuation { "tool complete" } else { "baseline" }
                }]
            }]
        }))
        .into_response()
    } else {
        Json(json!({
            "id": "chatcmpl_fixture",
            "object": "chat.completion",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": if is_continuation { "tool complete" } else { "baseline" }
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
