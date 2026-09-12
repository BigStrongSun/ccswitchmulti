//! Client-facing reasoning shape adaptation for Codex Responses.
//!
//! Third-party providers may expose readable reasoning as `reasoning_text`, while
//! current Codex Desktop renders the summary-backed `reasoning.summary` shape.
//! This module contains the narrow, request-scoped adapter; it must never be
//! applied to CLI/TUI or external OpenAI API callers.

use bytes::Bytes;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashSet;

use crate::proxy::sse::{append_utf8_safe, take_sse_block};

/// Convert a completed Responses value for the Codex Desktop presentation shape.
///
/// The function is intentionally a no-op unless the caller has already classified
/// the request as trusted Codex Desktop. It only converts reasoning items whose
/// visible text is present in `content`; native summary items remain unchanged.
pub(crate) fn map_completed_response_for_desktop(response: &mut Value) {
    let Some(output) = response.get_mut("output").and_then(Value::as_array_mut) else {
        return;
    };
    for item in output {
        map_reasoning_item_for_desktop(item);
    }
}

fn map_reasoning_item_for_desktop(item: &mut Value) -> bool {
    if item.get("type").and_then(Value::as_str) != Some("reasoning") {
        return false;
    }
    let Some(content) = item.get("content").and_then(Value::as_array) else {
        return false;
    };
    let has_summary_text = item
        .get("summary")
        .and_then(Value::as_array)
        .is_some_and(|summary| {
            summary.iter().any(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.is_empty())
            })
        });
    if has_summary_text {
        return false;
    }
    let text = content
        .iter()
        .filter_map(|part| {
            let kind = part.get("type").and_then(Value::as_str);
            matches!(
                kind,
                Some("reasoning_text" | "reasoning_details" | "reasoning" | "text")
            )
            .then(|| part.get("text").and_then(Value::as_str))
            .flatten()
        })
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() {
        return false;
    }
    let Some(object) = item.as_object_mut() else {
        return false;
    };
    object.insert(
        "summary".to_string(),
        json!([{ "type": "summary_text", "text": text }]),
    );
    object.remove("content");
    true
}

/// Rewrite one Responses SSE block. The returned bytes may contain the original
/// event plus the summary-part lifecycle events required by Desktop.
pub(crate) fn map_sse_block_for_desktop(
    block: &str,
    open_reasoning: &mut HashSet<String>,
) -> Bytes {
    let mut event_name = None;
    let mut data_parts = Vec::new();
    for line in block.lines() {
        if let Some(value) = line.strip_prefix("event: ") {
            event_name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data: ") {
            data_parts.push(value);
        }
    }
    let Some(data) = serde_json::from_str::<Value>(&data_parts.join("\n")).ok() else {
        return Bytes::from(format!("{block}\n\n"));
    };
    let event_name =
        event_name.or_else(|| data.get("type").and_then(Value::as_str).map(str::to_string));
    let Some(event_name) = event_name else {
        return Bytes::from(format!("{block}\n\n"));
    };
    let mut mapped = data;
    let item_id = mapped
        .get("item_id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            mapped
                .pointer("/item/id")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let mut out = String::new();
    match event_name.as_str() {
        "response.output_item.added" => {
            let is_reasoning = mapped.pointer("/item/type").and_then(Value::as_str)
                == Some("reasoning")
                && mapped.pointer("/item/content").is_some();
            if is_reasoning {
                if let Some(item) = mapped.get_mut("item") {
                    map_reasoning_item_for_desktop(item);
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        open_reasoning.insert(id.to_string());
                    }
                }
                out.push_str(&serialize_event(&event_name, &mapped));
                if let (Some(id), Some(index)) = (
                    item_id.as_deref(),
                    mapped.get("output_index").and_then(Value::as_u64),
                ) {
                    out.push_str(&serialize_event(
                        "response.reasoning_summary_part.added",
                        &json!({
                            "type":"response.reasoning_summary_part.added",
                            "item_id":id,
                            "output_index":index,
                            "summary_index":0,
                            "part":{"type":"summary_text","text":""}
                        }),
                    ));
                }
                return Bytes::from(out);
            }
        }
        "response.reasoning_text.delta" => {
            let mut prefix = String::new();
            if let (Some(id), Some(index)) = (
                item_id.as_deref(),
                mapped.get("output_index").and_then(Value::as_u64),
            ) {
                if open_reasoning.insert(id.to_string()) {
                    prefix.push_str(&serialize_event(
                        "response.output_item.added",
                        &json!({
                            "type":"response.output_item.added",
                            "output_index":index,
                            "item":{
                                "id":id,
                                "type":"reasoning",
                                "status":"in_progress",
                                "summary":[]
                            }
                        }),
                    ));
                    prefix.push_str(&serialize_event(
                        "response.reasoning_summary_part.added",
                        &json!({
                            "type":"response.reasoning_summary_part.added",
                            "item_id":id,
                            "output_index":index,
                            "summary_index":0,
                            "part":{"type":"summary_text","text":""}
                        }),
                    ));
                }
            }
            mapped["type"] = json!("response.reasoning_summary_text.delta");
            mapped["summary_index"] = json!(0);
            if let Some(object) = mapped.as_object_mut() {
                object.remove("content_index");
            }
            prefix.push_str(&serialize_event(
                "response.reasoning_summary_text.delta",
                &mapped,
            ));
            return Bytes::from(prefix);
        }
        "response.reasoning_text.done" => {
            let mut prefix = String::new();
            if let (Some(id), Some(index)) = (
                item_id.as_deref(),
                mapped.get("output_index").and_then(Value::as_u64),
            ) {
                if open_reasoning.insert(id.to_string()) {
                    prefix.push_str(&serialize_event(
                        "response.output_item.added",
                        &json!({
                            "type":"response.output_item.added",
                            "output_index":index,
                            "item":{
                                "id":id,
                                "type":"reasoning",
                                "status":"in_progress",
                                "summary":[]
                            }
                        }),
                    ));
                    prefix.push_str(&serialize_event(
                        "response.reasoning_summary_part.added",
                        &json!({
                            "type":"response.reasoning_summary_part.added",
                            "item_id":id,
                            "output_index":index,
                            "summary_index":0,
                            "part":{"type":"summary_text","text":""}
                        }),
                    ));
                }
            }
            mapped["type"] = json!("response.reasoning_summary_text.done");
            mapped["summary_index"] = json!(0);
            if let Some(object) = mapped.as_object_mut() {
                object.remove("content_index");
            }
            out.push_str(&prefix);
            out.push_str(&serialize_event(
                "response.reasoning_summary_text.done",
                &mapped,
            ));
            if let (Some(id), Some(index)) = (
                item_id.as_deref(),
                mapped.get("output_index").and_then(Value::as_u64),
            ) {
                out.push_str(&serialize_event(
                    "response.reasoning_summary_part.done",
                    &json!({
                        "type":"response.reasoning_summary_part.done",
                        "item_id":id,
                        "output_index":index,
                        "summary_index":0,
                        "part":{"type":"summary_text","text":mapped.get("text").cloned().unwrap_or_else(||json!(""))}
                    }),
                ));
            }
            if let Some(id) = item_id {
                open_reasoning.remove(&id);
            }
            return Bytes::from(out);
        }
        "response.output_item.done" => {
            let is_reasoning = mapped.pointer("/item/type").and_then(Value::as_str)
                == Some("reasoning")
                && mapped.pointer("/item/content").is_some();
            if is_reasoning {
                if let Some(item) = mapped.get_mut("item") {
                    map_reasoning_item_for_desktop(item);
                }
            }
        }
        "response.completed" | "response.incomplete" => {
            if let Some(response) = mapped.get_mut("response") {
                map_completed_response_for_desktop(response);
            }
        }
        _ => {}
    }
    serialize_event(&event_name, &mapped).into()
}

