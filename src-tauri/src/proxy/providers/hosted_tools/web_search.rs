//! Hosted `web_search` bridge primitives.

use crate::proxy::json_canonical::{canonical_json_string, short_sha256_hex};
use serde_json::{json, Value};

pub(crate) const WEB_SEARCH_FUNCTION_NAME: &str = "web_search";
const DEFAULT_MAX_RESULTS: u64 = 5;
const MAX_RESULT_COUNT: u64 = 10;
const MAX_TEXT_CHARS: usize = 4_000;
const MAX_SUMMARY_CHARS: usize = 1_000;
const MAX_SNIPPET_CHARS: usize = 500;

/// Codex 入站 hosted `web_search` 工具的安全子集配置。
///
/// 该结构只保留执行 OpenAI hosted tool 所需的非敏感字段；不会保存用户 prompt、
/// API key 或网页正文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostedWebSearchConfig {
    pub(crate) external_web_access: bool,
    pub(crate) search_content_types: Vec<String>,
}

impl Default for HostedWebSearchConfig {
    fn default() -> Self {
        Self {
            external_web_access: true,
            search_content_types: vec!["text".to_string()],
        }
    }
}

/// 第三方 Chat 模型发起 `web_search` function call 时的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebSearchArguments {
    pub(crate) query: String,
    pub(crate) count: u64,
}

/// 已规整的搜索来源条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebSearchSource {
    pub(crate) title: String,
    pub(crate) url: String,
    pub(crate) snippet: String,
}

/// 已规整的搜索结果，作为普通 tool output 回填给第三方模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebSearchResult {
    pub(crate) query: String,
    pub(crate) summary: String,
    pub(crate) sources: Vec<WebSearchSource>,
    pub(crate) raw_text: String,
}

/// 把单个 Responses hosted tool 定义规整成桥接配置。
///
/// 参数:
/// - `tool`: Codex 原始 `{"type":"web_search"}` tool 定义。
/// 返回:
/// - 去除未知字段后的本地执行配置。
/// 副作用:
/// - 无。
pub(crate) fn config_from_tool(tool: &Value) -> HostedWebSearchConfig {
    let external_web_access = tool
        .get("external_web_access")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut search_content_types = tool
        .get("search_content_types")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| matches!(*value, "text" | "image"))
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["text".to_string()]);
    search_content_types.sort();
    search_content_types.dedup();
    if search_content_types.is_empty() {
        search_content_types.push("text".to_string());
    }

    HostedWebSearchConfig {
        external_web_access,
        search_content_types,
    }
}

/// 生成第三方 Chat 上游可理解的 `web_search` function tool。
///
/// 返回:
/// - OpenAI Chat Completions `tools[]` 条目。
/// 副作用:
/// - 无。
pub(crate) fn chat_tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": WEB_SEARCH_FUNCTION_NAME,
            "description": "Search the web and return concise source-backed results.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The web search query."
                    },
                    "count": {
                        "type": "integer",
                        "description": "Maximum number of search results to use."
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }
        }
    })
}

/// 解析第三方模型传回的 `web_search` function arguments。
///
/// 参数:
/// - `arguments`: Chat tool call 中的 JSON 字符串参数；非 JSON 时按 query 兜底。
/// 返回:
/// - 已裁剪结果数量的搜索参数。
/// 副作用:
/// - 无。
pub(crate) fn parse_arguments(arguments: &str) -> WebSearchArguments {
    let parsed = serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| {
        json!({
            "query": arguments
        })
    });
    let query = parsed
        .get("query")
        .and_then(Value::as_str)
        .or_else(|| parsed.get("q").and_then(Value::as_str))
        .unwrap_or(arguments)
        .trim()
        .to_string();
    let count = parsed
        .get("count")
        .or_else(|| parsed.get("limit"))
        .or_else(|| parsed.get("max_results"))
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_RESULTS)
        .clamp(1, MAX_RESULT_COUNT);

    WebSearchArguments { query, count }
}

/// 把 OpenAI Responses 返回规整成第三方模型可消费的稳定 JSON。
///
/// 参数:
/// - `query`: 本次搜索 query，用于回填和日志关联。
/// - `response`: OpenAI hosted `web_search` Responses JSON。
/// 返回:
/// - 裁剪后的搜索结果。
/// 副作用:
/// - 无。
pub(crate) fn result_from_openai_response(query: &str, response: &Value) -> WebSearchResult {
    let raw_text = truncate_text(&extract_response_text(response), MAX_TEXT_CHARS);
    let summary = truncate_text(&raw_text, MAX_SUMMARY_CHARS);
    let sources = collect_sources(response)
        .into_iter()
        .take(MAX_RESULT_COUNT as usize)
        .collect();

    WebSearchResult {
        query: query.to_string(),
        summary,
        sources,
        raw_text,
    }
}

