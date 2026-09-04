use super::{codex_error_capture::redact, error::ProxyError, hyper_client::ProxyResponse};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, LazyLock,
    },
};

const BODY_LIMIT: usize = 64 * 1024;
const FILE_LIMIT: u64 = 2 * 1024 * 1024;

pub(crate) fn capture_discarded_response(response: ProxyResponse) {
    static SLOTS: LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
        LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(32)));
    let Ok(permit) = SLOTS.clone().try_acquire_owned() else {
        let context = Context::new("discarded_response", response.headers());
        record(context.event("discarded_body_capture_skipped", json!({"http_status":response.status().as_u16(),"reason":"capture concurrency limit","body":null})));
        return;
    };
    tokio::spawn(async move {
        let _permit = permit;
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            drain_error_response(response),
        )
        .await;
    });
}

async fn drain_error_response(response: ProxyResponse) {
    let mut stream = response.bytes_stream();
    let mut consumed = 0usize;
    while let Some(Ok(bytes)) = stream.next().await {
        consumed = consumed.saturating_add(bytes.len());
        if consumed >= BODY_LIMIT {
            break;
        }
    }
}

#[derive(Clone)]
pub(crate) struct Context(Value);

impl Context {
    pub(crate) fn new(target: &str, headers: &http::HeaderMap) -> Self {
        let target = url::Url::parse(target)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .map(|url| url.origin().ascii_serialization())
            .unwrap_or_else(|| redact(target, 256));
        let session = ["session-id", "x-session-id", "conversation_id"]
            .iter()
            .find_map(|name| headers.get(*name).and_then(|value| value.to_str().ok()))
            .and_then(|value| uuid::Uuid::parse_str(value).ok());
        Self(json!({"attempt_id":uuid::Uuid::new_v4(), "target":target, "session_id":session}))
    }

    fn event(&self, kind: &str, payload: Value) -> Value {
        json!({"timestamp":chrono::Utc::now().to_rfc3339(),"kind":kind,"context":self.0,"detail":payload,
            "notice":"Bounded, redacted diagnostic evidence; not a complete response. Review before sharing."})
    }

    pub(crate) fn failed(&self, error: &ProxyError) {
        if let ProxyError::UpstreamError { status, body } = error {
            record(self.event(
                "upstream_error_returned",
                json!({"http_status":status,"body_excerpt":body.as_deref().map(safe_excerpt)}),
            ));
            return;
        }
        record(self.event(
            "request_failure",
            json!({"response_body":null,"error":redact(&error.to_string(), 4000)}),
        ));
    }

    pub(crate) fn observe(self, response: ProxyResponse) -> ProxyResponse {
        let status = response.status();
        let headers = response.headers().clone();
        let observer = Observer::new(self, status.as_u16(), &headers);
        ProxyResponse::streamed(
            status,
            headers,
            observe_stream(response.bytes_stream(), observer),
        )
    }
}

fn record(mut record: Value) {
    if cfg!(test) {
        return;
    }
    static DROPPED: AtomicU64 = AtomicU64::new(0);
    static WRITER: LazyLock<Option<mpsc::SyncSender<Value>>> = LazyLock::new(|| {
        let (sender, receiver) = mpsc::sync_channel(128);
        std::thread::Builder::new()
            .name("proxy-error-journal".into())
            .spawn(move || {
                let root = crate::config::get_app_config_dir().join("logs");
                for record in receiver {
                    if let Err(error) = write_record(&root, &record, FILE_LIMIT) {
                        DROPPED.fetch_add(1, Ordering::Relaxed);
                        log::warn!("[ErrorJournal] diagnostic write failed: {error}");
                    }
                }
            })
            .ok()
            .map(|_| sender)
    });
    let lost = DROPPED.swap(0, Ordering::Relaxed);
    record["previous_records_lost"] = json!(lost);
    if WRITER
        .as_ref()
        .is_none_or(|sender| sender.try_send(record).is_err())
    {
        DROPPED.fetch_add(lost.saturating_add(1), Ordering::Relaxed);
    }
}