fn serialize_event(event: &str, data: &Value) -> String {
    format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(data).unwrap_or_default()
    )
}

/// Wrap a native Responses SSE stream with the Desktop-only reasoning adapter.
pub(crate) fn create_desktop_reasoning_mapping_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> impl Stream<Item = Result<Bytes, E>> + Send
where
    E: Send + 'static,
{
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder = Vec::new();
        let mut open_reasoning = HashSet::new();
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        yield Ok(map_sse_block_for_desktop(&block, &mut open_reasoning));
                    }
                }
                Err(error) => yield Err(error),
            }
        }
        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        if !buffer.trim().is_empty() {
            yield Ok(map_sse_block_for_desktop(&buffer, &mut open_reasoning));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{map_completed_response_for_desktop, map_sse_block_for_desktop};
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn raw_completed_reasoning_becomes_summary_and_drops_content() {
        let mut response = json!({"output":[{"id":"rs_1","type":"reasoning","summary":[],"content":[{"type":"reasoning_text","text":"Inspect route."}]}]});
        map_completed_response_for_desktop(&mut response);
        assert_eq!(response["output"][0]["summary"][0]["type"], "summary_text");
        assert_eq!(
            response["output"][0]["summary"][0]["text"],
            "Inspect route."
        );
        assert!(response["output"][0].get("content").is_none());
    }

    #[test]
    fn detected_readable_reasoning_variants_become_summary() {
        let mut response = json!({"output":[{"id":"rs_2","type":"reasoning","summary":[],"content":[{"type":"reasoning_details","text":"Inspect"},{"type":"reasoning","text":" route."}]}]});
        map_completed_response_for_desktop(&mut response);
        assert_eq!(
            response["output"][0]["summary"][0]["text"],
            "Inspect route."
        );
        assert!(response["output"][0].get("content").is_none());
    }

    #[test]
    fn raw_sse_delta_is_renamed_to_summary_delta() {
        let mut open = HashSet::new();
        let bytes = map_sse_block_for_desktop(
            "event: response.reasoning_text.delta\ndata: {\"type\":\"response.reasoning_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Inspect\"}",
            &mut open,
        );
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("response.reasoning_summary_text.delta"));
        assert!(!text.contains("content_index"));
        assert!(text.contains("summary_index"));
    }

    #[test]
    fn non_reasoning_sse_is_unchanged() {
        let mut open = HashSet::new();
        let bytes = map_sse_block_for_desktop(
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"answer\"}",
            &mut open,
        );
        assert_eq!(String::from_utf8(bytes.to_vec()).unwrap(), "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"answer\"}\n\n");
    }

    #[test]
    fn completed_sse_payload_maps_raw_output_items() {
        let mut open = HashSet::new();
        let bytes = map_sse_block_for_desktop(
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"reasoning\",\"content\":[{\"type\":\"reasoning_text\",\"text\":\"done\"}]}]}}",
            &mut open,
        );
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("summary_text"));
        assert!(!text.contains("reasoning_text"));
    }

    #[test]
    fn raw_delta_without_item_added_gets_complete_summary_lifecycle() {
        let mut open = HashSet::new();
        let bytes = map_sse_block_for_desktop(
            "event: response.reasoning_text.delta\ndata: {\"type\":\"response.reasoning_text.delta\",\"item_id\":\"rs_early\",\"output_index\":0,\"content_index\":0,\"delta\":\"Inspect\"}",
            &mut open,
        );
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("response.output_item.added"));
        assert!(text.contains("response.reasoning_summary_part.added"));
        assert!(text.contains("response.reasoning_summary_text.delta"));
    }
}