/// 构造回填给第三方 Chat 模型的 tool message content。
///
/// 参数:
/// - `result`: 已规整的搜索结果。
/// 返回:
/// - JSON 字符串，适合放入 Chat `role=tool` 的 `content` 字段。
/// 副作用:
/// - 无。
pub(crate) fn result_to_tool_content(result: &WebSearchResult) -> String {
    canonical_json_string(&json!({
        "query": result.query,
        "summary": result.summary,
        "sources": result.sources.iter().map(|source| {
            json!({
                "title": source.title,
                "url": source.url,
                "snippet": source.snippet
            })
        }).collect::<Vec<_>>(),
        "raw_text": result.raw_text
    }))
}

/// 构造可让模型继续回答的工具错误输出。
///
/// 参数:
/// - `query`: 原始搜索 query。
/// - `message`: 安全错误摘要，不应包含 API key 或完整网页正文。
/// 返回:
/// - JSON 字符串，适合放入 Chat `role=tool` 的 `content` 字段。
/// 副作用:
/// - 无。
pub(crate) fn error_tool_content(query: &str, message: &str) -> String {
    canonical_json_string(&json!({
        "query": query,
        "summary": "",
        "sources": [],
        "raw_text": "",
        "error": message
    }))
}

/// 对 query 生成短 hash，日志只记录 hash 不记录原文。
///
/// 参数:
/// - `query`: 原始搜索 query。
/// 返回:
/// - 短 SHA-256 十六进制摘要。
/// 副作用:
/// - 无。
pub(crate) fn query_hash(query: &str) -> String {
    short_sha256_hex(query.as_bytes())
}

/// 从 Responses JSON 中提取可读文本。
fn extract_response_text(response: &Value) -> String {
    if let Some(text) = response.get("output_text").and_then(Value::as_str) {
        return text.trim().to_string();
    }

    let mut parts = Vec::new();
    collect_text_parts(response, &mut parts);
    parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 递归收集 OpenAI Responses 输出中的文本片段。
fn collect_text_parts(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_text_parts(item, parts);
            }
        }
        Value::Object(obj) => {
            let item_type = obj.get("type").and_then(Value::as_str).unwrap_or("");
            if matches!(item_type, "output_text" | "text") {
                if let Some(text) = obj.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
            for value in obj.values() {
                collect_text_parts(value, parts);
            }
        }
        _ => {}
    }
}

/// 从 Responses JSON 中收集 URL citation 类来源。
fn collect_sources(response: &Value) -> Vec<WebSearchSource> {
    let mut sources = Vec::new();
    collect_sources_inner(response, &mut sources);
    let mut seen = std::collections::HashSet::new();
    sources
        .into_iter()
        .filter(|source| !source.url.trim().is_empty())
        .filter(|source| seen.insert(source.url.clone()))
        .collect()
}

/// 递归查找 annotation/search result 里的来源字段。
fn collect_sources_inner(value: &Value, sources: &mut Vec<WebSearchSource>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_sources_inner(item, sources);
            }
        }
        Value::Object(obj) => {
            if let Some(url) = obj.get("url").and_then(Value::as_str) {
                let title = obj
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(url)
                    .trim()
                    .to_string();
                let snippet = obj
                    .get("snippet")
                    .or_else(|| obj.get("text"))
                    .and_then(Value::as_str)
                    .map(|text| truncate_text(text, MAX_SNIPPET_CHARS))
                    .unwrap_or_default();
                sources.push(WebSearchSource {
                    title,
                    url: url.trim().to_string(),
                    snippet,
                });
            }
            for value in obj.values() {
                collect_sources_inner(value, sources);
            }
        }
        _ => {}
    }
}

/// 按字符数裁剪文本，避免把大段网页内容塞回模型上下文。
fn truncate_text(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut truncated = normalized.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosted_web_search_config_keeps_safe_fields_only() {
        let config = config_from_tool(&json!({
            "type": "web_search",
            "external_web_access": true,
            "search_content_types": ["image", "text", "binary"]
        }));

        assert!(config.external_web_access);
        assert_eq!(config.search_content_types, vec!["image", "text"]);
    }

    #[test]
    fn parse_arguments_accepts_count_aliases_and_clamps() {
        let parsed = parse_arguments(r#"{"query":"OpenAI Codex","max_results":99}"#);

        assert_eq!(parsed.query, "OpenAI Codex");
        assert_eq!(parsed.count, MAX_RESULT_COUNT);
    }

    #[test]
    fn result_from_openai_response_extracts_text_and_sources() {
        let result = result_from_openai_response(
            "Codex web search",
            &json!({
                "output": [{
                    "type": "message",
                    "content": [{
                        "type": "output_text",
                        "text": "Codex supports web search.",
                        "annotations": [{
                            "type": "url_citation",
                            "title": "Docs",
                            "url": "https://example.com/docs",
                            "snippet": "web search"
                        }]
                    }]
                }]
            }),
        );

        assert_eq!(result.summary, "Codex supports web search.");
        assert_eq!(result.sources[0].title, "Docs");
        assert_eq!(result.sources[0].url, "https://example.com/docs");
    }
}
