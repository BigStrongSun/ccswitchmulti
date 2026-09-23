//! TE Provider 运行态只读命令。
//!
//! 边界：
//! - 只读 `GET /healthz` 与 `GET /provider-health`，不发任何授权请求，也不写配置。
//! - 只接受数值回环 HTTP 端点；请求显式禁用环境代理，避免 loopback 被全局代理劫持。
//! - 运行时的 task/lease/session/binding 由注入器进程内存持有，注入器**刻意不通过 HTTP 暴露**，
//!   因此这里返回 `runtimeBindingExposed: false`，让 UI 明确「读不到不是故障」。
//! - 错误信息只保留稳定分类，不回显响应正文或凭据。

use crate::error::AppError;
use crate::provider::Provider;
use crate::services::ProviderService;
use crate::store::AppState;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use tauri::State;
use url::Url;

const HEALTHZ_PATH: &str = "/healthz";
const PROVIDER_HEALTH_PATH: &str = "/provider-health";
const PROBE_TIMEOUT_SECONDS: u64 = 5;

/// SDK 静态描述符是公开定位记录，不是 Task 凭据；未知字段和所有秘密必须拒绝。
pub fn validate_static_te_descriptor(raw: &Value) -> Result<Value, AppError> {
    const ROOT: &[&str] = &[
        "providerType",
        "providerId",
        "pluginId",
        "protocolVersion",
        "sidecarUrl",
        "healthPath",
        "expectedPartnerAic",
        "models",
    ];
    const MODEL: &[&str] = &[
        "id",
        "name",
        "inputModalities",
        "outputModalities",
        "contextWindowTokens",
        "maxOutputTokens",
        "supportsTools",
        "supportsReasoning",
        "reasoningEfforts",
        "cost",
    ];
    let invalid = || AppError::InvalidInput("te_descriptor_invalid".to_string());
    let map = raw.as_object().ok_or_else(invalid)?;
    if map.len() != ROOT.len()
        || map.keys().any(|key| !ROOT.contains(&key.as_str()))
        || raw["providerType"] != "token-exchange"
        || raw["pluginId"] != "token-exchange"
        || raw["protocolVersion"] != "te-provider.v1"
        || raw["healthPath"] != "/healthz"
    {
        return Err(invalid());
    }
    for field in ["providerId", "pluginId"] {
        let value = raw[field].as_str().ok_or_else(invalid)?;
        if value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || !value.as_bytes()[0].is_ascii_lowercase() && !value.as_bytes()[0].is_ascii_digit()
        {
            return Err(invalid());
        }
    }
    let sidecar = raw["sidecarUrl"].as_str().ok_or_else(invalid)?;
    let remainder = sidecar
        .strip_prefix("http://127.0.0.1")
        .or_else(|| sidecar.strip_prefix("http://[::1]"))
        .ok_or_else(invalid)?;
    if !remainder.is_empty() {
        let port = remainder.strip_prefix(':').ok_or_else(invalid)?;
        if port.is_empty()
            || port.starts_with('0')
            || port.len() > 5
            || !port.bytes().all(|byte| byte.is_ascii_digit())
            || port.parse::<u16>().is_err()
        {
            return Err(invalid());
        }
    }
    if normalize_loopback_sidecar_url(sidecar)? != sidecar
        || !url::Url::parse(sidecar)
            .map_err(|_| invalid())?
            .path()
            .is_empty()
            && url::Url::parse(sidecar).map_err(|_| invalid())?.path() != "/"
        || sidecar.ends_with('/')
    {
        return Err(invalid());
    }
    let aic = raw["expectedPartnerAic"].as_str().ok_or_else(invalid)?;
    if aic.trim().is_empty() || aic.len() > 256 {
        return Err(invalid());
    }
    let models = raw["models"].as_array().ok_or_else(invalid)?;
    if models.is_empty() {
        return Err(invalid());
    }
    let mut seen = std::collections::HashSet::new();
    for model in models {
        let entry = model.as_object().ok_or_else(invalid)?;
        if entry.keys().any(|key| !MODEL.contains(&key.as_str())) {
            return Err(invalid());
        }
        for field in ["id", "name"] {
            if entry
                .get(field)
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
            {
                return Err(invalid());
            }
        }
        if !seen.insert(entry["id"].as_str().unwrap()) {
            return Err(invalid());
        }
        for (field, allowed) in [
            (
                "inputModalities",
                &["text", "image", "audio", "video", "file"][..],
            ),
            (
                "outputModalities",
                &["text", "embedding", "audio", "image"][..],
            ),
            (
                "reasoningEfforts",
                &[
                    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
                ][..],
            ),
        ] {
            if let Some(value) = entry.get(field) {
                let entries = value
                    .as_array()
                    .filter(|items| !items.is_empty())
                    .ok_or_else(invalid)?;
                if entries
                    .iter()
                    .any(|item| item.as_str().is_none_or(|item| !allowed.contains(&item)))
                {
                    return Err(invalid());
                }
                let unique = entries
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<std::collections::HashSet<_>>();
                if unique.len() != entries.len() {
                    return Err(invalid());
                }
            }
        }
        for field in ["contextWindowTokens", "maxOutputTokens"] {
            if entry
                .get(field)
                .is_some_and(|value| value.as_u64().is_none_or(|number| number == 0))
            {
                return Err(invalid());
            }
        }
        for field in ["supportsTools", "supportsReasoning"] {
            if entry.get(field).is_some_and(|value| !value.is_boolean()) {
                return Err(invalid());
            }
        }
        if let Some(cost) = entry.get("cost") {
            let cost = cost.as_object().ok_or_else(invalid)?;
            if cost
                .keys()
                .any(|key| !["input", "output", "cacheRead", "cacheWrite"].contains(&key.as_str()))
                || cost.values().any(|value| {
                    value
                        .as_f64()
                        .is_none_or(|number| !number.is_finite() || number < 0.0)
                })
            {
                return Err(invalid());
            }
        }
    }
    Ok(raw.clone())
}

