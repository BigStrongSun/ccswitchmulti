use std::{convert::Infallible, time::Duration};

use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use serde_json::json;
use tokio::{net::TcpListener, task::JoinHandle};

use super::{
    capture_transport_probe, classify_captured_reasoning_shape, PreToolVisibleContent,
    ProbeCaptureError, ReasoningSemantic, ReasoningSource,
};

struct FixtureServer {
    base_url: String,
    task: JoinHandle<()>,
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn spawn_fixture() -> FixtureServer {
    let app = Router::new()
        .route("/json", post(json_response))
        .route("/sse", post(sse_response))
        .route("/sse-data-type", post(sse_data_type_response))
        .route("/failure", post(http_failure))
        .route("/tool-schema-failure", post(tool_schema_failure))
        .route("/reasoning-replay-failure", post(reasoning_replay_failure))
        .route("/rate-limit-failure", post(rate_limit_failure))
        .route("/service-failure", post(service_failure))
        .route("/slow", get(slow_response));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    FixtureServer {
        base_url: format!("http://{address}"),
        task,
    }
}

async fn json_response() -> impl IntoResponse {
    Json(json!({
        "choices": [{
            "message": {
                "content": "CCSM_PROTOCOL_BASELINE_OK",
                "reasoning_content": "private-json-reasoning"
            }
        }]
    }))
}

async fn sse_response() -> Response {
    let stream = async_stream::stream! {
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"event: response.reasoning_text.delta\r\ndata: {\"delta\":\"private-",
        ));
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"sse-reasoning\"}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_1\"}]}}]}\n\n",
        ));
        yield Ok::<Bytes, Infallible>(Bytes::from_static(b"data: [DONE]\n\n"));
    };
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response
}

async fn sse_data_type_response() -> Response {
    let stream = async_stream::stream! {
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"data: {\"type\":\"response.reasoning_text.delta\",\"delta\":\"inspect\"}\n\n",
        ));
        yield Ok::<Bytes, Infallible>(Bytes::from_static(
            b"data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"probe\",\"arguments\":\"{}\"}}\n\n",
        ));
        yield Ok::<Bytes, Infallible>(Bytes::from_static(b"data: [DONE]\n\n"));
    };
    let mut response = Response::new(Body::from_stream(stream));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    response
}

async fn http_failure() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": {"message": "secret-upstream-error-body"}})),
    )
}

async fn tool_schema_failure() -> impl IntoResponse {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(
            json!({"error": {"message": "tools.function.parameters is not a valid moonshot flavored json schema at $defs.node"}}),
        ),
    )
}

async fn reasoning_replay_failure() -> impl IntoResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(
            json!({"error": {"message": "Invalid input reasoning item: reasoning_text content must be passed back"}}),
        ),
    )
}

async fn rate_limit_failure() -> impl IntoResponse {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(
            json!({"error": {"message": "invalid tool schema and reasoning content during temporary overload"}}),
        ),
    )
}

async fn service_failure() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(
            json!({"error": {"message": "invalid tool schema and reasoning content while the service is unavailable"}}),
        ),
    )
}

async fn slow_response() -> impl IntoResponse {
    tokio::time::sleep(Duration::from_millis(150)).await;
    Json(json!({"ok": true}))
}

#[tokio::test]
async fn captures_json_shape_without_exposing_raw_payload_in_debug() {
    let fixture = spawn_fixture().await;
    let client = reqwest::Client::new();
    let exchange = capture_transport_probe(
        client.post(format!("{}/json", fixture.base_url)),
        Duration::from_secs(1),
    )
    .await
    .unwrap();

    assert_eq!(exchange.status_code(), 200);
    assert_eq!(exchange.payload_count(), 1);
    assert!(!exchange.saw_done());
    assert_eq!(exchange.evidence().status_code, 200);
    assert!(exchange
        .evidence()
        .paths
        .contains(&"choices[].message.reasoning_content".to_string()));

    let debug = format!("{exchange:?}");
    assert!(!debug.contains("private-json-reasoning"));
    assert!(!debug.contains("CCSM_PROTOCOL_BASELINE_OK"));
}

