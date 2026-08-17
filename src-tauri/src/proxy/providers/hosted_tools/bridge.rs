//! Chat-level hosted tool loop.

use super::{
    image_generation::{
        self, error_tool_content as image_error_tool_content,
        result_to_tool_content as image_result_to_tool_content, HostedImageGenerationConfig,
        IMAGE_GENERATION_FUNCTION_NAME,
    },
    openai_client::OpenAiHostedToolClient,
    web_search::{
        error_tool_content, parse_arguments, query_hash, result_to_tool_content,
        HostedWebSearchConfig, WEB_SEARCH_FUNCTION_NAME,
    },
};
use serde_json::{json, Value};

pub(crate) const HOSTED_TOOL_LOOP_HEADER: &str = "x-cc-switch-hosted-tool-loop";
pub(crate) const MAX_HOSTED_TOOL_ITERATIONS: usize = 3;

/// Codex 入站 hosted tools 的本地执行配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HostedToolLoopConfig {
    pub(crate) web_search: Option<HostedWebSearchConfig>,
    pub(crate) image_generation: Option<HostedImageGenerationConfig>,
}

impl HostedToolLoopConfig {
    pub(crate) fn is_empty(&self) -> bool {
        self.web_search.is_none() && self.image_generation.is_none()
    }
}

/// 已桥接的 hosted tool 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostedToolCallKind {
    WebSearch,
    ImageGeneration,
}

impl HostedToolCallKind {
    fn from_function_name(name: &str) -> Option<Self> {
        match name {
            WEB_SEARCH_FUNCTION_NAME => Some(Self::WebSearch),
            IMAGE_GENERATION_FUNCTION_NAME => Some(Self::ImageGeneration),
            _ => None,
        }
    }
}

/// 第三方 Chat response 中的一个 hosted tool call。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostedToolCall {
    pub(crate) kind: HostedToolCallKind,
    pub(crate) id: String,
    pub(crate) arguments: String,
}

/// Chat tool-call 扫描结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostedToolCallScan {
    NoToolCalls,
    OnlyHosted(Vec<HostedToolCall>),
    ContainsUnsupportedToolCalls,
}

/// 扫描 Chat response 是否只请求了本地可执行的 hosted tools。
pub(crate) fn scan_hosted_tool_calls(chat_response: &Value) -> HostedToolCallScan {
    let Some(message) = first_choice_message(chat_response) else {
        return HostedToolCallScan::NoToolCalls;
    };

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        if tool_calls.is_empty() {
            return HostedToolCallScan::NoToolCalls;
        }
        let mut calls = Vec::new();
        for (index, tool_call) in tool_calls.iter().enumerate() {
            let function = tool_call.get("function").unwrap_or(&Value::Null);
            let name = function.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(kind) = HostedToolCallKind::from_function_name(name) else {
                return HostedToolCallScan::ContainsUnsupportedToolCalls;
            };
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
            calls.push(HostedToolCall {
                kind,
                id,
                arguments,
            });
        }
        return HostedToolCallScan::OnlyHosted(calls);
    }

    if let Some(function_call) = message.get("function_call") {
        let name = function_call
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("");
        if name.is_empty() {
            return HostedToolCallScan::NoToolCalls;
        }
        let Some(kind) = HostedToolCallKind::from_function_name(name) else {
            return HostedToolCallScan::ContainsUnsupportedToolCalls;
        };
        let arguments = function_call
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}")
            .to_string();
        return HostedToolCallScan::OnlyHosted(vec![HostedToolCall {
            kind,
            id: "call_0".to_string(),
            arguments,
        }]);
    }

    HostedToolCallScan::NoToolCalls
}

/// 将 assistant tool-call message 与 tool output messages 追加到 Chat 请求体。
///
/// 返回:
/// - `true` 表示成功追加；`false` 表示缺少 messages 或 assistant message。
///
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

