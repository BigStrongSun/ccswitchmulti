use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{capture::CapturedProbeExchange, ReasoningSemantic, ReasoningSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreToolVisibleContent {
    Absent,
    Present,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedReasoningShape {
    pub semantic: ReasoningSemantic,
    pub source: ReasoningSource,
    pub pre_tool_visible_content: PreToolVisibleContent,
}

pub fn classify_captured_reasoning_shape(
    exchange: &CapturedProbeExchange,
) -> ClassifiedReasoningShape {
    let payloads = exchange
        .payloads()
        .iter()
        .map(|payload| payload.value.clone())
        .collect::<Vec<_>>();
    let mut shape = classify_reasoning_shape(&payloads);

    let has_native_reasoning_text = exchange.payloads().iter().any(|payload| {
        payload.event_type.as_deref() == Some("response.reasoning_text.delta")
            && nonempty(payload.value.get("delta"))
    });
    let has_native_summary = exchange.payloads().iter().any(|payload| {
        payload
            .event_type
            .as_deref()
            .is_some_and(|event| event.starts_with("response.reasoning_summary"))
            && (nonempty(payload.value.get("delta"))
                || nonempty(payload.value.get("text"))
                || nonempty(payload.value.get("part")))
    });

    if shape.semantic == ReasoningSemantic::None && has_native_summary {
        shape.semantic = ReasoningSemantic::Summary;
        shape.source = ReasoningSource::NativeResponses;
    } else if shape.semantic == ReasoningSemantic::None && has_native_reasoning_text {
        shape.semantic = ReasoningSemantic::Readable;
        shape.source = ReasoningSource::NativeResponses;
    }
    shape
}

pub fn classify_reasoning_shape(payloads: &[Value]) -> ClassifiedReasoningShape {
    let mut evidence = Evidence::default();
    let mut tool_seen = false;
    let mut ordinary_content = String::new();

    for payload in payloads {
        inspect_chat_choices(
            payload,
            &mut evidence,
            &mut ordinary_content,
            &mut tool_seen,
        );
        inspect_native_responses(payload, &mut evidence);
    }

    if ordinary_content.contains("<think>") && ordinary_content.contains("</think>") {
        evidence.think_tags = true;
    }

    let (semantic, source) = if evidence.opaque {
        (ReasoningSemantic::Opaque, ReasoningSource::NativeResponses)
    } else if evidence.summary {
        (ReasoningSemantic::Summary, ReasoningSource::NativeResponses)
    } else if evidence.reasoning_content {
        (
            ReasoningSemantic::Readable,
            ReasoningSource::ReasoningContent,
        )
    } else if evidence.reasoning {
        (ReasoningSemantic::Readable, ReasoningSource::Reasoning)
    } else if evidence.reasoning_details {
        (
            ReasoningSemantic::Readable,
            ReasoningSource::ReasoningDetails,
        )
    } else if evidence.think_tags {
        (ReasoningSemantic::Readable, ReasoningSource::ThinkTags)
    } else if evidence.native_reasoning_text {
        (
            ReasoningSemantic::Readable,
            ReasoningSource::NativeResponses,
        )
    } else {
        (ReasoningSemantic::None, ReasoningSource::None)
    };

    ClassifiedReasoningShape {
        semantic,
        source,
        pre_tool_visible_content: if evidence.pre_tool_visible_content && tool_seen {
            PreToolVisibleContent::Present
        } else {
            PreToolVisibleContent::Absent
        },
    }
}

#[derive(Default)]
struct Evidence {
    reasoning_content: bool,
    reasoning: bool,
    reasoning_details: bool,
    think_tags: bool,
    native_reasoning_text: bool,
    summary: bool,
    opaque: bool,
    pre_tool_visible_content: bool,
}

fn inspect_chat_choices(
    payload: &Value,
    evidence: &mut Evidence,
    ordinary_content: &mut String,
    tool_seen: &mut bool,
) {
    let Some(choices) = payload.get("choices").and_then(Value::as_array) else {
        return;
    };

    for choice in choices {
        let part = choice
            .get("delta")
            .or_else(|| choice.get("message"))
            .unwrap_or(&Value::Null);
        evidence.reasoning_content |= nonempty(part.get("reasoning_content"));
        evidence.reasoning |= nonempty(part.get("reasoning"));
        evidence.reasoning_details |= nonempty(part.get("reasoning_details"));

        let has_tool_call = nonempty(part.get("tool_calls"));
        if let Some(content) = part.get("content").and_then(Value::as_str) {
            if !content.is_empty() {
                ordinary_content.push_str(content);
                if !*tool_seen && !has_tool_call {
                    evidence.pre_tool_visible_content = true;
                }
            }
        }
        *tool_seen |= has_tool_call;
    }
}

fn inspect_native_responses(payload: &Value, evidence: &mut Evidence) {
    let Some(output) = payload.get("output").and_then(Value::as_array) else {
        return;
    };

    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            continue;
        }
        evidence.opaque |= nonempty(item.get("encrypted_content"));
        evidence.summary |= nonempty(item.get("summary"));
        if let Some(content) = item.get("content").and_then(Value::as_array) {
            for part in content {
                match part.get("type").and_then(Value::as_str) {
                    Some("reasoning_text") if nonempty(part.get("text")) => {
                        evidence.native_reasoning_text = true;
                    }
                    Some("summary_text") if nonempty(part.get("text")) => evidence.summary = true,
                    _ => {}
                }
            }
        }
    }
}

fn nonempty(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(values)) => !values.is_empty(),
        Some(Value::Object(values)) => !values.is_empty(),
        Some(Value::Number(_)) | Some(Value::Bool(_)) => true,
        _ => false,
    }
}
