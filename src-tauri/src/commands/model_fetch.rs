//! 模型列表获取命令
//!
//! 提供 Tauri 命令，供前端在供应商表单中获取可用模型列表。

use crate::services::model_fetch::{self, FetchedModel};
use reqwest::header::{HeaderValue, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeModelRef {
    pub provider_id: String,
    pub model_id: String,
}

const OPENCODE_MODELS_TIMEOUT: Duration = Duration::from_secs(20);

/// 获取 OpenCode 当前运行时实际可用的模型，而不局限于静态配置文件。
#[tauri::command]
pub async fn get_opencode_models() -> Result<Vec<OpenCodeModelRef>, String> {
    tokio::task::spawn_blocking(|| {
        let config_dir = crate::opencode_config::get_opencode_dir();
        let config_dir_env = config_dir.to_string_lossy().into_owned();
        let extra_env = [
            ("OPENCODE_CONFIG_DIR", config_dir_env),
            ("OPENCODE_DISABLE_PROJECT_CONFIG", "true".to_string()),
        ];
        let output = super::misc::run_detected_tool_command_with_timeout(
            "opencode",
            &["models"],
            Some(OPENCODE_MODELS_TIMEOUT),
            &extra_env,
            &config_dir,
        )?;
        if !output.status.success() {
            let stderr = super::misc::decode_command_output(&output.stderr);
            let stdout = super::misc::decode_command_output(&output.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            return Err(if detail.is_empty() {
                "Failed to load OpenCode models".to_string()
            } else {
                format!("Failed to load OpenCode models: {detail}")
            });
        }

        Ok(parse_opencode_models(&super::misc::decode_command_output(
            &output.stdout,
        )))
    })
    .await
    .map_err(|e| format!("OpenCode model discovery task failed: {e}"))?
}

fn parse_opencode_models(output: &str) -> Vec<OpenCodeModelRef> {
    output
        .lines()
        .filter_map(|line| {
            let (provider_id, model_id) = line.trim().split_once('/')?;
            if provider_id.is_empty()
                || model_id.is_empty()
                || !provider_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                || model_id
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control())
            {
                return None;
            }
            Some((provider_id.to_string(), model_id.to_string()))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|(provider_id, model_id)| OpenCodeModelRef {
            provider_id,
            model_id,
        })
        .collect()
}

/// 前端模型列表刷新命令的完整请求体。
///
/// 这个结构体保持 Tauri 命令只有一个入参，避免新增供应商专用字段后继续扩散成
/// 多个并列参数；字段名由 `camelCase` 与前端 `invoke` 请求保持一致。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchModelsForConfigRequest {
    pub base_url: String,
    pub api_key: String,
    pub is_full_url: Option<bool>,
    pub models_url: Option<String>,
    pub custom_user_agent: Option<String>,
    pub volcengine_model_list_action: Option<String>,
    pub volcengine_access_key_id: Option<String>,
    pub volcengine_secret_access_key: Option<String>,
}

/// Codex 上游协议探测结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexResponsesProbeResult {
    pub ok: bool,
    pub status: Option<u16>,
    pub url: String,
    pub model: String,
    pub detail: String,
}

/// 获取供应商的可用模型列表
///
/// 使用 OpenAI 兼容的 GET /v1/models 端点。优先使用 `models_url` 精确覆写；
/// 否则对 baseURL 生成候选列表（含「剥离 Anthropic 兼容子路径」兜底），按序尝试。
#[tauri::command(rename_all = "camelCase")]
pub async fn fetch_models_for_config(
    request: FetchModelsForConfigRequest,
) -> Result<Vec<FetchedModel>, String> {
    // 与转发 / 检测路径共用 parse_custom_user_agent：非法 UA 静默忽略（不阻断取模型）。
    let user_agent = crate::provider::parse_custom_user_agent(request.custom_user_agent.as_deref())
        .ok()
        .flatten();
    model_fetch::fetch_models(model_fetch::FetchModelsRequest {
        base_url: &request.base_url,
        api_key: &request.api_key,
        is_full_url: request.is_full_url.unwrap_or(false),
        models_url_override: request.models_url.as_deref(),
        user_agent,
        volcengine: model_fetch::VolcengineModelListRequest {
            action: request.volcengine_model_list_action.as_deref(),
            access_key_id: request.volcengine_access_key_id.as_deref(),
            secret_access_key: request.volcengine_secret_access_key.as_deref(),
        },
    })
    .await
}

