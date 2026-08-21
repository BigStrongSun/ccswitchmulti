//! OpenAI hosted tool HTTP client.

use super::{
    image_generation::{
        result_from_openai_response as image_result_from_openai_response,
        HostedImageGenerationConfig, ImageGenerationArguments,
    },
    web_search::{
        result_from_openai_response, HostedWebSearchConfig, WebSearchArguments, WebSearchResult,
    },
};
use crate::proxy::error::ProxyError;
use serde_json::{json, Value};
use std::time::Duration;

const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4.1-mini";
const DEFAULT_CODE_OAUTH_MODEL: &str = "gpt-5.4-mini";
const CODEX_OAUTH_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Hosted tool 调用使用的凭据来源。
#[derive(Debug, Clone, PartialEq, Eq)]
enum HostedToolAuth {
    ApiKey(String),
    CodexOAuth {
        access_token: String,
        account_id: Option<String>,
    },
}

impl HostedToolAuth {
    fn bearer_token(&self) -> &str {
        match self {
            Self::ApiKey(token)
            | Self::CodexOAuth {
                access_token: token,
                ..
            } => token,
        }
    }
}

/// OpenAI hosted tool 调用配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenAiHostedToolClient {
    auth: HostedToolAuth,
    base_url: String,
    model: String,
    timeout: Duration,
}

impl OpenAiHostedToolClient {
    /// 未配置 API Key 时返回 `None`，供 forwarder 回退到 Codex OAuth。
    pub(crate) fn from_env_if_enabled() -> Option<Result<Self, String>> {
        Self::hosted_tool_bridge_env_enabled().then(Self::from_env_inner)
    }

    /// 进程级总开关是否允许 hosted tool bridge。
    pub(crate) fn hosted_tool_bridge_env_enabled() -> bool {
        !env_flag_disabled("CCSWITCH_HOSTED_TOOLS_OPENAI_ENABLED")
    }

