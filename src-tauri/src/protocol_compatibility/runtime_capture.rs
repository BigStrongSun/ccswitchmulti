use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;

use super::{
    classify_reasoning_shape, redact_json_probe_response, redact_sse_probe_response,
    runtime_observer::ObservedResponseShape, TransportKind,
};

pub(crate) fn capture_chat_json_shape(body: &Value) -> ObservedResponseShape {
    let redacted = redact_json_probe_response(200, body);
    ObservedResponseShape {
        transport: TransportKind::OpenAiChat,
        reasoning_shape: classify_reasoning_shape(std::slice::from_ref(body)),
        tool_call_observed: chat_payload_has_tool_call(body),
        field_paths: redacted.paths,
        event_types: Vec::new(),
    }
}

pub(crate) fn capture_chat_sse_shape(body: &str) -> ObservedResponseShape {
    let redacted = redact_sse_probe_response(200, body);
    let payloads = parse_sse_json_payloads(body);
    ObservedResponseShape {
        transport: TransportKind::OpenAiChat,
        reasoning_shape: classify_reasoning_shape(&payloads),
        tool_call_observed: payloads.iter().any(chat_payload_has_tool_call),
        field_paths: redacted.paths,
        event_types: redacted.event_types,
    }
}

pub(crate) fn capture_chat_sse_stream<E, F>(
    stream: impl futures::Stream<Item = Result<Bytes, E>> + Send + 'static,
    on_shape: F,
) -> impl futures::Stream<Item = Result<Bytes, E>> + Send
where
    E: Send + 'static,
    F: FnOnce(ObservedResponseShape) + Send + 'static,
{
    const MAX_OBSERVED_BYTES: usize = 1024 * 1024;
    async_stream::stream! {
        futures::pin_mut!(stream);
        let mut scanner = SseDoneScanner::new(MAX_OBSERVED_BYTES);
        let mut on_shape = Some(on_shape);
        while let Some(item) = stream.next().await {
            if let Ok(bytes) = &item {
                if !scanner.done && !scanner.overflowed {
                    scanner.push(bytes);
                    if scanner.done {
                        if let Some(callback) = on_shape.take() {
                            if let Ok(body) = std::str::from_utf8(&scanner.captured) {
                                callback(capture_chat_sse_shape(body));
                            }
                        }
                    }
                }
            }
            yield item;
        }

        if !scanner.overflowed {
            if let Some(callback) = on_shape.take() {
                if let Ok(body) = std::str::from_utf8(&scanner.captured) {
                    callback(capture_chat_sse_shape(body));
                }
            }
        }
    }
}

struct SseDoneScanner {
    captured: Vec<u8>,
    frame_start: usize,
    line_start: usize,
    max_bytes: usize,
    done: bool,
    overflowed: bool,
}

impl SseDoneScanner {
    fn new(max_bytes: usize) -> Self {
        Self {
            captured: Vec::new(),
            frame_start: 0,
            line_start: 0,
            max_bytes,
            done: false,
            overflowed: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        for byte in chunk {
            if self.captured.len() == self.max_bytes {
                self.captured.clear();
                self.overflowed = true;
                return;
            }
            self.captured.push(*byte);
            if *byte != b'\n' {
                continue;
            }

            let mut line_end = self.captured.len() - 1;
            if line_end > self.line_start && self.captured[line_end - 1] == b'\r' {
                line_end -= 1;
            }
            if line_end == self.line_start {
                let frame = &self.captured[self.frame_start..self.line_start];
                if sse_frame_is_done(frame) {
                    self.done = true;
                    return;
                }
                self.frame_start = self.captured.len();
            }
            self.line_start = self.captured.len();
        }
    }
}

fn sse_frame_is_done(frame: &[u8]) -> bool {
    let Ok(frame) = std::str::from_utf8(frame) else {
        return false;
    };
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    data.trim() == "[DONE]"
}

fn parse_sse_json_payloads(body: &str) -> Vec<Value> {
    let normalized = body.replace("\r\n", "\n");
    normalized
        .split("\n\n")
        .filter_map(|frame| {
            let data = frame
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() || data == "[DONE]" {
                None
            } else {
                serde_json::from_str(&data).ok()
            }
        })
        .collect()
}

fn chat_payload_has_tool_call(payload: &Value) -> bool {
    payload
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                choice
                    .get("delta")
                    .or_else(|| choice.get("message"))
                    .and_then(|part| part.get("tool_calls"))
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty())
            })
        })
}

#[cfg(test)]
mod tests {
    use super::{capture_chat_json_shape, capture_chat_sse_shape, capture_chat_sse_stream};
    use crate::protocol_compatibility::{ReasoningSemantic, ReasoningSource};
    use bytes::Bytes;
    use futures::{stream, StreamExt};
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    fn runtime_capture_retains_structure_without_response_text() {
        let json_shape = capture_chat_json_shape(&json!({
            "choices": [{
                "message": {
                    "reasoning_content": "private reasoning sentinel",
                    "content": "private answer sentinel"
                }
            }]
        }));
        let json = serde_json::to_string(&json_shape).expect("serialize shape");
        assert!(!json.contains("private reasoning sentinel"));
        assert!(!json.contains("private answer sentinel"));

        let sse_shape = capture_chat_sse_shape(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private stream sentinel\"}}]}\n\ndata: [DONE]\n\n",
        );
        let json = serde_json::to_string(&sse_shape).expect("serialize stream shape");
        assert!(!json.contains("private stream sentinel"));
    }

