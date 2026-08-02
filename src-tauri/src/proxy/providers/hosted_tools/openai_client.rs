//! OpenAI hosted tool HTTP client.

use super::web_search::{
    result_from_openai_response, HostedWebSearchConfig, WebSearchArguments, WebSearchResult,
};
use crate::proxy::error::ProxyError;
use serde_json::{json, Value};
use std::time::Duration;

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4.1-mini";
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// OpenAI hosted tool 调用配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenAiHostedToolClient {
    api_key: String,
    base_url: String,
    model: String,
    timeout: Duration,
}

impl OpenAiHostedToolClient {
    /// 从环境变量创建 hosted tool client。
    ///
    /// 参数:
    /// - 无。
    /// 返回:
    /// - 成功时返回 client；未配置独立 OpenAI key 时返回安全错误文本。
    /// 副作用:
    /// - 读取进程环境变量，不访问磁盘。
    pub(crate) fn from_env() -> Result<Self, String> {
        if env_flag_disabled("CCSWITCH_HOSTED_TOOLS_OPENAI_ENABLED") {
            return Err("OpenAI hosted tool bridge is disabled by environment".to_string());
        }

        let api_key = std::env::var("CCSWITCH_HOSTED_TOOLS_OPENAI_API_KEY")
            .ok()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                "OpenAI hosted tool bridge requires CCSWITCH_HOSTED_TOOLS_OPENAI_API_KEY or OPENAI_API_KEY".to_string()
            })?;
        let base_url = std::env::var("CCSWITCH_HOSTED_TOOLS_OPENAI_BASE_URL")
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string());
        let model = std::env::var("CCSWITCH_HOSTED_TOOLS_OPENAI_MODEL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_OPENAI_MODEL.to_string());
        let timeout_ms = std::env::var("CCSWITCH_HOSTED_TOOLS_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(1_000, 120_000);

        Ok(Self {
            api_key,
            base_url,
            model,
            timeout: Duration::from_millis(timeout_ms),
        })
    }

    /// 调用 OpenAI Responses hosted `web_search` 并规整结果。
    ///
    /// 参数:
    /// - `args`: 第三方模型发出的搜索参数。
    /// - `config`: Codex 原始 hosted tool 配置。
    /// 返回:
    /// - 成功时返回可回填给第三方模型的稳定结果。
    /// 副作用:
    /// - 通过网络请求 OpenAI Responses API；不会记录 API key 或完整搜索正文。
    pub(crate) async fn run_web_search(
        &self,
        args: &WebSearchArguments,
        config: &HostedWebSearchConfig,
    ) -> Result<WebSearchResult, ProxyError> {
        let url = format!("{}/responses", self.base_url);
        let request_body = self.build_web_search_request(args, config);
        let response = crate::proxy::http_client::get()
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ProxyError::Timeout(format!("OpenAI hosted web_search timed out: {e}"))
                } else {
                    ProxyError::ForwardFailed(format!(
                        "OpenAI hosted web_search request failed: {e}"
                    ))
                }
            })?;

        let status = response.status();
        let value: Value = response.json().await.map_err(|e| {
            ProxyError::ForwardFailed(format!(
                "Failed to parse OpenAI hosted web_search response: {e}"
            ))
        })?;
        if !status.is_success() {
            return Err(ProxyError::UpstreamError {
                status: status.as_u16(),
                body: Some(summarize_error_body(&value)),
            });
        }

        Ok(result_from_openai_response(&args.query, &value))
    }

    /// 组装 OpenAI Responses hosted `web_search` 请求体。
    fn build_web_search_request(
        &self,
        args: &WebSearchArguments,
        config: &HostedWebSearchConfig,
    ) -> Value {
        let mut tool = json!({
            "type": "web_search",
            "search_content_types": config.search_content_types
        });
        if config.external_web_access {
            tool["external_web_access"] = json!(true);
        }

        json!({
            "model": self.model,
            "input": format!(
                "Search the web for: {}. Return concise source-backed results. Use at most {} result(s).",
                args.query,
                args.count
            ),
            "tools": [tool],
            "tool_choice": "auto",
            "store": false,
            "stream": false
        })
    }
}

/// 判断布尔环境变量是否显式关闭。
fn env_flag_disabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "disabled"
            )
        })
        .unwrap_or(false)
}

/// 将 OpenAI 错误体裁剪成可安全回填的短文本。
fn summarize_error_body(value: &Value) -> String {
    let message = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("OpenAI hosted web_search returned an error");
    if message.chars().count() <= 500 {
        message.to_string()
    } else {
        let mut truncated = message.chars().take(500).collect::<String>();
        truncated.push_str("...");
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_web_search_request_keeps_original_content_types() {
        let client = OpenAiHostedToolClient {
            api_key: "sk-test".to_string(),
            base_url: DEFAULT_OPENAI_BASE_URL.to_string(),
            model: "gpt-test".to_string(),
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        };
        let request = client.build_web_search_request(
            &WebSearchArguments {
                query: "Codex hosted tools".to_string(),
                count: 3,
            },
            &HostedWebSearchConfig {
                external_web_access: true,
                search_content_types: vec!["image".to_string(), "text".to_string()],
            },
        );

        assert_eq!(request["model"], "gpt-test");
        assert_eq!(request["tools"][0]["type"], "web_search");
        assert_eq!(request["tools"][0]["search_content_types"][0], "image");
        assert_eq!(request["store"], false);
    }
}