    fn from_env_inner() -> Result<Self, String> {
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
            auth: HostedToolAuth::ApiKey(api_key),
            base_url,
            model,
            timeout: Duration::from_millis(timeout_ms),
        })
    }

    /// 使用已登录 Codex OAuth 创建 hosted tool client。
    ///
    /// 不读取 API Key，直接复用 CCSM 托管的 ChatGPT 登录态访问官方 Codex 后端。
    pub(crate) fn from_codex_oauth(access_token: String, account_id: Option<String>) -> Self {
        let model = std::env::var("CCSWITCH_HOSTED_TOOLS_OPENAI_MODEL")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_CODE_OAUTH_MODEL.to_string());
        let timeout_ms = std::env::var("CCSWITCH_HOSTED_TOOLS_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(1_000, 120_000);

        Self {
            auth: HostedToolAuth::CodexOAuth {
                access_token,
                account_id,
            },
            base_url: CODEX_OAUTH_BASE_URL.to_string(),
            model,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    fn apply_oauth_headers(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, ProxyError> {
        let HostedToolAuth::CodexOAuth { account_id, .. } = &self.auth else {
            return Ok(request);
        };
        let originator =
            http::HeaderValue::from_static(crate::proxy::providers::CODEX_OAUTH_ORIGINATOR);
        let mut request = request.header("originator", originator);
        if let Some(account_id) = account_id {
            let value = http::HeaderValue::from_str(account_id).map_err(|e| {
                ProxyError::Internal(format!(
                    "Invalid Codex OAuth account id for hosted tool call: {e}"
                ))
            })?;
            request = request.header("chatgpt-account-id", value);
        }
        Ok(request)
    }

    /// 调用 OpenAI Responses hosted `web_search` 并规整结果。
    ///
    /// 参数:
    /// - `args`: 第三方模型发出的搜索参数。
    /// - `config`: Codex 原始 hosted tool 配置。
    ///
    /// 返回:
    /// - 成功时返回可回填给第三方模型的稳定结果。
    ///
    /// 副作用:
    /// - 通过网络请求 OpenAI Responses API；不会记录 API key 或完整搜索正文。
    pub(crate) async fn run_web_search(
        &self,
        args: &WebSearchArguments,
        config: &HostedWebSearchConfig,
    ) -> Result<WebSearchResult, ProxyError> {
        let url = format!("{}/responses", self.base_url);
        let request_body = self.build_web_search_request(args, config);
        let mut request = crate::proxy::http_client::get()
            .post(&url)
            .bearer_auth(self.auth.bearer_token())
            .json(&request_body)
            .timeout(self.timeout);
        request = self.apply_oauth_headers(request)?;
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                ProxyError::Timeout(format!("OpenAI hosted web_search timed out: {e}"))
            } else {
                ProxyError::ForwardFailed(format!("OpenAI hosted web_search request failed: {e}"))
            }
        })?;

        let status = response.status();
        let body = response.bytes().await.map_err(|e| {
            ProxyError::ForwardFailed(format!(
                "Failed to read OpenAI hosted web_search response: {e}"
            ))
        })?;
        if !status.is_success() {
            let value = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
            return Err(ProxyError::UpstreamError {
                status: status.as_u16(),
                body: Some(summarize_error_body(
                    &value,
                    "OpenAI hosted web_search returned an error",
                )),
            });
        }

        let value = decode_hosted_response(&body, self.uses_codex_oauth_stream())?;
        Ok(result_from_openai_response(&args.query, &value))
    }

    /// 调用 OpenAI Responses hosted `image_generation` 并规整结果。
    pub(crate) async fn run_image_generation(
        &self,
        args: &ImageGenerationArguments,
        config: &HostedImageGenerationConfig,
    ) -> Result<super::image_generation::ImageGenerationResult, ProxyError> {
        let url = format!("{}/responses", self.base_url);
        let request_body = self.build_image_generation_request(args, config);
        let mut request = crate::proxy::http_client::get()
            .post(&url)
            .bearer_auth(self.auth.bearer_token())
            .json(&request_body)
            .timeout(self.timeout);
        request = self.apply_oauth_headers(request)?;
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                ProxyError::Timeout(format!("OpenAI hosted image_generation timed out: {e}"))
            } else {
                ProxyError::ForwardFailed(format!(
                    "OpenAI hosted image_generation request failed: {e}"
                ))
            }
        })?;

        let status = response.status();
        let body = response.bytes().await.map_err(|e| {
            ProxyError::ForwardFailed(format!(
                "Failed to read OpenAI hosted image_generation response: {e}"
            ))
        })?;
        if !status.is_success() {
            let value = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
            return Err(ProxyError::UpstreamError {
                status: status.as_u16(),
                body: Some(summarize_error_body(
                    &value,
                    "OpenAI hosted image_generation returned an error",
                )),
            });
        }

        let value = decode_hosted_response(&body, self.uses_codex_oauth_stream())?;
        image_result_from_openai_response(args, &value)
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
            "input": [{
                "role": "user",
                "content": format!(
                    "Search the web for: {}. Return concise source-backed results. Use at most {} result(s).",
                    args.query,
                    args.count
                )
            }],
            "tools": [tool],
            "tool_choice": { "type": "web_search" },
            "store": false,
            "stream": self.uses_codex_oauth_stream()
        })
    }

    /// 组装 OpenAI Responses hosted `image_generation` 请求体。
    fn build_image_generation_request(
        &self,
        args: &ImageGenerationArguments,
        config: &HostedImageGenerationConfig,
    ) -> Value {
        let mut tool = json!({ "type": "image_generation" });
        if let Some(size) = args.size.as_deref().or(config.size.as_deref()) {
            tool["size"] = json!(size);
        }
        if let Some(quality) = args.quality.as_deref().or(config.quality.as_deref()) {
            tool["quality"] = json!(quality);
        }
        if let Some(format) = args.format.as_deref().or(config.format.as_deref()) {
            tool["format"] = json!(format);
        }
        if let Some(background) = config.background.as_deref() {
            tool["background"] = json!(background);
        }

        json!({
            "model": self.model,
            "input": [{
                "role": "user",
                "content": format!("Generate an image: {}", args.prompt)
            }],
            "tools": [tool],
            "tool_choice": { "type": "image_generation" },
            "store": false,
            "stream": self.uses_codex_oauth_stream()
        })
    }

    fn uses_codex_oauth_stream(&self) -> bool {
        matches!(&self.auth, HostedToolAuth::CodexOAuth { .. })
    }
}

