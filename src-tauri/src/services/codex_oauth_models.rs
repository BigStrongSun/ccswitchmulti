//! Codex OAuth model list service.
//!
//! ChatGPT Codex exposes models through `chatgpt.com/backend-api/codex/models`,
//! which is not an OpenAI-compatible `/v1/models` endpoint.

use crate::proxy::providers::CODEX_OAUTH_ORIGINATOR;
use crate::services::model_fetch::FetchedModel;
use serde_json::Value;
use std::error::Error;
use std::time::Duration;

const CODEX_OAUTH_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
const CODEX_PUBLIC_MODELS_URL: &str =
    "https://raw.githubusercontent.com/openai/codex/main/codex-rs/models-manager/models.json";
const CODEX_OAUTH_FETCH_TIMEOUT_SECS: u64 = 15;
const CODEX_PUBLIC_MODELS_FETCH_TIMEOUT_SECS: u64 = 10;
const CODEX_PUBLIC_MODELS_MAX_BYTES: u64 = 8 * 1024 * 1024;
const ERROR_BODY_MAX_CHARS: usize = 512;
const CODEX_OAUTH_CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 使用 ChatGPT OAuth access token 在线读取官方 Codex 模型列表。
///
/// 这里的失败分两层：HTTP 状态码失败说明请求已经到达 ChatGPT 后端；`send`
/// 失败则是 DNS、TLS、代理、超时或本机网络层问题，调用方可以再尝试本地缓存兜底。
pub async fn fetch_models_with_token(
    token: &str,
    workspace_id: &str,
) -> Result<Vec<FetchedModel>, String> {
    let client = crate::proxy::http_client::get();
    let response = client
        .get(CODEX_OAUTH_MODELS_URL)
        .query(&[("client_version", CODEX_OAUTH_CLIENT_VERSION)])
        .header("Authorization", format!("Bearer {token}"))
        .header("originator", CODEX_OAUTH_ORIGINATOR)
        .header("chatgpt-account-id", workspace_id)
        .timeout(Duration::from_secs(CODEX_OAUTH_FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(format_model_catalog_request_error)?;

    let status = response.status();
    if !status.is_success() {
        let body = truncate_body(response.text().await.unwrap_or_default());
        return Err(format!("HTTP {status}: {body}"));
    }

    let value: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    Ok(parse_models(value))
}

/// 格式化 OAuth 模型列表请求的网络层错误。
///
/// `reqwest::Error` 的默认文本经常只显示 `error sending request for url`，
/// 不足以区分超时、连接、TLS 或代理问题。这里展开错误链和 CCSM 全局代理状态，
/// 但不包含任何 token、账号明文或请求头。
fn format_model_catalog_request_error(error: reqwest::Error) -> String {
    let mut hints = Vec::new();
    if error.is_timeout() {
        hints.push("timeout");
    }
    if error.is_connect() {
        hints.push("connect");
    }
    if error.is_builder() {
        hints.push("request_builder");
    }
    if error.is_decode() {
        hints.push("decode");
    }

    let proxy_hint = crate::proxy::http_client::get_current_proxy_url()
        .map(|_| "CCSwitchMulti 全局代理已配置".to_string())
        .unwrap_or_else(|| {
            "CCSwitchMulti 全局代理未配置；Windows/浏览器系统代理不一定会被后端 reqwest 使用"
                .to_string()
        });

    let mut source_parts = Vec::new();
    let mut source = error.source();
    while let Some(current) = source {
        source_parts.push(current.to_string());
        if source_parts.len() >= 4 {
            break;
        }
        source = current.source();
    }

    let kind = if hints.is_empty() {
        "unknown".to_string()
    } else {
        hints.join(",")
    };
    let source_chain = if source_parts.is_empty() {
        "无底层错误链".to_string()
    } else {
        source_parts.join(" -> ")
    };

    format!("Request failed: {error}; kind={kind}; {proxy_hint}; source={source_chain}")
}

/// 读取本地 Codex 官方模型来源链，作为公共目录也不可用时的最终兜底。
///
/// 与配置投影共用同一个可信来源链：CCSM packaged 基线、本机未被 CCSM 接管的
/// 官方 cache/backup、当前 Codex bundled 目录。CCSM-owned 混合 cache 在来源选择
/// 阶段已被排除，因此这里不能再按模型名称猜测官方身份。
pub fn fetch_cached_models_from_disk() -> Result<Vec<FetchedModel>, String> {
    let models = crate::codex_config::codex_official_models_cache().unwrap_or_default();
    Ok(parse_cached_models(serde_json::json!({ "models": models })))
}

/// 无需 CCSM OAuth 的官方目录入口：优先刷新 OpenAI/Codex 公共 catalog，失败时
/// 使用上面的本地可信来源链。公共内容在写入独立缓存前会移除指令字段。
pub async fn fetch_official_fallback_models() -> Result<Vec<FetchedModel>, String> {
    fetch_official_fallback_models_from_url(CODEX_PUBLIC_MODELS_URL).await
}

async fn fetch_official_fallback_models_from_url(
    public_models_url: &str,
) -> Result<Vec<FetchedModel>, String> {
    match fetch_public_official_catalog_from_url(public_models_url).await {
        Ok(models) => {
            let parsed = parse_cached_models(serde_json::json!({ "models": models }));
            match crate::codex_config::store_codex_public_official_models_cache(&models) {
                Ok(()) => {
                    let merged = fetch_cached_models_from_disk()?;
                    if !merged.is_empty() {
                        return Ok(merged);
                    }
                }
                Err(error) => {
                    log::warn!("failed to cache OpenAI public Codex model catalog: {error}");
                    if !parsed.is_empty() {
                        return Ok(parsed);
                    }
                }
            }
        }
        Err(error) => {
            log::warn!("failed to refresh OpenAI public Codex model catalog: {error}");
        }
    }

    fetch_cached_models_from_disk()
}

async fn fetch_public_official_catalog_from_url(url: &str) -> Result<Vec<Value>, String> {
    let response = crate::proxy::http_client::get()
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::USER_AGENT,
            concat!("CCSwitchMulti/", env!("CARGO_PKG_VERSION")),
        )
        .timeout(Duration::from_secs(CODEX_PUBLIC_MODELS_FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(format_model_catalog_request_error)?;

    let status = response.status();
    if !status.is_success() {
        let body = truncate_body(response.text().await.unwrap_or_default());
        return Err(format!("HTTP {status}: {body}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > CODEX_PUBLIC_MODELS_MAX_BYTES)
    {
        return Err("OpenAI public Codex model catalog exceeds 8 MiB".to_string());
    }

    let body = response
        .bytes()
        .await
        .map_err(|error| format!("Failed to read public model catalog: {error}"))?;
    if body.len() as u64 > CODEX_PUBLIC_MODELS_MAX_BYTES {
        return Err("OpenAI public Codex model catalog exceeds 8 MiB".to_string());
    }
    let value: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("Failed to parse public model catalog: {error}"))?;
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| "OpenAI public Codex model catalog has no models array".to_string())?;
    let sanitized = models
        .iter()
        .filter_map(sanitize_public_official_model)
        .collect::<Vec<_>>();
    if parse_cached_models(serde_json::json!({ "models": sanitized })).is_empty() {
        return Err("OpenAI public Codex model catalog has no usable models".to_string());
    }
    Ok(sanitized)
}

fn sanitize_public_official_model(model: &Value) -> Option<Value> {
    let mut model = model.as_object()?.clone();
    for field in [
        "model_messages",
        "modelMessages",
        "base_instructions",
        "baseInstructions",
        "instructions",
        "instructions_template",
        "instructionsTemplate",
    ] {
        model.remove(field);
    }
    Some(Value::Object(model))
}

/// 解析已经过来源隔离的官方模型目录。
fn parse_cached_models(value: Value) -> Vec<FetchedModel> {
    parse_models(value)
}

/// 解析 ChatGPT Codex 模型列表响应，兼容数组、`data`、`items` 和 map 形态。
fn parse_models(value: Value) -> Vec<FetchedModel> {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.as_array());

    let mut models = Vec::new();

    if let Some(entries) = entries {
        for entry in entries {
            push_model_entry(&mut models, entry, None);
        }
    }

    if let Some(model_map) = value.get("models").and_then(Value::as_object) {
        for (key, entry) in model_map {
            push_model_entry(&mut models, entry, Some(key));
        }
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

/// 将单个响应条目追加到模型列表。
///
/// 条目可能只是字符串模型名，也可能是包含 `slug/id/model/name` 的对象；
/// `fallback_id` 仅用于 map 形态，避免对象里没有显式 id 时丢失 key。
fn push_model_entry(models: &mut Vec<FetchedModel>, entry: &Value, fallback_id: Option<&str>) {
    if let Some(id) = entry.as_str().map(str::trim).filter(|id| !id.is_empty()) {
        models.push(FetchedModel {
            context_window: None,
            id: id.to_string(),
            input_modalities: None,
            owned_by: Some("Codex".to_string()),
            supports_image: None,
            reasoning: None,
        });
        return;
    }

    let Some(obj) = entry.as_object() else {
        if let Some(id) = fallback_id.map(str::trim).filter(|id| !id.is_empty()) {
            models.push(FetchedModel {
                context_window: None,
                id: id.to_string(),
                input_modalities: None,
                owned_by: Some("Codex".to_string()),
                supports_image: None,
                reasoning: None,
            });
        }
        return;
    };

    if model_entry_is_explicitly_unavailable(obj) {
        return;
    }

    let Some(id) = string_field(obj, &["slug", "id", "model", "name"]).or_else(|| {
        fallback_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
    }) else {
        return;
    };
    let owned_by = string_field(
        obj,
        &[
            "owned_by", "ownedBy", "provider", "vendor", "category", "owner",
        ],
    )
    .or_else(|| Some("Codex".to_string()));

    let context_window = extract_context_window(obj);
    let input_modalities = extract_input_modalities(obj);
    let supports_image = extract_supports_image(obj, input_modalities.as_deref());
    let reasoning =
        crate::proxy::providers::codex_reasoning::official_reasoning_capability_for_model(
            &id,
            std::slice::from_ref(entry),
        )
        .and_then(|capability| serde_json::to_value(capability).ok());

    models.push(FetchedModel {
        context_window,
        id,
        input_modalities,
        owned_by,
        supports_image,
        reasoning,
    });
}

/// 判断官方 Codex 模型条目是否显式标为不可调用。
///
/// ChatGPT 后端有时会返回“存在但当前账号/API 不可用”的模型元数据；这类模型
/// 不能写进 MultiRouter catalog，否则 Codex 选择器会展示它，但 `/responses`
/// 随后返回 `Model not found`。缺少可用性字段时保守保留，只过滤明确否定值。
fn model_entry_is_explicitly_unavailable(obj: &serde_json::Map<String, Value>) -> bool {
    let false_flags = [
        "supported_in_api",
        "supportedInApi",
        "available",
        "is_available",
        "isAvailable",
        "enabled",
    ];
    if false_flags
        .iter()
        .any(|key| obj.get(*key).and_then(Value::as_bool) == Some(false))
    {
        return true;
    }

    if obj.get("disabled").and_then(Value::as_bool) == Some(true) {
        return true;
    }

    let hidden_visibility = string_field(obj, &["visibility", "status", "availability"])
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|value| {
            matches!(
                value.as_str(),
                "hide" | "hidden" | "disabled" | "unavailable" | "unsupported" | "denied"
            )
        });
    hidden_visibility
}

fn string_field(obj: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| obj.get(*key))
        .filter_map(Value::as_str)
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

/// 从 Codex OAuth 模型条目中提取上下文窗口。
///
/// 官方接口字段可能随客户端版本变化，只有明确的正整数才会被接受。
fn extract_context_window(obj: &serde_json::Map<String, Value>) -> Option<u64> {
    const KEYS: &[&str] = &[
        "context_window",
        "max_context_window",
        "contextWindow",
        "maxContextWindow",
    ];

    KEYS.iter()
        .filter_map(|key| obj.get(*key))
        .find_map(parse_positive_u64)
}

/// 从官方 Codex 模型条目读取输入模态。
///
/// 这是 Codex Desktop 判断图片入口的关键能力字段；不能在 OAuth 动态目录同步时丢掉。
fn extract_input_modalities(obj: &serde_json::Map<String, Value>) -> Option<Vec<String>> {
    ["input_modalities", "inputModalities", "modalities"]
        .iter()
        .filter_map(|key| obj.get(*key))
        .find_map(parse_input_modalities)
}

fn parse_input_modalities(value: &Value) -> Option<Vec<String>> {
    let values = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        Value::Object(obj) => obj
            .get("input")
            .or_else(|| obj.get("inputs"))
            .and_then(parse_input_modalities)?,
        _ => return None,
    };

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn extract_supports_image(
    obj: &serde_json::Map<String, Value>,
    input_modalities: Option<&[String]>,
) -> Option<bool> {
    if let Some(value) = [
        "supports_image",
        "supportsImage",
        "vision",
        "supports_image_detail_original",
        "supportsImageDetailOriginal",
    ]
    .iter()
    .filter_map(|key| obj.get(*key))
    .find_map(Value::as_bool)
    {
        return Some(value);
    }

    input_modalities.map(|modalities| {
        modalities
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("image"))
    })
}

/// 将 JSON 数字或纯数字字符串转换为正整数。
///
/// 带单位的文本会保留为未知值，让前端继续使用用户填写或默认兜底。
fn parse_positive_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64().filter(|v| *v > 0),
        Value::String(text) => text.trim().parse::<u64>().ok().filter(|value| *value > 0),
        _ => None,
    }
}

fn truncate_body(body: String) -> String {
    if body.chars().count() <= ERROR_BODY_MAX_CHARS {
        body
    } else {
        let mut s: String = body.chars().take(ERROR_BODY_MAX_CHARS).collect();
        s.push_str("...");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Json, Router};
    use serde_json::json;

    async fn spawn_public_catalog_server(body: Value) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind public catalog fixture");
        let address = listener.local_addr().expect("fixture address");
        let app = Router::new().route(
            "/models.json",
            get(move || {
                let body = body.clone();
                async move { Json(body) }
            }),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve public catalog fixture");
        });
        (format!("http://{address}/models.json"), task)
    }

    #[test]
    fn parse_codex_oauth_models_accepts_openai_style_data() {
        let models = parse_models(json!({
            "data": [
                { "id": "gpt-5.4", "owned_by": "openai" },
                { "id": "gpt-5.4-mini", "ownedBy": "openai" }
            ]
        }));

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-5.4");
        assert_eq!(models[0].owned_by.as_deref(), Some("openai"));
        assert_eq!(models[1].id, "gpt-5.4-mini");
        assert_eq!(models[1].owned_by.as_deref(), Some("openai"));
    }

    #[test]
    fn parse_codex_oauth_models_accepts_model_list_shape() {
        let models = parse_models(json!({
            "models": [
                { "slug": "gpt-5.3-codex", "display_name": "GPT-5.3 Codex" },
                "gpt-5.5"
            ]
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.3-codex".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parse_codex_oauth_models_deduplicates_ids() {
        let models = parse_models(json!({
            "data": [
                { "id": "gpt-5.4" },
                { "model": "gpt-5.4" }
            ]
        }));

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "gpt-5.4");
    }

    #[test]
    fn parse_codex_oauth_models_accepts_model_map_shape() {
        let models = parse_models(json!({
            "models": {
                "gpt-5.4": { "display_name": "GPT-5.4" },
                "gpt-5.5": { "slug": "gpt-5.5" }
            }
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parse_codex_oauth_models_extracts_context_window() {
        let models = parse_models(json!({
            "models": [
                { "slug": "gpt-5.4", "context_window": 272000 },
                { "slug": "gpt-5.5", "maxContextWindow": "1000000" },
                { "slug": "bad", "contextWindow": "128000 tokens" }
            ]
        }));

        assert_eq!(models[0].context_window, None);
        assert_eq!(models[1].context_window, Some(272_000));
        assert_eq!(models[2].context_window, Some(1_000_000));
    }

    #[test]
    fn parse_codex_oauth_models_preserves_image_modalities() {
        let models = parse_models(json!({
            "models": [
                {
                    "slug": "gpt-5.6-sol",
                    "input_modalities": ["text", "image"],
                    "supports_image_detail_original": true
                },
                {
                    "slug": "gpt-5.3-codex-spark",
                    "input_modalities": ["text"],
                    "supports_image_detail_original": false
                }
            ]
        }));

        let sol = models
            .iter()
            .find(|model| model.id == "gpt-5.6-sol")
            .unwrap();
        assert_eq!(
            sol.input_modalities.as_deref(),
            Some(&["text".to_string(), "image".to_string()][..])
        );
        assert_eq!(sol.supports_image, Some(true));

        let spark = models
            .iter()
            .find(|model| model.id == "gpt-5.3-codex-spark")
            .unwrap();
        assert_eq!(
            spark.input_modalities.as_deref(),
            Some(&["text".to_string()][..])
        );
        assert_eq!(spark.supports_image, Some(false));
    }

    #[test]
    fn parse_codex_oauth_models_preserves_reasoning_capability() {
        let models = parse_models(json!({
            "models": [{
                "slug": "gpt-6-astra",
                "default_reasoning_level": "low",
                "supported_reasoning_levels": [
                    { "effort": "low", "description": "Low" },
                    { "effort": "medium", "description": "Medium" },
                    { "effort": "high", "description": "High" },
                    { "effort": "xhigh", "description": "Extra high" },
                    { "effort": "max", "description": "Max" }
                ]
            }]
        }));

        let serialized = serde_json::to_value(&models[0]).expect("serialize fetched model");
        assert_eq!(
            serialized.pointer("/reasoning/supportedEfforts"),
            Some(&json!(["low", "medium", "high", "xhigh", "max"]))
        );
        assert_eq!(
            serialized.pointer("/reasoning/defaultEffort"),
            Some(&json!("low"))
        );
        assert_eq!(
            serialized.pointer("/reasoning/upstream/effortMap/none"),
            Some(&json!("low")),
            "legacy threads configured with none must migrate to Astra's lowest supported effort"
        );
    }

    #[test]
    fn parse_codex_oauth_models_filters_explicitly_unavailable_entries() {
        let models = parse_models(json!({
            "models": [
                { "slug": "gpt-5.6-luna", "supported_in_api": false },
                { "slug": "gpt-5.6-hidden", "visibility": "hide" },
                { "slug": "gpt-5.6-disabled", "disabled": true },
                { "slug": "gpt-5.5", "supportedInApi": true },
                { "slug": "gpt-5.4" }
            ]
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["gpt-5.4".to_string(), "gpt-5.5".to_string()]
        );
    }

    #[test]
    fn parse_trusted_cached_models_does_not_guess_official_identity_from_the_model_name() {
        let models = parse_cached_models(json!({
            "models": [
                { "slug": "gpt-6-astra", "owned_by": "openai" },
                { "slug": "aurora-code", "provider": "Codex" }
            ]
        }));

        assert_eq!(
            models.into_iter().map(|model| model.id).collect::<Vec<_>>(),
            vec!["aurora-code".to_string(), "gpt-6-astra".to_string()]
        );
    }

    #[tokio::test]
    async fn public_official_catalog_supports_future_names_without_importing_instructions() {
        let (url, server) = spawn_public_catalog_server(json!({
            "models": [{
                "slug": "aurora-code",
                "display_name": "Aurora Code",
                "supported_in_api": true,
                "model_messages": {"instructions_template": "remote instructions"},
                "base_instructions": "remote base instructions"
            }]
        }))
        .await;

        let models = fetch_public_official_catalog_from_url(&url)
            .await
            .expect("fetch public official catalog");
        server.abort();

        assert_eq!(models[0]["slug"], json!("aurora-code"));
        assert!(models[0].get("model_messages").is_none());
        assert!(models[0].get("base_instructions").is_none());
    }
}