/// 对指定模型发起最小 `/v1/responses` 探测。
///
/// 该命令只验证 provider 是否能直接接受 Codex Responses API 形态；Chat-only
/// provider 失败并不等于 MultiRouter 运行失败，前端会结合 apiFormat 判定是否阻塞继续。
#[tauri::command(rename_all = "camelCase")]
pub async fn probe_codex_responses_for_config(
    base_url: String,
    api_key: String,
    model: String,
    is_full_url: Option<bool>,
    custom_user_agent: Option<String>,
) -> Result<CodexResponsesProbeResult, String> {
    if api_key.trim().is_empty() {
        return Err("API Key is required to probe /v1/responses".to_string());
    }
    if model.trim().is_empty() {
        return Err("Model is required to probe /v1/responses".to_string());
    }

    let url = build_responses_probe_url(&base_url, is_full_url.unwrap_or(false))?;
    let user_agent = crate::provider::parse_custom_user_agent(custom_user_agent.as_deref())
        .ok()
        .flatten();
    probe_codex_responses(&url, &api_key, &model, user_agent).await
}

/// 对指定模型发起最小 `/v1/chat/completions` 探测。
///
/// 该命令与 Responses 探测配对使用：只有两个协议都失败时，前端才把问题归类为
/// 凭据、网络、模型权限或上游不可用；单一协议失败只说明该协议路径不适合当前 provider。
#[tauri::command(rename_all = "camelCase")]
pub async fn probe_codex_chat_for_config(
    base_url: String,
    api_key: String,
    model: String,
    is_full_url: Option<bool>,
    custom_user_agent: Option<String>,
) -> Result<CodexResponsesProbeResult, String> {
    if api_key.trim().is_empty() {
        return Err("API Key is required to probe /v1/chat/completions".to_string());
    }
    if model.trim().is_empty() {
        return Err("Model is required to probe /v1/chat/completions".to_string());
    }

    let url = build_chat_probe_url(&base_url, is_full_url.unwrap_or(false))?;
    let user_agent = crate::provider::parse_custom_user_agent(custom_user_agent.as_deref())
        .ok()
        .flatten();
    probe_codex_chat(&url, &api_key, &model, user_agent).await
}

/// 构造 Responses 探测 URL。
///
/// 用户可能填写 provider 根地址、`/v1` 地址、智谱这类 `/v4` 版本化根地址，
/// 也可能把完整 endpoint 当作 Base URL；这里会保留已有版本段，避免把
/// `.../api/coding/paas/v4` 错拼成 `.../v4/v1/responses`。
fn build_responses_probe_url(base_url: &str, is_full_url: bool) -> Result<String, String> {
    crate::protocol_compatibility::endpoint::build_probe_url(
        base_url,
        crate::protocol_compatibility::TransportKind::OpenAiResponses,
        is_full_url,
    )
}

/// 构造 Chat Completions 探测 URL。
///
/// 与 Responses URL 生成规则保持同源；对已经包含 `/v4` 等版本段的供应商，
/// 直接拼 `/chat/completions`，避免智谱 Coding Plan 被错拼成 `/v4/v1/...`。
fn build_chat_probe_url(base_url: &str, is_full_url: bool) -> Result<String, String> {
    crate::protocol_compatibility::endpoint::build_probe_url(
        base_url,
        crate::protocol_compatibility::TransportKind::OpenAiChat,
        is_full_url,
    )
}