#[tauri::command]
pub fn get_te_provider_descriptor(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Option<Value>, AppError> {
    state.db.get_te_provider_descriptor(&provider_id)
}

/// UI 专用写入口：SQLite 原子持久化；live 发布结果独立返回。
#[tauri::command]
pub fn save_te_provider(
    state: State<'_, AppState>,
    provider: Provider,
    descriptor: Value,
    #[allow(non_snake_case)] originalId: Option<String>,
    #[allow(non_snake_case)] addToLive: Option<bool>,
) -> Result<&'static str, AppError> {
    ProviderService::save_token_exchange_openclaw(
        state.inner(),
        provider,
        descriptor,
        originalId.as_deref(),
        addToLive.unwrap_or(true),
    )
}

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
        return Err(AppError::Message(
            "TE Provider endpoint is empty".to_string(),
        ));
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
        // 探针只验证本机注入器；禁止 3xx 把请求带到外部或私网地址。
        .redirect(Policy::none())
        .timeout(Duration::from_secs(PROBE_TIMEOUT_SECONDS))
        .connect_timeout(Duration::from_secs(PROBE_TIMEOUT_SECONDS))
        .build()
        .map_err(|_| AppError::Message("failed to build TE Provider probe client".to_string()))
}

const HEALTH_STATUSES: &[&str] = &[
    "ok", "healthy", "ready", "running", "degraded", "offline", "unknown",
];
const HEALTH_REASONS: &[&str] = &[
    "provider_probe_not_configured",
    "provider_probe_unreachable",
    "provider_probe_http_error",
    "provider_probe_invalid_response",
    "provider_offline",
    "provider_online",
    "unknown",
];

/// 健康响应来自本地 sidecar，仍按不可信输入处理，避免秘密/超长值进入 UI 和日志。
fn sanitize_health_status(value: Option<&str>) -> String {
    let value = value.unwrap_or_default();
    if value.len() <= 32
        && !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && HEALTH_STATUSES.contains(&value)
    {
        value.to_string()
    } else {
        "unknown".to_string()
    }
}

fn sanitize_health_reason(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.len() > 64
        || value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
        || !HEALTH_REASONS.contains(&value)
    {
        return Some("provider_reason_unavailable".to_string());
    }
    Some(value.to_string())
}

fn sanitize_checked_at(value: Option<&str>) -> Option<String> {
    let value = value?;
    if value.len() > 64 {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|_| value.to_string())
}

fn parse_provider_snapshot(body: &serde_json::Value) -> Option<TeProviderHealthSnapshot> {
    let provider = body.get("provider")?;
    if !provider.is_object() {
        return None;
    }
    Some(TeProviderHealthSnapshot {
        online: provider.get("online").and_then(|value| value.as_bool()),
        reason: sanitize_health_reason(provider.get("reason").and_then(|value| value.as_str())),
        checked_at: sanitize_checked_at(provider.get("checkedAt").and_then(|value| value.as_str())),
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
            let http_status = response.status().as_u16();
            status.sidecar_reachable = true;
            if !response.status().is_success() {
                status.sidecar_error = Some(format!("health_degraded_{http_status}"));
            }
            match response.json::<serde_json::Value>().await {
                Ok(body) => {
                    status.sidecar_status = Some(sanitize_health_status(
                        body.get("status").and_then(|value| value.as_str()),
                    ));
                }
                Err(_) => {
                    status.sidecar_status = Some("unknown".to_string());
                    if status.sidecar_error.is_none() {
                        status.sidecar_error = Some("invalid_response".to_string());
                    }
                }
            }
        }
        Err(_) => status.sidecar_error = Some("connect_failed".to_string()),
    }

    if status.sidecar_reachable {
        let provider_url = format!("{base_url}{PROVIDER_HEALTH_PATH}");
        match client.get(&provider_url).send().await {
            Ok(response) => {
                let http_status = response.status().as_u16();
                if response.status().is_success() {
                    status.provider = response
                        .json::<serde_json::Value>()
                        .await
                        .ok()
                        .and_then(|body| parse_provider_snapshot(&body))
                        .or_else(|| {
                            Some(TeProviderHealthSnapshot {
                                online: None,
                                reason: Some("provider_probe_invalid_response".to_string()),
                                checked_at: None,
                                http_status: Some(http_status),
                            })
                        });
                } else {
                    status.provider = Some(TeProviderHealthSnapshot {
                        online: Some(false),
                        reason: Some("provider_probe_http_error".to_string()),
                        checked_at: None,
                        http_status: Some(http_status),
                    });
                }
            }
            // 上游探针不可用不影响「注入器存活」这一结论，明确表示未知而非在线。
            Err(_) => {
                status.provider = Some(TeProviderHealthSnapshot {
                    online: None,
                    reason: Some("provider_probe_unreachable".to_string()),
                    checked_at: None,
                    http_status: None,
                });
            }
        }
    }

    status.latency_ms = started.elapsed().as_millis() as u64;
    Ok(status)
}