fn write_record(root: &Path, record: &Value, limit: u64) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let path = root.join("proxy-errors.jsonl");
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    if path.metadata().map(|meta| meta.len()).unwrap_or(0) + bytes.len() as u64 > limit {
        let oldest = root.join("proxy-errors.2.jsonl");
        if oldest.exists() {
            std::fs::remove_file(&oldest)?;
        }
        let previous = root.join("proxy-errors.1.jsonl");
        if previous.exists() {
            std::fs::rename(&previous, &oldest)?;
        }
        if path.exists() {
            std::fs::rename(&path, &previous)?;
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(&bytes)
}

fn selected_error(payload: &Value) -> Option<Value> {
    if matches!(
        payload.get("status").and_then(Value::as_str),
        Some("failed" | "cancelled")
    ) {
        let fields: serde_json::Map<String, Value> =
            ["id", "model", "status", "error", "incomplete_details"]
                .into_iter()
                .filter_map(|name| {
                    payload
                        .get(name)
                        .map(|value| (name.to_string(), value.clone()))
                })
                .collect();
        return Some(Value::Object(fields));
    }
    if payload
        .pointer("/promptFeedback/blockReason")
        .and_then(Value::as_str)
        .is_some_and(|reason| !reason.is_empty() && reason != "BLOCK_REASON_UNSPECIFIED")
    {
        return payload.get("promptFeedback").cloned();
    }
    if let Some(candidate) = payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|candidates| {
            candidates.iter().find(|candidate| {
                matches!(
                    candidate.get("finishReason").and_then(Value::as_str),
                    Some(
                        "SAFETY"
                            | "RECITATION"
                            | "BLOCKLIST"
                            | "PROHIBITED_CONTENT"
                            | "SPII"
                            | "MALFORMED_FUNCTION_CALL"
                            | "UNEXPECTED_TOOL_CALL"
                    )
                )
            })
        })
    {
        return Some(
            json!({"finishReason":candidate.get("finishReason"),"safetyRatings":candidate.get("safetyRatings")}),
        );
    }
    if let Some(error) = payload
        .pointer("/response/error")
        .or_else(|| payload.get("error"))
        .filter(|value| !value.is_null())
    {
        return Some(error.clone());
    }
    matches!(
        payload.get("type").and_then(Value::as_str),
        Some("error" | "response.error" | "response.failed")
    )
    .then(|| payload.clone())
}

fn safe_excerpt(body: &str) -> String {
    fn scrub(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    let key = key.to_ascii_lowercase().replace(['-', '_'], "");
                    if [
                        "token",
                        "password",
                        "secret",
                        "apikey",
                        "authorization",
                        "cookie",
                    ]
                    .iter()
                    .any(|name| key.contains(name))
                        || matches!(
                            key.as_str(),
                            "request"
                                | "headers"
                                | "messages"
                                | "prompt"
                                | "input"
                                | "output"
                                | "content"
                        )
                    {
                        *value = json!("[redacted]");
                    } else {
                        scrub(value);
                    }
                }
            }
            Value::Array(values) => values.iter_mut().for_each(scrub),
            _ => {}
        }
    }
    if let Ok(mut value) = serde_json::from_str::<Value>(body) {
        scrub(&mut value);
        redact(&value.to_string(), 8000)
    } else if body.trim_start().starts_with(['{', '[']) {
        "[malformed or truncated JSON omitted]".to_string()
    } else {
        redact(body, 8000)
    }
}

struct Observer {
    context: Context,
    status: u16,
    headers: Value,
    encoding: Option<String>,
    sse: bool,
    buffer: String,
    remainder: Vec<u8>,
    body: Vec<u8>,
    total: usize,
    terminal: bool,
    skipping: bool,
    finished: bool,
}

