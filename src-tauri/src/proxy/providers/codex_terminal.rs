//! Shared terminal semantics for Codex protocol adapters.
//!
//! Transport closure is deliberately excluded from these decisions. Callers
//! must provide the upstream protocol's explicit terminal signal together
//! with the structured output they successfully decoded.

use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ChatTerminalEvidence {
    pub(crate) has_final_message: bool,
    pub(crate) valid_tool_calls: usize,
    pub(crate) dropped_tool_calls: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TerminalDisposition {
    Completed,
    Incomplete { reason: &'static str },
    Failed { code: &'static str, message: String },
}

impl TerminalDisposition {
    pub(crate) fn status(&self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Incomplete { .. } => "incomplete",
            Self::Failed { .. } => "failed",
        }
    }
}

pub(crate) fn classify_chat_terminal(
    finish_reason: Option<&str>,
    evidence: ChatTerminalEvidence,
) -> TerminalDisposition {
    match finish_reason {
        Some("length") => TerminalDisposition::Incomplete {
            reason: "max_output_tokens",
        },
        Some("content_filter") => TerminalDisposition::Incomplete {
            reason: "content_filter",
        },
        Some(reason @ ("tool_calls" | "function_call")) => {
            if evidence.valid_tool_calls > 0 {
                TerminalDisposition::Completed
            } else if evidence.dropped_tool_calls > 0 {
                TerminalDisposition::Failed {
                    code: "upstream_tool_call_dropped",
                    message: format!(
                        "Upstream returned {} tool call(s) without a function name, leaving no usable tool call in this turn",
                        evidence.dropped_tool_calls
                    ),
                }
            } else {
                TerminalDisposition::Failed {
                    code: "upstream_tool_call_missing",
                    message: format!(
                        "Upstream finish_reason={reason} did not include a complete tool call"
                    ),
                }
            }
        }
        Some("stop") => {
            if evidence.has_final_message || evidence.valid_tool_calls > 0 {
                TerminalDisposition::Completed
            } else {
                TerminalDisposition::Failed {
                    code: "upstream_final_output_missing",
                    message: "Upstream finish_reason=stop did not include a final output message or complete tool call"
                        .to_string(),
                }
            }
        }
        None => TerminalDisposition::Failed {
            code: "upstream_finish_reason_missing",
            message: "Upstream Chat Completions response ended without finish_reason".to_string(),
        },
        Some(reason) => TerminalDisposition::Failed {
            code: "upstream_finish_reason_unknown",
            message: format!("Upstream returned unknown finish_reason={reason}"),
        },
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct NativeResponsesEvidence {
    pub(crate) has_final_message: bool,
    pub(crate) has_compaction_output: bool,
    pub(crate) valid_tool_calls: usize,
    pub(crate) dropped_tool_calls: usize,
}

impl NativeResponsesEvidence {
    pub(crate) fn observe_event(&mut self, event_name: &str, payload: &Value) {
        match event_name {
            "response.output_text.delta" | "response.output_text.done" => {
                self.has_final_message |= payload
                    .get(if event_name.ends_with(".delta") {
                        "delta"
                    } else {
                        "text"
                    })
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty());
            }
            "response.refusal.delta" | "response.refusal.done" => {
                self.has_final_message |= payload
                    .get(if event_name.ends_with(".delta") {
                        "delta"
                    } else {
                        "refusal"
                    })
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty());
            }
            "response.output_item.done" => {
                if let Some(item) = payload.get("item") {
                    self.observe_output_item(item);
                }
            }
            "response.completed" => {
                if let Some(output) = payload
                    .pointer("/response/output")
                    .and_then(Value::as_array)
                {
                    for item in output {
                        self.observe_output_item(item);
                    }
                }
            }
            _ => {}
        }
    }

    fn observe_output_item(&mut self, item: &Value) {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => {
                self.has_final_message |= item
                    .get("content")
                    .and_then(Value::as_array)
                    .is_some_and(|content| content.iter().any(is_nonempty_final_content));
            }
            Some("compaction") => {
                self.has_compaction_output |= is_nonempty_string(item.get("encrypted_content"));
            }
            Some(item_type) if is_client_tool_call_type(item_type) => {
                if is_complete_client_tool_call(item_type, item) {
                    self.valid_tool_calls += 1;
                } else {
                    self.dropped_tool_calls += 1;
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeResponsesTerminalDisposition {
    Completed,
    Incomplete,
    Failed,
    ProtocolError { code: &'static str, message: String },
}

pub(crate) fn classify_native_responses_terminal(
    event_name: &str,
    payload: &Value,
    evidence: NativeResponsesEvidence,
) -> Option<NativeResponsesTerminalDisposition> {
    let response_status = payload.pointer("/response/status").and_then(Value::as_str);

    match event_name {
        "response.completed" => {
            if response_status != Some("completed") {
                return Some(NativeResponsesTerminalDisposition::ProtocolError {
                    code: "upstream_terminal_status_mismatch",
                    message: format!(
                        "Upstream response.completed carried status={}",
                        response_status.unwrap_or("missing")
                    ),
                });
            }
            if evidence.has_final_message
                || evidence.has_compaction_output
                || evidence.valid_tool_calls > 0
            {
                Some(NativeResponsesTerminalDisposition::Completed)
            } else {
                let detail = if evidence.dropped_tool_calls > 0 {
                    format!(
                        "; {} client tool call(s) were structurally incomplete",
                        evidence.dropped_tool_calls
                    )
                } else {
                    String::new()
                };
                Some(NativeResponsesTerminalDisposition::ProtocolError {
                    code: "upstream_final_output_missing",
                    message: format!(
                        "Upstream response.completed did not include final output text, refusal, compaction output, or a complete client tool call{detail}"
                    ),
                })
            }
        }
        "response.incomplete" => {
            if response_status.is_some_and(|status| status != "incomplete") {
                Some(NativeResponsesTerminalDisposition::ProtocolError {
                    code: "upstream_terminal_status_mismatch",
                    message: format!(
                        "Upstream response.incomplete carried status={}",
                        response_status.unwrap_or("missing")
                    ),
                })
            } else {
                Some(NativeResponsesTerminalDisposition::Incomplete)
            }
        }
        "response.failed" => {
            if response_status.is_some_and(|status| status != "failed") {
                Some(NativeResponsesTerminalDisposition::ProtocolError {
                    code: "upstream_terminal_status_mismatch",
                    message: format!(
                        "Upstream response.failed carried status={}",
                        response_status.unwrap_or("missing")
                    ),
                })
            } else {
                Some(NativeResponsesTerminalDisposition::Failed)
            }
        }
        "error" | "response.error" | "response.cancelled" | "response.canceled"
        | "response.aborted" => Some(NativeResponsesTerminalDisposition::Failed),
        _ => None,
    }
}

fn is_nonempty_string(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
}

fn is_nonempty_final_content(content: &Value) -> bool {
    match content.get("type").and_then(Value::as_str) {
        Some("output_text") => is_nonempty_string(content.get("text")),
        Some("refusal") => is_nonempty_string(content.get("refusal")),
        _ => false,
    }
}

fn is_client_tool_call_type(item_type: &str) -> bool {
    matches!(
        item_type,
        "function_call"
            | "custom_tool_call"
            | "tool_search_call"
            | "local_shell_call"
            | "shell_call"
    )
}

fn is_complete_client_tool_call(item_type: &str, item: &Value) -> bool {
    let status_is_usable = item
        .get("status")
        .and_then(Value::as_str)
        .is_none_or(|status| status == "completed");
    if !status_is_usable || !is_nonempty_string(item.get("call_id")) {
        return false;
    }

    match item_type {
        "function_call" => {
            is_nonempty_string(item.get("name")) && is_nonempty_string(item.get("arguments"))
        }
        "custom_tool_call" => {
            is_nonempty_string(item.get("name")) && is_nonempty_string(item.get("input"))
        }
        "tool_search_call" => {
            item.get("execution").and_then(Value::as_str) == Some("client")
                && item
                    .get("arguments")
                    .is_some_and(|arguments| !arguments.is_null())
        }
        "local_shell_call" | "shell_call" => {
            item.get("action").is_some_and(|action| action.is_object())
        }
        _ => false,
    }
}
