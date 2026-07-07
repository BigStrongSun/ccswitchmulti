//! Chat-level hosted tool loop.

use super::{
    openai_client::OpenAiHostedToolClient,
    web_search::{
        error_tool_content, parse_arguments, query_hash, result_to_tool_content,
        HostedWebSearchConfig, WEB_SEARCH_FUNCTION_NAME,
    },
};
use serde_json::{json, Value};

pub(crate) const HOSTED_TOOL_LOOP_HEADER: &str = "x-cc-switch-hosted-tool-loop";
pub(crate) const MAX_HOSTED_TOOL_ITERATIONS: usize = 3;

/// 第三方 Chat response 中的 `web_search` tool call。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebSearchToolCall {
    pub(crate) id: String,
    pub(crate) arguments: String,
}

/// Chat tool-call 扫描结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WebSearchCallScan {
    NoToolCalls,
    OnlyWebSearch(Vec<WebSearchToolCall>),
    ContainsUnsupportedToolCalls,
}

/// 扫描 Chat response 是否只请求了本地可执行的 hosted `web_search`。
///
/// 参数:
/// - `chat_response`: 第三方 Chat Completions JSON。
/// 返回:
/// - 没有工具调用、只有 web_search、或混有其它工具调用三种状态。
/// 副作用:
/// - 无。
pub(crate) fn scan_web_search_tool_calls(chat_response: &Value) -> WebSearchCallScan {
    let Some(message) = first_choice_message(chat_response) else {
        return WebSearchCallScan::NoToolCalls;
    };

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        if tool_calls.is_empty() {
            return WebSearchCallScan::NoToolCalls;
        }
        let mut calls = Vec::new();
        for (index, tool_call) in tool_calls.iter().enumerate() {
            let function = tool_call.get("function").unwrap_or(&Value::Null);
            let name = function.get("name").and_then(Value::as_str).unwrap_or("");
            if name != WEB_SEARCH_FUNCTION_NAME {
                return WebSearchCallScan::ContainsUnsupportedToolCalls;
            }
            let id = tool_call
                .get("id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("call_{index}"));
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string();
            calls.push(WebSearchToolCall { id, arguments });
        }
        return WebSearchCallScan::OnlyWebSearch(calls);
    }

    if let Some(function_call) = message.get("function_call") {
        let name = function_call
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if name.is_empty() {
            return WebSearchCallScan::NoToolCalls;
        }
        if name != WEB_SEARCH_FUNCTION_NAME {
            return WebSearchCallScan::ContainsUnsupportedToolCalls;
        }
        let arguments = function_call
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}")
            .to_string();
        return WebSearchCallScan::OnlyWebSearch(vec![WebSearchToolCall {
            id: "call_0".to_string(),
            arguments,
        }]);
    }

    WebSearchCallScan::NoToolCalls
}

/// 将 assistant tool-call message 与 tool output messages 追加到 Chat 请求体。
///
/// 参数:
/// - `chat_request`: 已转换后的 Chat Completions 请求体，会被就地追加 messages。
/// - `chat_response`: 上一轮 Chat response，提供 assistant tool_calls message。
/// - `tool_messages`: 本地执行后的 tool messages。
/// 返回:
/// - `true` 表示成功追加；`false` 表示缺少 messages 或 assistant message。
/// 副作用:
/// - 修改 `chat_request.messages`，并确保后续请求为非流式。
pub(crate) fn append_tool_outputs_to_chat_request(
    chat_request: &mut Value,
    chat_response: &Value,
    tool_messages: Vec<Value>,
) -> bool {
    let Some(assistant_message) = first_choice_message(chat_response).cloned() else {
        return false;
    };
    let Some(messages) = chat_request
        .get_mut("messages")
        .and_then(Value::as_array_mut)
    else {
        return false;
    };

    messages.push(assistant_message);
    messages.extend(tool_messages);
    if let Some(obj) = chat_request.as_object_mut() {
        obj.insert("stream".to_string(), json!(false));
        obj.remove("stream_options");
    }
    true
}

