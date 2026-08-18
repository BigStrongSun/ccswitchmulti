//! Provider 只读元数据发现适配器（首版：OpenRouter、vLLM）。
//!
//! 原则：
//! - 只读元数据，禁止通过 low/high/none 真实推理请求主动试错；
//! - 只提取 allowlist 字段，不保存或展示原始服务器路径、凭据和其他敏感配置；
//! - `NotAdvertised`/`Unavailable`/`Invalid` 均不能自动生成 `confirmed_unsupported`。

use crate::provider::Provider;
use crate::reasoning_capabilities::{
    DiscoveryOutcome, ProviderCapabilitySnapshot, ReasoningCapabilitySnapshot,
};
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration;

/// 发现请求超时（与 model_fetch 保持一致）。
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);

/// 错误响应体截断长度：避免把几十 KB 的 HTML 404 页整页保留到日志。
const ERROR_BODY_MAX_CHARS: usize = 512;

/// 按平台标识（name / base_url）判定 provider 平台。
///
/// 仅以平台标识判定，绝不掺入 model 名——model 名属于模型厂商，会把托管
/// 平台误判成模型官方接口。
pub fn detect_platform(provider: &Provider) -> Option<&'static str> {
    let base_url = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .or_else(|| provider.settings_config.get("baseUrl"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let platform = format!("{} {}", provider.name, base_url).to_ascii_lowercase();
    if platform.contains("openrouter") {
        Some("openrouter")
    } else if platform.contains("vllm") {
        Some("vllm")
    } else {
        None
    }
}

/// 统一发现入口：按平台选择适配器。未知平台返回 `Unavailable`。
pub async fn discover_provider_capability(provider: &Provider, model: &str) -> DiscoveryOutcome {
    match detect_platform(provider) {
        Some("openrouter") => discover_openrouter(provider, model).await,
        Some("vllm") => discover_vllm(provider, model).await,
        _ => DiscoveryOutcome::Unavailable,
    }
}

fn provider_key(provider: &Provider) -> String {
    provider.id.clone()
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn base_url(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .or_else(|| provider.settings_config.get("baseUrl"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToString::to_string)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(|error| format!("cannot build discovery client: {error}"))
}

fn truncate_body(body: &str) -> String {
    body.chars().take(ERROR_BODY_MAX_CHARS).collect()
}

/// OpenRouter 适配器：`GET {base}/api/v1/models`（公共端点，无需鉴权）。
///
/// 提取目标模型的 `reasoning` 对象（allowlist 字段）：
/// `supported_efforts` / `default_effort` / `mandatory` / `default_enabled` /
/// `supports_max_tokens`。
pub async fn discover_openrouter(provider: &Provider, model: &str) -> DiscoveryOutcome {
    let Some(base) = base_url(provider) else {
        return DiscoveryOutcome::Unavailable;
    };
    let models_url = openrouter_models_url(&base);
    let Ok(http) = client() else {
        return DiscoveryOutcome::Unavailable;
    };

    let response = match http.get(&models_url).send().await {
        Ok(response) => response,
        Err(error) => {
            log::debug!("openrouter discovery unreachable at {models_url}: {error}");
            return DiscoveryOutcome::Unavailable;
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        log::debug!(
            "openrouter discovery failed at {models_url}: {status} {}",
            truncate_body(&body)
        );
        return DiscoveryOutcome::Unavailable;
    }

    let Ok(payload) = response.json::<Value>().await else {
        return DiscoveryOutcome::Invalid;
    };
    let Some(models) = payload.get("data").and_then(Value::as_array) else {
        return DiscoveryOutcome::Invalid;
    };

    let normalized = model.trim().to_ascii_lowercase();
    let entry = models.iter().find(|candidate| {
        ["id", "canonical_slug", "slug"]
            .into_iter()
            .filter_map(|field| candidate.get(field).and_then(Value::as_str))
            .any(|value| value.trim().eq_ignore_ascii_case(&normalized))
    });

    let Some(entry) = entry else {
        // 端点可达但模型未列出：NotAdvertised（不是 Unavailable）。
        return DiscoveryOutcome::NotAdvertised;
    };

    let Some(reasoning) = entry.get("reasoning") else {
        return DiscoveryOutcome::NotAdvertised;
    };
    if reasoning.is_null() {
        return DiscoveryOutcome::NotAdvertised;
    }

    let snapshot = ProviderCapabilitySnapshot {
        provider_key: provider_key(provider),
        model: model.to_string(),
        fetched_at: now_millis(),
        source: "openrouter_api".to_string(),
        reasoning: Some(ReasoningCapabilitySnapshot {
            supported_efforts: reasoning
                .get("supported_efforts")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            default_effort: reasoning
                .get("default_effort")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            mandatory: reasoning
                .get("mandatory")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            default_enabled: reasoning.get("default_enabled").and_then(Value::as_bool),
            supports_max_tokens: reasoning
                .get("supports_max_tokens")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            upstream_format: Some("object".to_string()),
            upstream_parameter: Some("reasoning.effort".to_string()),
            output_format: Some("auto".to_string()),
        }),
    };
    DiscoveryOutcome::Found(snapshot)
}

/// 归一化 OpenRouter models 端点：base 已含 `/api/v1` 时直接拼 `/models`。
fn openrouter_models_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.to_ascii_lowercase().ends_with("/api/v1") {
        format!("{trimmed}/models")
    } else if trimmed.to_ascii_lowercase().ends_with("/models") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/api/v1/models")
    }
}

/// vLLM 适配器：组合 `/version`、`/v1/models`、`/server_info?config_format=json`。
///
/// vLLM 不在服务端声明逐模型 effort 档位（那是模型属性，不是服务属性），
/// 因此本适配器只产出服务级快照（版本 + 模型存在性 + 推理解析器配置），
/// `reasoning` 子对象保持 None——逐模型能力由版本化能力库提供。
///
/// `/server_info` 是开发端点（`VLLM_SERVER_DEV_MODE` 开启才暴露），生产部署
/// 通常 404——按 `Unavailable` 降级，不是错误。
pub async fn discover_vllm(provider: &Provider, model: &str) -> DiscoveryOutcome {
    let Some(base) = base_url(provider) else {
        return DiscoveryOutcome::Unavailable;
    };
    let Ok(http) = client() else {
        return DiscoveryOutcome::Unavailable;
    };

    // 1. /version：服务版本（revision 匹配用）。
    let version_url = format!("{}/version", base.trim_end_matches('/'));
    // 首版仅用于确认服务可达 + 为后续 revision 匹配预留；vLLM 库条目（P6）
    // 引入后用于 `revision_range` 校验。
    let _version = match http.get(&version_url).send().await {
        Ok(response) if response.status().is_success() => {
            response.json::<Value>().await.ok().and_then(|payload| {
                payload
                    .get("version")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
        }
        Ok(response) => {
            log::debug!("vllm /version failed: {}", response.status());
            None
        }
        Err(error) => {
            log::debug!("vllm /version unreachable at {version_url}: {error}");
            return DiscoveryOutcome::Unavailable;
        }
    };

    // 2. /v1/models：确认目标模型存在于服务实例。
    let models_url = format!("{}/v1/models", base.trim_end_matches('/'));
    let model_listed = match http.get(&models_url).send().await {
        Ok(response) if response.status().is_success() => response
            .json::<Value>()
            .await
            .ok()
            .and_then(|payload| {
                payload.get("data").and_then(Value::as_array).map(|items| {
                    items.iter().any(|item| {
                        item.get("id")
                            .and_then(Value::as_str)
                            .is_some_and(|id| id.trim().eq_ignore_ascii_case(model.trim()))
                    })
                })
            })
            .unwrap_or(false),
        Ok(response) => {
            log::debug!("vllm /v1/models failed: {}", response.status());
            return DiscoveryOutcome::Unavailable;
        }
        Err(error) => {
            log::debug!("vllm /v1/models unreachable at {models_url}: {error}");
            return DiscoveryOutcome::Unavailable;
        }
    };
    if !model_listed {
        return DiscoveryOutcome::NotAdvertised;
    }

    // 3. /server_info?config_format=json：开发端点，只提取 allowlist 字段。
    //    404/403 是常态（生产部署未开 VLLM_SERVER_DEV_MODE），按 Unavailable 降级。
    let info_url = format!(
        "{}/server_info?config_format=json",
        base.trim_end_matches('/')
    );
    let server_info = match http.get(&info_url).send().await {
        Ok(response) if response.status().is_success() => response.json::<Value>().await.ok(),
        Ok(response) if response.status() == StatusCode::NOT_FOUND => {
            log::debug!("vllm /server_info not exposed (dev mode off); degrading");
            None
        }
        Ok(response) => {
            log::debug!("vllm /server_info failed: {}", response.status());
            None
        }
        Err(_) => None,
    };

    // allowlist 提取：只保留推理解析器相关字段，绝不落盘/展示完整 VllmConfig。
    let reasoning_parser = server_info
        .as_ref()
        .and_then(|payload| payload.get("vllm_config"))
        .and_then(|config| config.get("reasoning_config"))
        .and_then(|reasoning| reasoning.get("reasoning_parser"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let _ = reasoning_parser; // 首版仅记录到日志，不进入快照（无消费方）。

    DiscoveryOutcome::Found(ProviderCapabilitySnapshot {
        provider_key: provider_key(provider),
        model: model.to_string(),
        fetched_at: now_millis(),
        source: "vllm_server".to_string(),
        // vLLM 不声明逐模型 effort：reasoning 保持 None，resolver 落到能力库。
        reasoning: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(name: &str, base_url: &str) -> Provider {
        Provider {
            id: "test-provider".into(),
            name: name.into(),
            settings_config: json!({ "base_url": base_url }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn detect_platform_openrouter_by_name_or_url() {
        assert_eq!(
            detect_platform(&provider("OpenRouter", "https://api.deepseek.com")),
            Some("openrouter")
        );
        assert_eq!(
            detect_platform(&provider("My Gateway", "https://openrouter.ai/api/v1")),
            Some("openrouter")
        );
        // 名称与 URL 均不含平台标识 → 未知平台。
        assert_eq!(
            detect_platform(&provider("Local Server", "http://127.0.0.1:8000")),
            None
        );
        assert_eq!(
            detect_platform(&provider("Qwen vLLM", "http://vllm-roglinux:8000")),
            Some("vllm")
        );
    }

    #[test]
    fn openrouter_models_url_normalization() {
        assert_eq!(
            openrouter_models_url("https://openrouter.ai/api/v1"),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            openrouter_models_url("https://openrouter.ai"),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            openrouter_models_url("https://openrouter.ai/api/v1/"),
            "https://openrouter.ai/api/v1/models"
        );
    }
}