impl Observer {
    fn new(context: Context, status: u16, headers: &http::HeaderMap) -> Self {
        let safe_headers: serde_json::Map<String, Value> = [
            "content-type",
            "content-encoding",
            "x-request-id",
            "request-id",
            "cf-ray",
            "retry-after",
        ]
        .into_iter()
        .filter_map(|name| {
            headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(|value| (name.to_string(), json!(redact(value, 256))))
        })
        .collect();
        Self {
            context,
            status,
            headers: Value::Object(safe_headers),
            encoding: super::content_encoding::get_content_encoding(headers),
            sse: headers
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("text/event-stream")),
            buffer: String::new(),
            remainder: Vec::new(),
            body: Vec::new(),
            total: 0,
            terminal: false,
            skipping: false,
            finished: false,
        }
    }

    fn evidence(&self, kind: &str, detail: Value) -> Value {
        self.context.event(kind, json!({"http_status":self.status,"headers":self.headers,"bytes_seen":self.total,"evidence":detail}))
    }

    fn feed(&mut self, bytes: &[u8]) -> Vec<Value> {
        self.total = self.total.saturating_add(bytes.len());
        let mut events = Vec::new();
        if !self.sse || self.encoding.is_some() || !(200..300).contains(&self.status) {
            let remaining = BODY_LIMIT.saturating_sub(self.body.len());
            self.body
                .extend_from_slice(&bytes[..remaining.min(bytes.len())]);
            return events;
        }
        for chunk in bytes.chunks(8192) {
            super::sse::append_utf8_safe(&mut self.buffer, &mut self.remainder, chunk);
            while let Some(block) = super::sse::take_sse_block(&mut self.buffer) {
                if self.skipping {
                    self.skipping = false;
                    continue;
                }
                let data = block
                    .lines()
                    .filter_map(|line| super::sse::strip_sse_field(line, "data"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let event = block
                    .lines()
                    .find_map(|line| super::sse::strip_sse_field(line, "event"))
                    .unwrap_or("");
                if data.trim() == "[DONE]"
                    || matches!(
                        event,
                        "message_stop"
                            | "response.completed"
                            | "response.done"
                            | "response.incomplete"
                    )
                {
                    self.terminal = true;
                }
                if data.is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(&data) {
                    Ok(payload) => {
                        if matches!(
                            payload.get("type").and_then(Value::as_str),
                            Some(
                                "message_stop"
                                    | "response.completed"
                                    | "response.done"
                                    | "response.incomplete"
                            )
                        ) || payload
                            .pointer("/choices/0/finish_reason")
                            .is_some_and(|value| !value.is_null())
                            || payload.pointer("/candidates/0/finishReason").is_some()
                        {
                            self.terminal = true;
                        }
                        if let Some(error) = selected_error(&payload).or_else(|| {
                            matches!(event, "error" | "response.error" | "response.failed")
                                .then(|| payload.clone())
                        }) {
                            self.terminal = true;
                            events.push(self.evidence("upstream_sse_error", json!({"event":redact(event,128),"error":safe_excerpt(&error.to_string())})));
                        }
                    }
                    Err(_) if data.trim() != "[DONE]" => events.push(
                        self.evidence("sse_parse_error", json!({"excerpt":safe_excerpt(&data)})),
                    ),
                    _ => {}
                }
            }
            if self.buffer.len() > BODY_LIMIT {
                self.buffer.clear();
                self.remainder.clear();
                self.skipping = true;
                events.push(self.evidence(
                    "sse_inspection_limit",
                    json!({"body":null,"limit":BODY_LIMIT}),
                ));
            }
        }
        events
    }

    fn finish(&mut self, transport_error: Option<&str>) -> Vec<Value> {
        self.finished = true;
        let mut events = Vec::new();
        if let Some(error) = transport_error {
            events.push(self.evidence(
                "stream_transport_error",
                json!({"error":redact(error,4000)}),
            ));
        }
        if self.sse && (200..300).contains(&self.status) && self.encoding.is_none() {
            if !self.terminal && transport_error.is_none() {
                events.push(self.evidence(
                    "sse_eof_without_terminal",
                    json!({"pending_bytes":self.buffer.len(),"body":null}),
                ));
            }
            return events;
        }
        let decoded = match self.encoding.as_deref() {
            Some(encoding) => super::content_encoding::decompress_body_with_limit(
                encoding, &self.body, BODY_LIMIT,
            )
            .ok()
            .flatten(),
            None => Some(self.body.clone()),
        };
        let text = decoded
            .as_deref()
            .and_then(|body| std::str::from_utf8(body).ok());
        let content_type = self
            .headers
            .get("content-type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let unexpected_body = (200..300).contains(&self.status)
            && self.total == self.body.len()
            && (content_type.contains("text/html")
                || (content_type.contains("application/json") || content_type.contains("+json"))
                    && text.is_some_and(|text| serde_json::from_str::<Value>(text).is_err()));
        if self.sse && (200..300).contains(&self.status) && self.encoding.is_some() {
            if let Some(decoded) = decoded.as_deref() {
                let mut headers = http::HeaderMap::new();
                headers.insert(
                    "content-type",
                    http::HeaderValue::from_static("text/event-stream"),
                );
                let mut inspection = Self::new(self.context.clone(), self.status, &headers);
                inspection.headers = self.headers.clone();
                events.extend(inspection.feed(decoded));
                events.extend(inspection.finish(transport_error));
            } else {
                events.push(self.evidence(
                    "encoded_sse_inspection_unavailable",
                    json!({"body":null,"captured_bytes":self.body.len(),"limit":BODY_LIMIT}),
                ));
            }
            return events;
        }
        if unexpected_body
            || !(200..300).contains(&self.status)
            || text
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .and_then(|payload| selected_error(&payload))
                .is_some()
        {
            use sha2::{Digest, Sha256};
            events.push(self.evidence(if unexpected_body { "unexpected_response_body" } else { "upstream_http_error" }, json!({"body_excerpt":text.map(safe_excerpt),"captured_prefix_sha256":format!("{:x}",Sha256::digest(&self.body)),"captured_bytes":self.body.len(),"truncated":self.total>self.body.len(),"complete":transport_error.is_none(),"decode_available":text.is_some()})));
        } else if self.sse && self.encoding.is_some() && transport_error.is_some() {
            events.push(self.evidence("encoded_sse_uninspected", json!({"body":null})));
        }
        events
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        if !self.finished && !self.terminal {
            if !(200..300).contains(&self.status) && !self.body.is_empty() {
                for event in self.finish(Some("consumer stopped before full response")) {
                    if event["kind"] != "stream_transport_error" {
                        record(event);
                    }
                }
            }
            record(self.evidence("response_consumption_stopped", json!({"body":null,"meaning":"Consumer stopped reading; not proof of an upstream failure."})));
        }
    }
}

fn observe_stream<S>(
    stream: S,
    mut observer: Observer,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    async_stream::stream! {
        futures::pin_mut!(stream);
        while let Some(item) = stream.next().await {
            match &item {
                Ok(bytes) => for event in observer.feed(bytes) { record(event); },
                Err(error) => for event in observer.finish(Some(&super::error::error_chain_message(error))) { record(event); },
            }
            yield item;
        }
        if !observer.finished {
            for event in observer.finish(None) { record(event); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn journal_drains_discarded_error_bodies_with_a_byte_limit() {
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let stream_count = count.clone();
        let stream = futures::stream::repeat_with(move || {
            stream_count.fetch_add(1, Ordering::Relaxed);
            Ok(Bytes::from(vec![b'x'; BODY_LIMIT]))
        });
        let response = ProxyResponse::streamed(
            http::StatusCode::BAD_GATEWAY,
            http::HeaderMap::new(),
            stream,
        );
        drain_error_response(response).await;
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn journal_discarded_body_timeout_does_not_block_caller() {
        let response = ProxyResponse::streamed(
            http::StatusCode::BAD_GATEWAY,
            http::HeaderMap::new(),
            futures::stream::pending::<Result<Bytes, std::io::Error>>(),
        );
        let before = tokio::time::Instant::now();
        capture_discarded_response(response);
        assert_eq!(tokio::time::Instant::now(), before);
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        let response = ProxyResponse::streamed(
            http::StatusCode::BAD_GATEWAY,
            http::HeaderMap::new(),
            futures::stream::pending::<Result<Bytes, std::io::Error>>(),
        );
        assert!(tokio::time::timeout(
            std::time::Duration::from_secs(1),
            drain_error_response(response)
        )
        .await
        .is_err());
    }

    #[test]
    fn journal_keeps_safe_upstream_origin() {
        let context = Context::new(
            "https://username:password@api.example.com/path?token=secret",
            &http::HeaderMap::new(),
        );
        assert_eq!(context.0["target"], "https://api.example.com");
    }

    #[test]
    fn journal_detects_status_only_and_gemini_rejections() {
        for payload in [
            json!({"id":"resp-x","status":"failed","error":null}),
            json!({"status":"cancelled"}),
            json!({"promptFeedback":{"blockReason":"SAFETY"}}),
            json!({"candidates":[{"finishReason":"SAFETY","content":{"parts":[{"text":"private"}]}}]}),
        ] {
            assert!(selected_error(&payload).is_some(), "{payload}");
        }
        assert!(selected_error(&json!({"candidates":[{"finishReason":"STOP"}]})).is_none());
    }

    #[test]
    fn journal_inspects_bounded_gzip_errors_and_unexpected_html() {
        let mut encoded = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoded
            .write_all(b"data: {\"error\":{\"message\":\"real compressed cause\"}}\n\n")
            .unwrap();
        let mut observer = observer(200, "text/event-stream");
        observer.encoding = Some("gzip".into());
        observer.feed(&encoded.finish().unwrap());
        let events = observer.finish(None);
        assert_eq!(events.len(), 1);
        assert!(events[0].to_string().contains("real compressed cause"));
        let mut html = self::observer(200, "text/html");
        html.feed(b"<html>reverse proxy unavailable</html>");
        assert_eq!(html.finish(None)[0]["kind"], "unexpected_response_body");
    }

    fn observer(status: u16, content_type: &str) -> Observer {
        let mut headers = http::HeaderMap::new();
        headers.insert("content-type", content_type.parse().unwrap());
        Observer::new(Context::new("test", &headers), status, &headers)
    }

    #[test]
    fn journal_records_http_statuses_and_real_error_bodies() {
        for status in [400, 401, 403, 429, 500, 502, 503] {
            let mut observer = observer(status, "application/json");
            observer.feed(br#"{"error":{"message":"real upstream cause","code":"overloaded"}}"#);
            let events = observer.finish(None);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["detail"]["http_status"], status);
            assert!(events[0].to_string().contains("real upstream cause"));
        }
        let mut observer = observer(502, "text/html");
        observer.feed(b"<html>Bad gateway. Bearer supersecret</html>");
        let events = observer.finish(None);
        assert!(events[0].to_string().contains("Bad gateway"));
        assert!(!events[0].to_string().contains("supersecret"));
    }

    #[test]
    fn journal_collects_multiline_sse_in_arbitrary_chunks() {
        let body = b"event: error\r\ndata: {\"type\":\"error\",\r\ndata: \"error\":{\"message\":\"overloaded\"}}\r\n\r\n";
        let mut observer = observer(200, "text/event-stream");
        let mut records = Vec::new();
        for chunk in body.chunks(3) {
            records.extend(observer.feed(chunk));
        }
        records.extend(observer.finish(None));
        assert_eq!(records.len(), 1);
        assert!(records[0].to_string().contains("overloaded"));
    }

    #[test]
    fn journal_success_is_not_persisted_and_eof_is_explicit() {
        let mut successful = observer(200, "text/event-stream");
        assert!(successful.feed(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"private\"}\n\ndata: {\"type\":\"response.completed\"}\n\n").is_empty());
        assert!(successful.finish(None).is_empty());
        let mut interrupted = observer(200, "text/event-stream");
        interrupted.feed(b"data: {\"delta\":\"private\"}\n\n");
        let records = interrupted.finish(None);
        assert_eq!(records[0]["kind"], "sse_eof_without_terminal");
        assert!(!records[0].to_string().contains("private"));
    }

    #[tokio::test]
    async fn journal_observation_preserves_bytes_and_transport_errors() {
        let source = futures::stream::iter(vec![
            Ok(Bytes::from_static(b"data: [DONE]\n\n")),
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "original timeout",
            )),
        ]);
        let output = observe_stream(source, observer(200, "text/event-stream"))
            .collect::<Vec<_>>()
            .await;
        assert_eq!(
            output[0].as_ref().unwrap(),
            &Bytes::from_static(b"data: [DONE]\n\n")
        );
        assert_eq!(
            output[1].as_ref().unwrap_err().kind(),
            std::io::ErrorKind::TimedOut
        );
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn journal_caps_memory_and_rotates_only_owned_files() {
        let mut observer = observer(500, "text/plain");
        observer.feed(&vec![b'x'; BODY_LIMIT * 2]);
        assert_eq!(observer.body.len(), BODY_LIMIT);
        assert_eq!(
            observer.finish(None)[0]["detail"]["evidence"]["truncated"],
            true
        );
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("unrelated.log"), "keep").unwrap();
        for index in 0..10 {
            write_record(root.path(), &json!({"index":index,"message":"test"}), 50).unwrap();
        }
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 4);
        assert_eq!(
            std::fs::read_to_string(root.path().join("unrelated.log")).unwrap(),
            "keep"
        );
        for name in [
            "proxy-errors.jsonl",
            "proxy-errors.1.jsonl",
            "proxy-errors.2.jsonl",
        ] {
            for line in std::fs::read_to_string(root.path().join(name))
                .unwrap()
                .lines()
            {
                assert!(serde_json::from_str::<Value>(line).is_ok());
            }
        }
    }

    #[test]
    fn journal_detects_provider_errors_without_recording_success_text() {
        for payload in [
            json!({"error":{"message":"overloaded","type":"overloaded_error"}}),
            json!({"type":"response.failed","response":{"error":{"code":"invalid_request","message":"bad input"}}}),
            json!({"type":"error","message":"bad gateway","code":502}),
        ] {
            assert!(selected_error(&payload).is_some(), "{payload}");
        }
        assert!(selected_error(
            &json!({"type":"response.output_text.delta","delta":"private answer"})
        )
        .is_none());
        assert!(selected_error(&json!({"error":null,"choices":[]})).is_none());
    }

    #[test]
    fn journal_preserves_error_reason_but_removes_credentials_and_request_echoes() {
        let body = json!({"error":{"message":"capacity exhausted","password":"supersecret"},"request":{"messages":[{"content":"private prompt"}]},"access_token":"mytoken","headers":{"Cookie":"session=abc; auth=def"}}).to_string();
        let safe = safe_excerpt(&body);
        assert!(safe.contains("capacity exhausted"));
        for secret in [
            "supersecret",
            "private prompt",
            "mytoken",
            "session=abc",
            "auth=def",
        ] {
            assert!(!safe.contains(secret), "leaked {secret}: {safe}");
        }
        assert!(safe_excerpt(&"x".repeat(100000)).len() < 20000);
    }
}
