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
        let mut captured = Vec::new();
        let mut overflowed = false;
        while let Some(item) = stream.next().await {
            if let Ok(bytes) = &item {
                if captured.len().saturating_add(bytes.len()) <= MAX_OBSERVED_BYTES {
                    captured.extend_from_slice(bytes);
                } else {
                    overflowed = true;
                    captured.clear();
                }
            }
            yield item;
        }
        if !overflowed {
            if let Ok(body) = std::str::from_utf8(&captured) {
                on_shape(capture_chat_sse_shape(body));
            }
        }
    }
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
    use super::{capture_chat_json_shape, capture_chat_sse_shape};
    use crate::protocol_compatibility::{ReasoningSemantic, ReasoningSource};
    use serde_json::json;

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
}
