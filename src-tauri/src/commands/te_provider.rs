//! TE Provider 运行态只读命令。
//!
//! 边界：
//! - 只读 `GET /healthz` 与 `GET /provider-health`，不发任何授权请求，也不写配置。
//! - 只接受数值回环 HTTP 端点；请求显式禁用环境代理，避免 loopback 被全局代理劫持。
//! - 运行时的 task/lease/session/binding 由注入器进程内存持有，注入器**刻意不通过 HTTP 暴露**，
//!   因此这里返回 `runtimeBindingExposed: false`，让 UI 明确「读不到不是故障」。
//! - 错误信息只保留稳定分类，不回显响应正文或凭据。

use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use url::Url;

const HEALTHZ_PATH: &str = "/healthz";
const PROVIDER_HEALTH_PATH: &str = "/provider-health";
const PROBE_TIMEOUT_SECONDS: u64 = 5;

/// 上游模型提供商的只读存活快照。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TeProviderHealthSnapshot {
    /// None 表示「未配置探针 / 未知」，不代表离线。
    pub online: Option<bool>,
    pub reason: Option<String>,
    pub checked_at: Option<String>,
    pub http_status: Option<u16>,
}

/// CCSM 侧可见的 TE Provider 运行态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TeProviderRuntimeStatus {
    pub sidecar_url: String,
    pub sidecar_reachable: bool,
    pub sidecar_status: Option<String>,
    /// 稳定错误分类，例如 `connect_failed`、`unexpected_status`、`invalid_response`。
    pub sidecar_error: Option<String>,
    pub provider: Option<TeProviderHealthSnapshot>,
    pub latency_ms: u64,
    pub checked_at: String,
    pub runtime_binding_exposed: bool,
}

/// 校验并规范化注入器端点：只允许数值回环 HTTP。
pub fn normalize_loopback_sidecar_url(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Message("TE Provider endpoint is empty".to_string()));
    }
    let parsed = Url::parse(trimmed)
        .map_err(|_| AppError::Message("TE Provider endpoint is not a valid URL".to_string()))?;
    if parsed.scheme() != "http" {
        return Err(AppError::Message(
            "TE Provider endpoint must use http on loopback".to_string(),
        ));
    }
    // 只信数值回环：localhost 依赖名称解析，可能被 hosts/DNS 重写到非本机端点。
    // `url::Url` 对 IPv6 会保留方括号（`[::1]`），比较前先去掉。
    let host = parsed.host_str().unwrap_or("").trim_matches(['[', ']']);
    let host_is_loopback = matches!(host, "127.0.0.1" | "::1");
    if !host_is_loopback {
        return Err(AppError::Message(
            "TE Provider endpoint must be a numeric loopback host".to_string(),
        ));
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(AppError::Message(
            "TE Provider endpoint must not contain credentials, query or fragment".to_string(),
        ));
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

fn http_client() -> Result<reqwest::Client, AppError> {
    // `no_proxy()`：loopback 探针绝不能被用户的全局出站代理改写目标。
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(PROBE_TIMEOUT_SECONDS))
        .connect_timeout(Duration::from_secs(PROBE_TIMEOUT_SECONDS))
        .build()
        .map_err(|_| AppError::Message("failed to build TE Provider probe client".to_string()))
}

fn parse_provider_snapshot(body: &serde_json::Value) -> Option<TeProviderHealthSnapshot> {
    let provider = body.get("provider")?;
    if !provider.is_object() {
        return None;
    }
    Some(TeProviderHealthSnapshot {
        online: provider.get("online").and_then(|value| value.as_bool()),
        reason: provider
            .get("reason")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        checked_at: provider
            .get("checkedAt")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string()),
        http_status: provider
            .get("httpStatus")
            .and_then(|value| value.as_u64())
            .and_then(|value| u16::try_from(value).ok()),
    })
}

/// 读取 TE Provider 运行态：注入器是否存活，以及上游模型提供商是否在线。
#[tauri::command]
pub async fn te_provider_runtime_status(
    sidecar_url: String,
) -> Result<TeProviderRuntimeStatus, AppError> {
    let base_url = normalize_loopback_sidecar_url(&sidecar_url)?;
    let client = http_client()?;
    let started = Instant::now();
    let checked_at = chrono::Utc::now().to_rfc3339();

    let mut status = TeProviderRuntimeStatus {
        sidecar_url: base_url.clone(),
        sidecar_reachable: false,
        sidecar_status: None,
        sidecar_error: None,
        provider: None,
        latency_ms: 0,
        checked_at,
        runtime_binding_exposed: false,
    };

    let healthz_url = format!("{base_url}{HEALTHZ_PATH}");
    match client.get(&healthz_url).send().await {
        Ok(response) => {
            if !response.status().is_success() {
                status.sidecar_error = Some(format!("unexpected_status_{}", response.status().as_u16()));
            } else {
                match response.json::<serde_json::Value>().await {
                    Ok(body) => {
                        status.sidecar_reachable = true;
                        status.sidecar_status = body
                            .get("status")
                            .and_then(|value| value.as_str())
                            .map(|value| value.to_string());
                    }
                    Err(_) => status.sidecar_error = Some("invalid_response".to_string()),
                }
            }
        }
        Err(_) => status.sidecar_error = Some("connect_failed".to_string()),
    }

    if status.sidecar_reachable {
        let provider_url = format!("{base_url}{PROVIDER_HEALTH_PATH}");
        match client.get(&provider_url).send().await {
            Ok(response) if response.status().is_success() => {
                if let Ok(body) = response.json::<serde_json::Value>().await {
                    status.provider = parse_provider_snapshot(&body);
                }
            }
            // 上游探针不可用不影响「注入器存活」这一结论，也不伪装成在线。
            Ok(_) | Err(_) => status.provider = None,
        }
    }

    status.latency_ms = started.elapsed().as_millis() as u64;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_numeric_loopback_http_endpoints() {
        assert_eq!(
            normalize_loopback_sidecar_url("http://127.0.0.1:9814/").unwrap(),
            "http://127.0.0.1:9814"
        );
        assert_eq!(
            normalize_loopback_sidecar_url("http://[::1]:9814").unwrap(),
            "http://[::1]:9814"
        );
        for rejected in [
            "https://127.0.0.1:9814",
            "http://localhost:9814",
            "http://0.0.0.0:9814",
            "http://10.0.0.5:9814",
            "http://user:pass@127.0.0.1:9814",
            "http://127.0.0.1:9814?x=1",
            "",
        ] {
            assert!(
                normalize_loopback_sidecar_url(rejected).is_err(),
                "must reject {rejected}"
            );
        }
    }

    #[test]
    fn provider_snapshot_keeps_unknown_distinct_from_offline() {
        let snapshot = parse_provider_snapshot(&serde_json::json!({
            "provider": {
                "online": null,
                "reason": "provider_probe_not_configured",
                "checkedAt": null,
                "httpStatus": null,
            }
        }))
        .expect("snapshot");

        assert_eq!(snapshot.online, None);
        assert_eq!(
            snapshot.reason.as_deref(),
            Some("provider_probe_not_configured")
        );
        assert!(parse_provider_snapshot(&serde_json::json!({})).is_none());
    }
}