/// 发送真正的最小 Responses 请求。
///
/// HTTP 错误、网络错误和超时都以结构化 `ok=false` 返回，让前端状态机可以用同一套表格
/// 展示异常；只有参数校验或 URL 推导这类本地输入错误才由命令层返回 `Err`。
async fn probe_codex_responses(
    url: &str,
    api_key: &str,
    model: &str,
    user_agent: Option<HeaderValue>,
) -> Result<CodexResponsesProbeResult, String> {
    let client = crate::proxy::http_client::get();
    let body = serde_json::json!({
        "model": model,
        "input": "ping",
        "max_output_tokens": 1024,
        "stream": false
    });
    let mut request = client
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .timeout(Duration::from_secs(12));
    if let Some(ua) = user_agent {
        request = request.header(USER_AGENT, ua);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return Ok(CodexResponsesProbeResult {
                ok: false,
                status: None,
                url: url.to_string(),
                model: model.to_string(),
                detail: format!("Request failed: {error}"),
            });
        }
    };
    let status = response.status();
    if status.is_success() {
        return Ok(CodexResponsesProbeResult {
            ok: true,
            status: Some(status.as_u16()),
            url: url.to_string(),
            model: model.to_string(),
            detail: "Responses probe succeeded".to_string(),
        });
    }

    let body = truncate_probe_body(response.text().await.unwrap_or_default());
    Ok(CodexResponsesProbeResult {
        ok: false,
        status: Some(status.as_u16()),
        url: url.to_string(),
        model: model.to_string(),
        detail: format!("HTTP {status}: {body}"),
    })
}

/// 发送真正的最小 Chat Completions 请求。
///
/// `max_tokens` 使用 1024，避免部分兼容服务拒绝过小输出上限；提示词仍然极短，实际输出通常很少。
async fn probe_codex_chat(
    url: &str,
    api_key: &str,
    model: &str,
    user_agent: Option<HeaderValue>,
) -> Result<CodexResponsesProbeResult, String> {
    let client = crate::proxy::http_client::get();
    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": "ping" }],
        "max_tokens": 1024,
        "stream": false
    });
    let mut request = client
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .timeout(Duration::from_secs(12));
    if let Some(ua) = user_agent {
        request = request.header(USER_AGENT, ua);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            return Ok(CodexResponsesProbeResult {
                ok: false,
                status: None,
                url: url.to_string(),
                model: model.to_string(),
                detail: format!("Request failed: {error}"),
            });
        }
    };
    let status = response.status();
    if status.is_success() {
        return Ok(CodexResponsesProbeResult {
            ok: true,
            status: Some(status.as_u16()),
            url: url.to_string(),
            model: model.to_string(),
            detail: "Chat Completions probe succeeded".to_string(),
        });
    }

    let body = truncate_probe_body(response.text().await.unwrap_or_default());
    Ok(CodexResponsesProbeResult {
        ok: false,
        status: Some(status.as_u16()),
        url: url.to_string(),
        model: model.to_string(),
        detail: format!("HTTP {status}: {body}"),
    })
}