/// 执行一组 web_search tool calls 并生成 Chat tool messages。
///
/// 参数:
/// - `calls`: 第三方模型请求的 web_search 调用。
/// - `config`: Codex 原始 hosted web_search 配置。
/// - `trace_id`: 可选请求 trace id，只用于脱敏日志关联。
/// 返回:
/// - 可追加到 Chat messages 的 `role=tool` 消息。
/// 副作用:
/// - 可能发起 OpenAI Responses 网络请求；日志只记录 query hash、耗时和状态。
pub(crate) async fn execute_web_search_tool_calls(
    calls: &[WebSearchToolCall],
    config: &HostedWebSearchConfig,
    trace_id: Option<&str>,
) -> Vec<Value> {
    let client = OpenAiHostedToolClient::from_env();
    let mut messages = Vec::new();

    for call in calls {
        let args = parse_arguments(&call.arguments);
        let hash = query_hash(&args.query);
        let started = std::time::Instant::now();
        let content = match &client {
            Ok(client) if !args.query.trim().is_empty() => {
                match client.run_web_search(&args, config).await {
                    Ok(result) => {
                        log_hosted_tool_event(
                            trace_id,
                            &hash,
                            "ok",
                            started.elapsed().as_millis(),
                            None,
                        );
                        result_to_tool_content(&result)
                    }
                    Err(err) => {
                        let message = safe_error_message(&err.to_string());
                        log_hosted_tool_event(
                            trace_id,
                            &hash,
                            "error",
                            started.elapsed().as_millis(),
                            Some(&message),
                        );
                        error_tool_content(&args.query, &message)
                    }
                }
            }
            Ok(_) => {
                let message = "web_search query is empty";
                log_hosted_tool_event(
                    trace_id,
                    &hash,
                    "invalid",
                    started.elapsed().as_millis(),
                    Some(message),
                );
                error_tool_content(&args.query, message)
            }
            Err(err) => {
                let message = safe_error_message(err);
                log_hosted_tool_event(
                    trace_id,
                    &hash,
                    "not_configured",
                    started.elapsed().as_millis(),
                    Some(&message),
                );
                error_tool_content(&args.query, &message)
            }
        };

        messages.push(json!({
            "role": "tool",
            "tool_call_id": call.id,
            "content": content
        }));
    }

    messages
}

/// 取第一条 Chat choice message。
fn first_choice_message(chat_response: &Value) -> Option<&Value> {
    chat_response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
}

/// 写入 hosted tool 脱敏诊断事件。
fn log_hosted_tool_event(
    trace_id: Option<&str>,
    query_hash: &str,
    status: &str,
    elapsed_ms: u128,
    error: Option<&str>,
) {
    if let Some(trace_id) = trace_id {
        let mut fields = vec![
            ("trace", trace_id.to_string()),
            ("tool", WEB_SEARCH_FUNCTION_NAME.to_string()),
            ("query_hash", query_hash.to_string()),
            ("status", status.to_string()),
            ("elapsed_ms", elapsed_ms.to_string()),
        ];
        if let Some(error) = error {
            fields.push(("error", error.to_string()));
        }
        crate::proxy::codex_router_log::append_event("hosted_tool_call", &fields);
    }
}

/// 裁剪错误文本，避免把上游长响应或敏感上下文回填给模型。
fn safe_error_message(message: &str) -> String {
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 500 {
        return normalized;
    }
    let mut truncated = normalized.chars().take(500).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_web_search_tool_calls_accepts_only_web_search() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_search",
                        "type": "function",
                        "function": {
                            "name": "web_search",
                            "arguments": "{\"query\":\"Codex\"}"
                        }
                    }]
                }
            }]
        });

        let scan = scan_web_search_tool_calls(&response);

        assert_eq!(
            scan,
            WebSearchCallScan::OnlyWebSearch(vec![WebSearchToolCall {
                id: "call_search".to_string(),
                arguments: "{\"query\":\"Codex\"}".to_string()
            }])
        );
    }

    #[test]
    fn scan_web_search_tool_calls_rejects_mixed_tools() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_file",
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "arguments": "{}"
                        }
                    }]
                }
            }]
        });

        assert_eq!(
            scan_web_search_tool_calls(&response),
            WebSearchCallScan::ContainsUnsupportedToolCalls
        );
    }

    #[test]
    fn append_tool_outputs_to_chat_request_adds_assistant_and_tool_messages() {
        let mut request = json!({
            "model": "deepseek-v4-flash",
            "messages": [{"role": "user", "content": "Search."}],
            "stream": true,
            "stream_options": {"include_usage": true}
        });
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_search",
                        "type": "function",
                        "function": {"name": "web_search", "arguments": "{}"}
                    }]
                }
            }]
        });

        assert!(append_tool_outputs_to_chat_request(
            &mut request,
            &response,
            vec![json!({
                "role": "tool",
                "tool_call_id": "call_search",
                "content": "{}"
            })],
        ));
        assert_eq!(request["messages"].as_array().unwrap().len(), 3);
        assert_eq!(request["stream"], false);
        assert!(request.get("stream_options").is_none());
    }
}