#[cfg(test)]
mod tests {
    #[test]
    fn static_descriptor_rejects_runtime_and_unknown_fields() {
        use serde_json::json;
        let good = json!({
            "providerType": "token-exchange", "providerId": "token-exchange",
            "pluginId": "token-exchange", "protocolVersion": "te-provider.v1",
            "sidecarUrl": "http://127.0.0.1:9814", "healthPath": "/healthz",
            "expectedPartnerAic": "partner-aic",
            "models": [{"id":"qwen3.8", "name":"Qwen 3.8"}]
        });
        assert!(super::validate_static_te_descriptor(&good).is_ok());
        for key in [
            "taskId",
            "leaseId",
            "bindingId",
            "proxyKey",
            "secret",
            "unknown",
        ] {
            let mut invalid = good.clone();
            invalid[key] = json!("forbidden");
            assert!(
                super::validate_static_te_descriptor(&invalid).is_err(),
                "{key}"
            );
        }
        for sidecar in [
            "http://localhost:9814",
            "http://127.1:9814",
            "http://127.0.0.1:09814",
            "http://127.0.0.1:9814/path",
        ] {
            let mut invalid = good.clone();
            invalid["sidecarUrl"] = json!(sidecar);
            assert!(
                super::validate_static_te_descriptor(&invalid).is_err(),
                "{sidecar}"
            );
        }
        let mut invalid = good.clone();
        invalid["models"][0]["reasoningEfforts"] = json!(["high", "high"]);
        assert!(super::validate_static_te_descriptor(&invalid).is_err());
        let mut capable = good;
        capable["models"][0]["inputModalities"] = json!(["text", "image"]);
        capable["models"][0]["contextWindowTokens"] = json!(262144);
        capable["models"][0]["supportsReasoning"] = json!(true);
        capable["models"][0]["cost"] = json!({"cacheRead": 0.01});
        assert_eq!(
            super::validate_static_te_descriptor(&capable).unwrap(),
            capable
        );
        capable["models"][0]["cost"]["output"] = json!(-1);
        assert!(super::validate_static_te_descriptor(&capable).is_err());
    }
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

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

    #[test]
    fn provider_snapshot_sanitizes_untrusted_status_reason_and_timestamp() {
        let snapshot = parse_provider_snapshot(&serde_json::json!({
            "provider": {
                "online": false,
                "reason": "secret-token-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "checkedAt": "not-a-timestamp"
            }
        }))
        .expect("snapshot");
        assert_eq!(
            snapshot.reason.as_deref(),
            Some("provider_reason_unavailable")
        );
        assert_eq!(snapshot.checked_at, None);
        assert_eq!(sanitize_health_status(Some("arbitrary-secret")), "unknown");
    }

    #[tokio::test]
    async fn probe_does_not_follow_redirects_and_treats_http_response_as_reachable() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let address = listener.local_addr().expect("local address");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 302 Found\r\nLocation: http://192.0.2.1:9/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .expect("write redirect");
        });

        let result = te_provider_runtime_status(format!("http://{address}"))
            .await
            .expect("probe result");
        assert!(result.sidecar_reachable);
        assert_eq!(result.sidecar_error.as_deref(), Some("health_degraded_302"));
        assert_eq!(result.sidecar_status.as_deref(), Some("unknown"));
    }

    #[tokio::test]
    async fn probe_distinguishes_health_degraded_from_transport_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let address = listener.local_addr().expect("local address");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            stream
                .write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nContent-Length: 21\r\nConnection: close\r\n\r\n{\"status\":\"degraded\"}",
                )
                .expect("write degraded health");
        });

        let result = te_provider_runtime_status(format!("http://{address}"))
            .await
            .expect("probe result");
        assert!(result.sidecar_reachable);
        assert_eq!(result.sidecar_status.as_deref(), Some("degraded"));
        assert_eq!(result.sidecar_error.as_deref(), Some("health_degraded_503"));
    }
}
