//! Shared terminal semantics for Codex protocol adapters.
//!
//! Transport closure is deliberately excluded from these decisions. Callers
//! must provide the upstream protocol's explicit terminal signal together
//! with the structured output they successfully decoded.

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