/// 截断探测错误体，避免 UI 和日志被上游长响应淹没。
fn truncate_probe_body(body: String) -> String {
    const MAX_CHARS: usize = 512;
    if body.chars().count() <= MAX_CHARS {
        body
    } else {
        let mut truncated: String = body.chars().take(MAX_CHARS).collect();
        truncated.push('…');
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_chat_probe_url, build_responses_probe_url, parse_opencode_models, OpenCodeModelRef,
    };

    #[test]
    fn responses_probe_url_appends_v1_responses_to_root_base_url() {
        assert_eq!(
            build_responses_probe_url("https://example.com", false).unwrap(),
            "https://example.com/v1/responses"
        );
    }

    #[test]
    fn responses_probe_url_reuses_existing_v1_segment() {
        assert_eq!(
            build_responses_probe_url("https://example.com/v1", false).unwrap(),
            "https://example.com/v1/responses"
        );
    }

    #[test]
    fn responses_probe_url_keeps_existing_responses_endpoint() {
        assert_eq!(
            build_responses_probe_url("https://example.com/v1/responses", false).unwrap(),
            "https://example.com/v1/responses"
        );
    }

    #[test]
    fn responses_probe_url_derives_from_full_url() {
        assert_eq!(
            build_responses_probe_url("https://example.com/v1/chat/completions", true).unwrap(),
            "https://example.com/v1/responses"
        );
    }

    #[test]
    fn responses_probe_url_keeps_v1_when_full_url_is_version_root() {
        assert_eq!(
            build_responses_probe_url("https://example.com/v1", true).unwrap(),
            "https://example.com/v1/responses"
        );
    }

    #[test]
    fn chat_probe_url_derives_from_responses_full_url() {
        assert_eq!(
            build_chat_probe_url("https://example.com/v1/responses", true).unwrap(),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_probe_url_keeps_v1_when_full_url_is_version_root() {
        assert_eq!(
            build_chat_probe_url("https://example.com/v1", true).unwrap(),
            "https://example.com/v1/chat/completions"
        );
    }

    #[test]
    fn protocol_probe_url_preserves_query_parameters_when_switching_transport() {
        assert_eq!(
            build_chat_probe_url(
                "https://azure.example/openai/v1/responses?api-version=2026-08-01&key=secret",
                true
            )
            .unwrap(),
            "https://azure.example/openai/v1/chat/completions?api-version=2026-08-01&key=secret"
        );
    }

    #[test]
    fn responses_probe_url_respects_versioned_base_url() {
        assert_eq!(
            build_responses_probe_url("https://open.bigmodel.cn/api/coding/paas/v4", false)
                .unwrap(),
            "https://open.bigmodel.cn/api/coding/paas/v4/responses"
        );
    }

    #[test]
    fn chat_probe_url_respects_versioned_base_url() {
        assert_eq!(
            build_chat_probe_url("https://open.bigmodel.cn/api/coding/paas/v4", false).unwrap(),
            "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions"
        );
    }

    #[test]
    fn probe_urls_respect_volcengine_v3_base_urls() {
        assert_eq!(
            build_chat_probe_url("https://ark.cn-beijing.volces.com/api/coding/v3", false).unwrap(),
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions"
        );
        assert_eq!(
            build_responses_probe_url("https://ark.cn-beijing.volces.com/api/v3", false).unwrap(),
            "https://ark.cn-beijing.volces.com/api/v3/responses"
        );
    }

    #[test]
    fn probe_urls_preserve_non_v1_full_endpoints() {
        assert_eq!(
            build_responses_probe_url(
                "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions",
                true
            )
            .unwrap(),
            "https://open.bigmodel.cn/api/coding/paas/v4/responses"
        );
        assert_eq!(
            build_chat_probe_url(
                "https://open.bigmodel.cn/api/coding/paas/v4/responses",
                true
            )
            .unwrap(),
            "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions"
        );
    }
    #[test]
    fn parses_sorts_and_deduplicates_models() {
        assert_eq!(
            parse_opencode_models(
                "openrouter/vendor/model\nopencode/free-model\ninvalid\nopencode/free-model\n"
            ),
            vec![
                OpenCodeModelRef {
                    provider_id: "opencode".to_string(),
                    model_id: "free-model".to_string(),
                },
                OpenCodeModelRef {
                    provider_id: "openrouter".to_string(),
                    model_id: "vendor/model".to_string(),
                },
            ]
        );
    }

    #[test]
    fn skips_malformed_output_lines() {
        assert!(parse_opencode_models(
            "notice: loading models\n/model\nprovider/\nbad provider/model\nprovider/bad model\nprovider/bad\u{1b}[0m\n"
        )
        .is_empty());
    }
}