fn decode_hosted_response(body: &[u8], expects_sse: bool) -> Result<Value, ProxyError> {
    if expects_sse {
        let body = std::str::from_utf8(body).map_err(|e| {
            ProxyError::TransformError(format!("OpenAI hosted tool returned non-UTF-8 SSE: {e}"))
        })?;
        return crate::proxy::handlers::responses_sse_to_response_value(body);
    }

    serde_json::from_slice(body).map_err(|e| {
        ProxyError::ForwardFailed(format!("Failed to parse OpenAI hosted tool response: {e}"))
    })
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
fn summarize_error_body(value: &Value, fallback: &str) -> String {
    let message = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("detail"))
        .and_then(Value::as_str)
        .unwrap_or(fallback);
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
            auth: HostedToolAuth::ApiKey("sk-test".to_string()),
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
        assert_eq!(request["tool_choice"], json!({"type": "web_search"}));
        assert!(request["input"].is_array());
        assert_eq!(request["stream"], false);
        assert_eq!(request["store"], false);
    }

    #[test]
    fn build_web_search_request_uses_codex_oauth_stream_shape() {
        let client = OpenAiHostedToolClient {
            auth: HostedToolAuth::CodexOAuth {
                access_token: "oauth-test".to_string(),
                account_id: Some("acct-test".to_string()),
            },
            base_url: CODEX_OAUTH_BASE_URL.to_string(),
            model: DEFAULT_CODE_OAUTH_MODEL.to_string(),
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        };

        let request = client.build_web_search_request(
            &WebSearchArguments {
                query: "Codex hosted tools".to_string(),
                count: 2,
            },
            &HostedWebSearchConfig::default(),
        );

        assert!(request["input"].is_array());
        assert_eq!(request["input"][0]["role"], "user");
        assert_eq!(request["stream"], true);
    }

    #[test]
    fn build_image_generation_request_merges_tool_options() {
        let client = OpenAiHostedToolClient {
            auth: HostedToolAuth::ApiKey("sk-test".to_string()),
            base_url: DEFAULT_OPENAI_BASE_URL.to_string(),
            model: "gpt-test".to_string(),
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        };
        let args = ImageGenerationArguments {
            prompt: "a robot".to_string(),
            size: Some("1024x1024".to_string()),
            quality: Some("high".to_string()),
            format: Some("png".to_string()),
        };
        let config = HostedImageGenerationConfig {
            size: None,
            quality: None,
            format: None,
            background: Some("transparent".to_string()),
        };

        let request = client.build_image_generation_request(&args, &config);

        assert_eq!(request["model"], "gpt-test");
        assert_eq!(request["tools"][0]["type"], "image_generation");
        assert_eq!(request["tools"][0]["size"], "1024x1024");
        assert_eq!(request["tools"][0]["quality"], "high");
        assert_eq!(request["tools"][0]["format"], "png");
        assert_eq!(request["tools"][0]["background"], "transparent");
        assert_eq!(request["tool_choice"]["type"], "image_generation");
        assert_eq!(request["store"], false);
    }

    #[test]
    fn from_codex_oauth_uses_official_backend_and_default_model() {
        let client = OpenAiHostedToolClient::from_codex_oauth(
            "token-test".to_string(),
            Some("acct_1".to_string()),
        );

        assert_eq!(client.base_url, "https://chatgpt.com/backend-api/codex");
        assert_eq!(client.model, "gpt-5.4-mini");
        assert!(matches!(
            client.auth,
            HostedToolAuth::CodexOAuth { account_id: Some(ref id), .. } if id == "acct_1"
        ));
    }

    #[test]
    fn decode_hosted_response_accepts_codex_oauth_sse() {
        let body = br#"event: response.output_item.done
data: {"type":"response.output_item.done","item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"search result"}]}}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_1","status":"completed","output":[]}}

"#;

        let response = decode_hosted_response(body, true).expect("SSE response");

        assert_eq!(response["status"], "completed");
        assert_eq!(response["output"][0]["type"], "message");
        assert_eq!(response["output"][0]["content"][0]["text"], "search result");
    }

    #[test]
    fn summarize_error_body_includes_codex_detail() {
        let summary =
            summarize_error_body(&json!({ "detail": "Input must be a list" }), "fallback");

        assert_eq!(summary, "Input must be a list");
    }
}