    #[test]
    fn runtime_capture_parses_crlf_sse_frames() {
        let shape = capture_chat_sse_shape(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private reasoning\"}}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"call_1\"}]}}]}\r\n\r\ndata: [DONE]\r\n\r\n",
        );

        assert_eq!(shape.reasoning_shape.semantic, ReasoningSemantic::Readable);
        assert_eq!(
            shape.reasoning_shape.source,
            ReasoningSource::ReasoningContent
        );
        assert!(shape.tool_call_observed);
    }

    #[tokio::test]
    async fn runtime_capture_observes_done_before_pending_upstream() {
        let (sender, mut receiver) = tokio::sync::oneshot::channel();
        let upstream = stream::once(async {
            Ok::<_, std::io::Error>(Bytes::from_static(b"data: [DONE]\r\n\r\n"))
        })
        .chain(stream::pending());
        let mut wrapped = Box::pin(capture_chat_sse_stream(upstream, move |shape| {
            let _ = sender.send(shape);
        }));

        let item = tokio::time::timeout(std::time::Duration::from_secs(1), wrapped.next())
            .await
            .expect("wrapper should yield DONE without waiting for EOF")
            .expect("upstream chunk");
        assert!(item.is_ok());
        assert_eq!(
            receiver
                .try_recv()
                .expect("shape observed before yielding chunk")
                .reasoning_shape
                .source,
            ReasoningSource::None
        );
    }

    #[tokio::test]
    async fn runtime_capture_drop_before_done_does_not_observe_partial_capture() {
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = Arc::clone(&calls);
        let upstream = stream::once(async {
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"partial\"}}]}\n\n",
            ))
        })
        .chain(stream::pending());
        let mut wrapped = Box::pin(capture_chat_sse_stream(upstream, move |_| {
            callback_calls.fetch_add(1, Ordering::SeqCst);
        }));

        assert!(wrapped.next().await.unwrap().is_ok());
        drop(wrapped);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn runtime_capture_detects_done_split_across_chunks() {
        let chunks = vec![
            Ok::<_, std::io::Error>(Bytes::from_static(b"data: [DO")),
            Ok(Bytes::from_static(b"NE]\r\n\r\n")),
        ];
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let output = capture_chat_sse_stream(stream::iter(chunks), move |shape| {
            let _ = sender.send(shape);
        })
        .collect::<Vec<_>>()
        .await;
        let observed = receiver.await.expect("DONE frame should invoke callback");
        assert_eq!(observed.reasoning_shape.source, ReasoningSource::None);
        assert_eq!(
            output
                .into_iter()
                .map(|item| item.unwrap())
                .collect::<Vec<_>>(),
            [
                Bytes::from_static(b"data: [DO"),
                Bytes::from_static(b"NE]\r\n\r\n")
            ]
        );
    }

    #[tokio::test]
    async fn runtime_capture_ignores_same_chunk_reasoning_after_done() {
        let input = Bytes::from_static(
            b"data: [DONE]\n\ndata: {\"choices\":[{\"delta\":{\"reasoning\":\"late\"}}]}\n\n",
        );
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let output = capture_chat_sse_stream(
            stream::iter(vec![Ok::<_, std::io::Error>(input.clone())]),
            move |shape| {
                let _ = sender.send(shape);
            },
        )
        .collect::<Vec<_>>()
        .await;
        let observed = receiver.await.expect("DONE should invoke callback");
        assert_eq!(observed.reasoning_shape.source, ReasoningSource::None);
        assert_eq!(output.into_iter().next().unwrap().unwrap(), input);
    }

    #[tokio::test]
    async fn runtime_capture_calls_eof_observer_once_and_preserves_bytes() {
        let chunks = vec![
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"visible\"}}]}\n\n",
            )),
            Ok(Bytes::from_static(b"data: {\"choices\":[]}\n\n")),
        ];
        let expected = vec![
            Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"visible\"}}]}\n\n",
            ),
            Bytes::from_static(b"data: {\"choices\":[]}\n\n"),
        ];
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = Arc::clone(&calls);
        let output = capture_chat_sse_stream(stream::iter(chunks), move |_| {
            callback_calls.fetch_add(1, Ordering::SeqCst);
        })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(Result::unwrap)
        .collect::<Vec<_>>();

        assert_eq!(output, expected);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn runtime_capture_overflow_fails_closed_without_changing_passthrough() {
        let input = Bytes::from(vec![b'x'; 1024 * 1024 + 1]);
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = Arc::clone(&calls);
        let output = capture_chat_sse_stream(
            stream::iter(vec![Ok::<_, std::io::Error>(input.clone())]),
            move |_| {
                callback_calls.fetch_add(1, Ordering::SeqCst);
            },
        )
        .collect::<Vec<_>>()
        .await;

        assert_eq!(output.len(), 1);
        assert_eq!(output.into_iter().next().unwrap().unwrap(), input);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