/// 执行一组 hosted tool calls 并生成 Chat tool messages。
pub(crate) async fn execute_hosted_tool_calls(
    calls: &[HostedToolCall],
    config: &HostedToolLoopConfig,
    client: &Result<OpenAiHostedToolClient, String>,
    trace_id: Option<&str>,
) -> Vec<Value> {
    let mut messages = Vec::new();

    for call in calls {
        let content = match call.kind {
            HostedToolCallKind::WebSearch => {
                execute_web_search_call(client, call, config.web_search.as_ref(), trace_id).await
            }
            HostedToolCallKind::ImageGeneration => {
                execute_image_generation_call(
                    client,
                    call,
                    config.image_generation.as_ref(),
                    trace_id,
                )
                .await
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

async fn execute_web_search_call(
    client: &Result<OpenAiHostedToolClient, String>,
    call: &HostedToolCall,
    config: Option<&HostedWebSearchConfig>,
    trace_id: Option<&str>,
) -> String {
    let Some(config) = config else {
        return error_tool_content(
            &call.arguments,
            "web_search hosted tool is not configured for this request",
        );
    };
    let args = parse_arguments(&call.arguments);
    let hash = query_hash(&args.query);
    let started = std::time::Instant::now();
    match client {
        Ok(client) if !args.query.trim().is_empty() => {
            match client.run_web_search(&args, config).await {
                Ok(result) => {
                    log_hosted_tool_event(
                        trace_id,
                        WEB_SEARCH_FUNCTION_NAME,
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
                        WEB_SEARCH_FUNCTION_NAME,
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
                WEB_SEARCH_FUNCTION_NAME,
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
                WEB_SEARCH_FUNCTION_NAME,
                &hash,
                "not_configured",
                started.elapsed().as_millis(),
                Some(&message),
            );
            error_tool_content(&args.query, &message)
        }
    }
}

async fn execute_image_generation_call(
    client: &Result<OpenAiHostedToolClient, String>,
    call: &HostedToolCall,
    config: Option<&HostedImageGenerationConfig>,
    trace_id: Option<&str>,
) -> String {
    let Some(config) = config else {
        return image_error_tool_content(
            &call.arguments,
            "image_generation hosted tool is not configured for this request",
        );
    };
    let args = image_generation::parse_arguments(&call.arguments, config);
    let hash = image_generation::prompt_hash(&args.prompt);
    let started = std::time::Instant::now();
    match client {
        Ok(client) if !args.prompt.trim().is_empty() => {
            match client.run_image_generation(&args, config).await {
                Ok(result) => {
                    log_hosted_tool_event(
                        trace_id,
                        IMAGE_GENERATION_FUNCTION_NAME,
                        &hash,
                        "ok",
                        started.elapsed().as_millis(),
                        None,
                    );
                    image_result_to_tool_content(&result)
                }
                Err(err) => {
                    let message = safe_error_message(&err.to_string());
                    log_hosted_tool_event(
                        trace_id,
                        IMAGE_GENERATION_FUNCTION_NAME,
                        &hash,
                        "error",
                        started.elapsed().as_millis(),
                        Some(&message),
                    );
                    image_error_tool_content(&args.prompt, &message)
                }
            }
        }
        Ok(_) => {
            let message = "image_generation prompt is empty";
            log_hosted_tool_event(
                trace_id,
                IMAGE_GENERATION_FUNCTION_NAME,
                &hash,
                "invalid",
                started.elapsed().as_millis(),
                Some(message),
            );
            image_error_tool_content(&args.prompt, message)
        }
        Err(err) => {
            let message = safe_error_message(err);
            log_hosted_tool_event(
                trace_id,
                IMAGE_GENERATION_FUNCTION_NAME,
                &hash,
                "not_configured",
                started.elapsed().as_millis(),
                Some(&message),
            );
            image_error_tool_content(&args.prompt, &message)
        }
    }
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
    tool: &str,
    query_hash: &str,
    status: &str,
    elapsed_ms: u128,
    error: Option<&str>,
) {
    if let Some(trace_id) = trace_id {
        let mut fields = vec![
            ("trace", trace_id.to_string()),
            ("tool", tool.to_string()),
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
    fn hosted_tool_loop_config_is_empty_only_when_no_tools() {
        assert!(HostedToolLoopConfig::default().is_empty());
        assert!(!HostedToolLoopConfig {
            web_search: Some(HostedWebSearchConfig::default()),
            ..HostedToolLoopConfig::default()
        }
        .is_empty());
        assert!(!HostedToolLoopConfig {
            image_generation: Some(HostedImageGenerationConfig::default()),
            ..HostedToolLoopConfig::default()
        }
        .is_empty());
    }

    #[test]
    fn scan_hosted_tool_calls_accepts_web_search_and_image_generation() {
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [
                        {
                            "id": "call_search",
                            "type": "function",
                            "function": {
                                "name": "web_search",
                                "arguments": "{\"query\":\"Codex\"}"
                            }
                        },
                        {
                            "id": "call_image",
                            "type": "function",
                            "function": {
                                "name": "generate_image",
                                "arguments": "{\"prompt\":\"robot\"}"
                            }
                        }
                    ]
                }
            }]
        });

        assert_eq!(
            scan_hosted_tool_calls(&response),
            HostedToolCallScan::OnlyHosted(vec![
                HostedToolCall {
                    kind: HostedToolCallKind::WebSearch,
                    id: "call_search".to_string(),
                    arguments: "{\"query\":\"Codex\"}".to_string()
                },
                HostedToolCall {
                    kind: HostedToolCallKind::ImageGeneration,
                    id: "call_image".to_string(),
                    arguments: "{\"prompt\":\"robot\"}".to_string()
                }
            ])
        );
    }

    #[test]
    fn scan_hosted_tool_calls_rejects_mixed_tools() {
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
            scan_hosted_tool_calls(&response),
            HostedToolCallScan::ContainsUnsupportedToolCalls
        );
    }

    #[test]
    fn append_tool_outputs_to_chat_request_adds_assistant_and_tool_messages() {
        let mut request = json!({
            "model": "deepseek-v4-flash",
            "messages": [{"role": "user", "content": "Generate."}],
            "stream": true,
            "stream_options": {"include_usage": true}
        });
        let response = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_image",
                        "type": "function",
                        "function": {"name": "generate_image", "arguments": "{}"}
                    }]
                }
            }]
        });

        assert!(append_tool_outputs_to_chat_request(
            &mut request,
            &response,
            vec![json!({
                "role": "tool",
                "tool_call_id": "call_image",
                "content": "{}"
            })],
        ));
        assert_eq!(request["messages"].as_array().unwrap().len(), 3);
        assert_eq!(request["stream"], false);
        assert!(request.get("stream_options").is_none());
    }
}