#[tokio::test]
async fn captures_chunked_sse_event_names_data_only_frames_and_done() {
    let fixture = spawn_fixture().await;
    let client = reqwest::Client::new();
    let exchange = capture_transport_probe(
        client.post(format!("{}/sse", fixture.base_url)),
        Duration::from_secs(1),
    )
    .await
    .unwrap();

    assert_eq!(exchange.payload_count(), 3);
    assert!(exchange.saw_done());
    assert_eq!(
        exchange.evidence().event_types,
        vec!["response.reasoning_text.delta", "data", "data", "done"]
    );
    let shape = classify_captured_reasoning_shape(&exchange);
    assert_eq!(shape.semantic, ReasoningSemantic::Readable);
    assert_eq!(shape.source, ReasoningSource::NativeResponses);
    assert_eq!(
        shape.pre_tool_visible_content,
        PreToolVisibleContent::Present
    );
    assert!(!format!("{exchange:?}").contains("private-sse-reasoning"));
}

#[tokio::test]
async fn normalizes_data_only_responses_type_into_the_captured_event_name() {
    let fixture = spawn_fixture().await;
    let exchange = capture_transport_probe(
        reqwest::Client::new().post(format!("{}/sse-data-type", fixture.base_url)),
        Duration::from_secs(1),
    )
    .await
    .unwrap();

    assert_eq!(
        exchange
            .payloads()
            .iter()
            .map(|payload| payload.event_type.as_deref())
            .collect::<Vec<_>>(),
        vec![
            Some("response.reasoning_text.delta"),
            Some("response.output_item.done")
        ]
    );
    let shape = classify_captured_reasoning_shape(&exchange);
    assert_eq!(shape.semantic, ReasoningSemantic::Readable);
    assert_eq!(shape.source, ReasoningSource::NativeResponses);
}

#[tokio::test]
async fn http_failure_is_structural_and_never_contains_the_raw_error_body() {
    let fixture = spawn_fixture().await;
    let client = reqwest::Client::new();
    let error = capture_transport_probe(
        client.post(format!("{}/failure?api_key=secret-key", fixture.base_url)),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();

    assert_eq!(error, ProbeCaptureError::HttpStatus { status_code: 401 });
    let debug = format!("{error:?}");
    assert!(!debug.contains("secret-upstream-error-body"));
    assert!(!debug.contains("secret-key"));
}

#[tokio::test]
async fn classifies_only_explicit_tool_schema_rejections_for_safe_dialect_retry() {
    let fixture = spawn_fixture().await;
    let error = capture_transport_probe(
        reqwest::Client::new().post(format!("{}/tool-schema-failure", fixture.base_url)),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        ProbeCaptureError::ToolSchemaRejected { status_code: 422 }
    );
    assert!(!format!("{error:?}").contains("$defs.node"));
}

#[tokio::test]
async fn classifies_only_explicit_reasoning_replay_rejections_for_safe_retry() {
    let fixture = spawn_fixture().await;
    let error = capture_transport_probe(
        reqwest::Client::new().post(format!("{}/reasoning-replay-failure", fixture.base_url)),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();

    assert_eq!(
        error,
        ProbeCaptureError::ReasoningReplayRejected { status_code: 400 }
    );
    assert!(!format!("{error:?}").contains("reasoning_text content"));
}

#[tokio::test]
async fn availability_errors_never_trigger_schema_or_history_downgrades() {
    let fixture = spawn_fixture().await;
    for (path, status_code) in [("rate-limit-failure", 429), ("service-failure", 503)] {
        let error = capture_transport_probe(
            reqwest::Client::new().post(format!("{}/{path}", fixture.base_url)),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

        assert_eq!(error, ProbeCaptureError::HttpStatus { status_code });
    }
}

#[tokio::test]
async fn response_deadline_aborts_without_returning_request_details() {
    let fixture = spawn_fixture().await;
    let client = reqwest::Client::new();
    let error = capture_transport_probe(
        client.get(format!("{}/slow?token=do-not-log", fixture.base_url)),
        Duration::from_millis(20),
    )
    .await
    .unwrap_err();

    assert_eq!(error, ProbeCaptureError::Timeout);
    assert!(!format!("{error:?}").contains("do-not-log"));
}
