//! 请求转发器
//!
//! 负责将请求转发到上游Provider，支持故障转移

use super::hyper_client::{ProxyResponse, MAX_RESPONSE_BODY_BYTES};
use super::providers::{
    hosted_tools::bridge::{
        append_tool_outputs_to_chat_request, execute_hosted_tool_calls, scan_hosted_tool_calls,
        HostedToolCallScan, HostedToolLoopConfig, HOSTED_TOOL_LOOP_HEADER,
        MAX_HOSTED_TOOL_ITERATIONS,
    },
    hosted_tools::openai_client::OpenAiHostedToolClient,
    transform_codex_chat::CodexToolContext,
};
use super::{
    body_filter::filter_private_params_with_whitelist,
    content_encoding::{decompress_body_with_limit, get_content_encoding},
    error::*,
    failover_switch::FailoverSwitchManager,
    json_canonical::{canonicalize_value, short_value_hash},
    log_codes::fwd as log_fwd,
    provider_router::ProviderRouter,
    providers::{
        codex_chat_history::CodexChatHistoryStore, gemini_shadow::GeminiShadowStore, get_adapter,
        streaming_retry::StreamReconnector, AuthInfo, AuthStrategy, ProviderAdapter, ProviderType,
    },
    thinking_budget_rectifier::{rectify_thinking_budget, should_rectify_thinking_budget},
    thinking_rectifier::{
        normalize_thinking_type, rectify_anthropic_request, should_rectify_thinking_signature,
    },
    types::{CopilotOptimizerConfig, OptimizerConfig, ProxyStatus, RectifierConfig},
    ProxyError,
};
use crate::commands::{CodexOAuthState, CopilotAuthState, XaiOAuthState};
use crate::proxy::providers::codex_oauth_auth::CodexOAuthManager;
use crate::proxy::providers::codex_oauth_auth::NATIVE_CODEX_ACCOUNT_ID;
use crate::proxy::providers::codex_oauth_pool::CodexPoolAttemptOutcome;
use crate::proxy::providers::copilot_auth::CopilotAuthManager;
use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
use crate::proxy::providers::CODEX_ACCOUNT_POOL_ENABLED;
use crate::{
    app_config::AppType,
    provider::{LocalProxyRequestOverrides, Provider},
};
use bytes::Bytes;
use futures::StreamExt;
use http::Extensions;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Manager;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const PROXY_AUTH_PLACEHOLDER: &str = "PROXY_MANAGED";
const CODEX_RESPONSES_LITE_FALLBACK_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// 同一 Provider 在流建立前发生连接/请求构造失败时，CCSM 自身额外尝试的次数。
///
/// Codex 端 `request_max_retries` 是从客户端重新 POST；这里是在代理内直接复用
/// 已转换好的请求体重试，减少不稳定网络下“一次 send 失败就整轮失败”的概率。
const UPSTREAM_TRANSPORT_RETRY_LIMIT: usize = 2;
/// 真实上游在尚未建立成功响应时明确返回 429，等价于拒绝接收本次采样请求，
/// 因而可以安全重放同一份 headers/body。次数与 Codex 默认流恢复预算对齐。
const CODEX_RATE_LIMIT_RETRY_LIMIT: usize = 5;
const CODEX_RATE_LIMIT_MAX_SINGLE_DELAY: Duration = Duration::from_secs(60);
const CODEX_RATE_LIMIT_TOTAL_DELAY_BUDGET: Duration = Duration::from_secs(180);

fn validate_codex_official_authorization(headers: &http::HeaderMap) -> Result<(), ProxyError> {
    let authorization = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    match authorization {
        None | Some("") => Err(ProxyError::AuthError(
            "Codex 官方登录不可用，请先在 Codex 中完成 ChatGPT 登录".to_string(),
        )),
        Some(value) if value.contains(PROXY_AUTH_PLACEHOLDER) => Err(ProxyError::AuthError(
            "已切换到 OpenAI 官方供应商，请重启 Codex 或新建会话以加载官方登录配置".to_string(),
        )),
        Some(_) => Ok(()),
    }
}

fn should_passthrough_codex_official_auth(
    app_type: &AppType,
    provider: &Provider,
    headers: &http::HeaderMap,
) -> bool {
    matches!(app_type, AppType::Codex)
        && super::providers::provider_uses_native_codex_auth(provider)
        && !headers.contains_key("x-cc-switch-external-openai-api")
}

pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    pub claude_api_format: Option<String>,
    /// 实际发往上游的模型名（路由接管/模型映射后的真值）。
    ///
    /// usage 归因不能依赖 ctx.request_model（映射前的客户端别名）：上游响应
    /// 缺失 model 或回显别名时，接管流量会被记成 claude-* 并按其定价计费。
    pub outbound_model: Option<String>,
    /// 活跃连接 RAII guard：随响应一起流转到 response_processor / handle_claude_transform，
    /// 最终被 move 进流式 body future（或非流式响应作用域），覆盖整个响应生命周期。
    pub(crate) connection_guard: Option<ActiveConnectionGuard>,
    /// 流式 Responses 请求的上游重连工厂：下游转换层在流中断且尚未向客户端
    /// 转发实质内容时用它做有界自动重连（见 providers::streaming_retry）。
    pub(crate) stream_reconnect: Option<StreamReconnector>,
}

pub struct ForwardError {
    pub error: ProxyError,
    pub provider: Option<Provider>,
}

/// 活跃连接 RAII guard
///
/// 构造时把 `ProxyStatus.active_connections` +1；Drop 时在 tokio runtime 上调度
/// 一个异步任务执行 -1，从而支持把 guard move 进流式 body future（stream 自然结束
/// 时 guard 与 future 一起 drop）。
///
/// 设计动机：之前在 `forward_with_retry` 出口处同步 -1，但流式响应的 body 实际
/// 在 `create_logged_passthrough_stream` 内还会继续 yield 字节流，导致 UI 的
/// `active_connections` 计数过早归零。RAII guard 让"减量"由 Rust 类型系统驱动，
/// 不需要每条出口路径都手动调用。
pub(crate) struct ActiveConnectionGuard {
    status: Arc<RwLock<ProxyStatus>>,
}

impl ActiveConnectionGuard {
    pub(crate) async fn acquire(status: Arc<RwLock<ProxyStatus>>) -> Self {
        {
            let mut s = status.write().await;
            s.active_connections = s.active_connections.saturating_add(1);
        }
        Self { status }
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        // Drop 不能 await：把减量操作调度到 tokio runtime
        let status = self.status.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut s = status.write().await;
                s.active_connections = s.active_connections.saturating_sub(1);
            });
        }
        // 没有 runtime 时静默丢失计数（仅 UI 展示用，可接受最终一致性）
    }
}

/// 已连接的 Codex GPT-Live WebSocket 上游流。
pub struct CodexRealtimeWebSocketStream(
    pub tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
);

pub struct RequestForwarder {
    /// 共享的 ProviderRouter（持有熔断器状态）
    router: Arc<ProviderRouter>,
    status: Arc<RwLock<ProxyStatus>>,
    current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    gemini_shadow: Arc<GeminiShadowStore>,
    codex_chat_history: Arc<CodexChatHistoryStore>,
    /// 故障转移切换管理器
    failover_manager: Arc<FailoverSwitchManager>,
    /// AppHandle，用于发射事件和更新托盘
    app_handle: Option<tauri::AppHandle>,
    /// 请求开始时的"当前供应商 ID"（用于判断是否需要同步 UI/托盘）
    current_provider_id_at_start: String,
    /// 代理会话 ID（用于 Gemini Native shadow replay）
    session_id: String,
    /// Session ID 是否由客户端提供；生成值不能作为上游缓存身份。
    session_client_provided: bool,
    /// 是否允许保留可信本地 Codex 客户端提供的 first-party originator。
    preserve_codex_client_originator: bool,
    /// 整流器配置
    rectifier_config: RectifierConfig,
    /// 优化器配置
    optimizer_config: OptimizerConfig,
    /// Copilot 优化器配置
    copilot_optimizer_config: CopilotOptimizerConfig,
    /// Codex Responses-Lite 上游能力负缓存。
    ///
    /// key 按 provider + 上游 URL path + 模型隔离；value 是过期时间。命中时直接
    /// 去掉 `x-openai-internal-codex-responses-lite`，过期后重新带头探测，避免每次
    /// 请求都先失败一次，也避免永久禁用未来可能支持 Lite 的上游。
    codex_responses_lite_fallbacks: Arc<RwLock<HashMap<String, Instant>>>,
    /// 非流式请求超时（秒）
    non_streaming_timeout: std::time::Duration,
    /// 流式请求响应头等待超时（秒）
    streaming_first_byte_timeout: std::time::Duration,
    /// 单个客户端请求最多尝试的 provider 数。
    ///
    /// 由 `AppProxyConfig.max_retries` (UI: "请求失败时的重试次数, 0-10") 派生：
    /// `max_attempts = max_retries + 1`，所以 max_retries=0 表示仅尝试一家、
    /// max_retries=3（默认）表示最多 4 家。loop 同时受 providers.len() 自然限制。
    max_attempts: usize,
}

fn provider_requests_codex_account_pool(provider: &Provider) -> bool {
    provider
        .settings_config
        .get(CODEX_ACCOUNT_POOL_ENABLED)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn provider_codex_pool_account(provider: &Provider) -> Option<(&str, u64)> {
    let account_id = provider
        .settings_config
        .get("codexPoolAccountId")?
        .as_str()?;
    let credential_generation = provider
        .settings_config
        .get("codexPoolCredentialGeneration")?
        .as_u64()?;
    Some((account_id, credential_generation))
}

fn classify_codex_pool_attempt(error: &ProxyError) -> CodexPoolAttemptOutcome {
    match error {
        ProxyError::AuthError(_) => CodexPoolAttemptOutcome::Credential { status: None },
        ProxyError::UpstreamError { status, .. } if matches!(*status, 401 | 403) => {
            CodexPoolAttemptOutcome::Credential {
                status: Some(*status),
            }
        }
        ProxyError::UpstreamError { status, .. } if matches!(*status, 402 | 429) => {
            CodexPoolAttemptOutcome::Quota { status: *status }
        }
        ProxyError::Timeout(_)
        | ProxyError::ForwardFailed(_)
        | ProxyError::ProviderUnhealthy(_)
        | ProxyError::StreamIdleTimeout(_) => CodexPoolAttemptOutcome::Transient { status: None },
        ProxyError::UpstreamError {
            status: 400 | 405 | 406 | 413 | 414 | 415 | 422 | 501,
            ..
        } => CodexPoolAttemptOutcome::Neutral,
        ProxyError::UpstreamError { status, .. } if *status >= 500 => {
            CodexPoolAttemptOutcome::Transient {
                status: Some(*status),
            }
        }
        _ => CodexPoolAttemptOutcome::Neutral,
    }
}

fn retryable_failure_affects_provider_health(provider: &Provider, error: &ProxyError) -> bool {
    provider_codex_pool_account(provider).is_none()
        || matches!(
            classify_codex_pool_attempt(error),
            CodexPoolAttemptOutcome::Neutral
        )
}

impl RequestForwarder {
    /// 把 retry 层已经选中的 Codex route 物化为真实目标 provider。
    ///
    /// `resolve_codex_model_routed_providers` 会生成带 `codexResolvedRouteId` 和父路由
    /// 归因的 request-local provider。这个标记同时会让
    /// `forward()` 跳过再次解析，因此必须在进入 account pool / retry loop 前完成
    /// targetProviderId 物化，否则候选会错误继承父 MultiRouter 的本地 base_url。
    fn materialize_codex_forward_attempt_provider(
        &self,
        app_type: &AppType,
        provider: &Provider,
    ) -> Result<Provider, ProxyError> {
        if !matches!(app_type, AppType::Codex) {
            return Ok(provider.clone());
        }
        let Some(target_provider_id) = super::providers::codex_route_target_provider_id(provider)
        else {
            return Ok(provider.clone());
        };
        let target_provider = self
            .router
            .get_provider_by_id(target_provider_id, app_type.as_str())
            .map_err(|err| {
                ProxyError::ConfigError(format!(
                    "读取 Codex retry route 目标供应商 '{target_provider_id}' 失败: {err}"
                ))
            })?
            .ok_or_else(|| {
                ProxyError::ConfigError(format!(
                    "Codex retry route 引用了不存在的目标供应商 '{target_provider_id}'"
                ))
            })?;
        Ok(
            super::providers::materialize_codex_routed_provider_from_target(
                provider,
                &target_provider,
            ),
        )
    }

    async fn expand_codex_account_pool(
        &self,
        app_type: &AppType,
        headers: &http::HeaderMap,
        providers: Vec<Provider>,
    ) -> Vec<Provider> {
        if !matches!(app_type, AppType::Codex)
            || headers.contains_key("x-cc-switch-external-openai-api")
            || !providers.iter().any(provider_requests_codex_account_pool)
        {
            return providers;
        }
        let Some(app_handle) = &self.app_handle else {
            return providers;
        };
        let state = app_handle.state::<CodexOAuthState>();
        let manager = state.0.read().await;
        let policy = manager.account_pool_policy().await;
        if !policy.enabled {
            log::warn!("[CodexOAuthPool] 当前 Router 选择了 OAuth 账号池，但全局账号池未启用");
            return providers;
        }
        let native_authorization = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let native_token = native_authorization
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        // 先让 manager 观察 Desktop Authorization 代际，再发 quota 探测。
        // 否则新凭据的首个 quota 快照会先写入旧代际，随后被身份切换清掉。
        let _ = manager
            .ordered_pool_entries(&self.session_id, native_authorization)
            .await;
        let probes = policy
            .entries
            .iter()
            .filter(|entry| entry.enabled)
            .map(|entry| {
                let manager = &manager;
                let native_token = native_token.clone();
                async move {
                    if !manager.pool_quota_refresh_due(&entry.account_id).await {
                        return None;
                    }
                    let token = if entry.account_id == NATIVE_CODEX_ACCOUNT_ID {
                        native_token
                    } else {
                        manager
                            .get_valid_token_for_account(&entry.account_id)
                            .await
                            .ok()
                    };
                    let result = match token {
                        Some(token) => {
                            crate::services::subscription::query_codex_remaining_percent(
                                &token,
                                (entry.account_id != NATIVE_CODEX_ACCOUNT_ID)
                                    .then_some(entry.account_id.as_str()),
                            )
                            .await
                        }
                        None => Err("account token unavailable".to_string()),
                    };
                    Some((entry.account_id.clone(), result))
                }
            });
        for (account_id, result) in futures::future::join_all(probes)
            .await
            .into_iter()
            .flatten()
        {
            match result {
                Ok(remaining) => {
                    manager
                        .record_pool_remaining_percent(&account_id, remaining)
                        .await;
                }
                Err(error) => {
                    log::warn!("[CodexOAuthPool] 额度刷新失败 account={account_id}: {error}");
                    manager.mark_pool_quota_checked(&account_id).await;
                }
            }
        }
        let entries = state
            .0
            .read()
            .await
            .ordered_pool_entries(&self.session_id, native_authorization)
            .await;
        let mut expanded = Vec::new();
        for provider in providers {
            if !provider_requests_codex_account_pool(&provider) {
                expanded.push(provider);
                continue;
            }
            for pool_candidate in &entries {
                let entry = &pool_candidate.entry;
                let mut candidate = provider.clone();
                candidate.settings_config["codexPoolCredentialGeneration"] =
                    Value::from(pool_candidate.credential_generation);
                if entry.account_id == NATIVE_CODEX_ACCOUNT_ID {
                    candidate.settings_config["codexNativeAuthPassthrough"] = Value::Bool(true);
                    candidate.settings_config["codexPoolAccountId"] =
                        Value::String(NATIVE_CODEX_ACCOUNT_ID.to_string());
                    if let Some(meta) = candidate.meta.as_mut() {
                        meta.provider_type = None;
                        meta.auth_binding = None;
                        meta.api_format = Some("openai_responses".to_string());
                    }
                } else {
                    candidate.settings_config["codexNativeAuthPassthrough"] = Value::Bool(false);
                    candidate.settings_config["codexPoolAccountId"] =
                        Value::String(entry.account_id.clone());
                    let meta = candidate.meta.get_or_insert_with(Default::default);
                    meta.provider_type = Some("codex_oauth".to_string());
                    meta.api_format = Some("openai_responses".to_string());
                    meta.auth_binding = Some(crate::provider::AuthBinding {
                        source: crate::provider::AuthBindingSource::ManagedAccount,
                        auth_provider: Some("codex_oauth".to_string()),
                        account_id: Some(entry.account_id.clone()),
                    });
                }
                candidate.id = format!("{}::account::{}", candidate.id, entry.account_id);
                candidate.name = format!("{} [{}]", candidate.name, entry.account_id);
                expanded.push(candidate);
            }
        }
        expanded
    }

    /// 记录本阶段已有的请求发出前/首包结果边界。
    ///
    /// media 二次重试成功与完整 SSE terminal 仍由后续阶段补齐，不能把首包
    /// `Success` 描述成最终流式结果。
    async fn record_codex_pool_attempt(
        &self,
        provider: &Provider,
        outcome: CodexPoolAttemptOutcome,
    ) {
        let Some((account_id, credential_generation)) = provider_codex_pool_account(provider)
        else {
            return;
        };
        let Some(app_handle) = &self.app_handle else {
            return;
        };
        let state = app_handle.state::<CodexOAuthState>();
        let manager = state.0.read().await;
        let _ = manager
            .record_pool_attempt(account_id, credential_generation, &self.session_id, outcome)
            .await;
    }

    /// 预防式 media 降级：发送前对 text-only 模型把图片块替换为标记。
    ///
    /// 受 `enabled && request_media_fallback` 管辖；其中"启发式模型名单预测"
    /// 再受 `request_media_heuristic` 单独管辖（显式声明 text-only 始终生效）。
    /// 返回被替换的图片块数量（0 = 未触发或开关关闭）。
    fn apply_media_prevention(&self, body: &mut Value, provider: &Provider) -> usize {
        if !(self.rectifier_config.enabled && self.rectifier_config.request_media_fallback) {
            return 0;
        }
        let replaced_images = super::media_sanitizer::replace_images_for_text_only_model(
            body,
            provider,
            self.rectifier_config.request_media_heuristic,
        );
        if replaced_images > 0 {
            let model = body.get("model").and_then(Value::as_str).unwrap_or("");
            log::info!(
                "[Media] Replaced {replaced_images} image block(s) with {} for text-only provider={}, model={}",
                super::media_sanitizer::UNSUPPORTED_IMAGE_MARKER,
                provider.id,
                model
            );
        }
        replaced_images
    }

    /// 反应式 media 重试判定：上游因图片输入报错后，是否应替换图片块并对同一供应商重试一次。
    ///
    /// 受 `enabled && request_media_fallback` 管辖；不涉及 `request_media_heuristic`——
    /// 这里是上游"实测"错误后的纯恢复，不是预测，故启发式开关与它无关。
    fn media_retry_should_trigger(
        &self,
        adapter_name: &str,
        already_retried: bool,
        provider_body: &Value,
        error: &ProxyError,
    ) -> bool {
        matches!(adapter_name, "Claude" | "Codex")
            && self.rectifier_config.enabled
            && self.rectifier_config.request_media_fallback
            && !already_retried
            && super::media_sanitizer::contains_image_blocks(provider_body)
            && super::media_sanitizer::is_retriable_image_error(error)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        router: Arc<ProviderRouter>,
        non_streaming_timeout: u64,
        status: Arc<RwLock<ProxyStatus>>,
        current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
        gemini_shadow: Arc<GeminiShadowStore>,
        codex_chat_history: Arc<CodexChatHistoryStore>,
        failover_manager: Arc<FailoverSwitchManager>,
        app_handle: Option<tauri::AppHandle>,
        current_provider_id_at_start: String,
        session_id: String,
        session_client_provided: bool,
        preserve_codex_client_originator: bool,
        streaming_first_byte_timeout: u64,
        _streaming_idle_timeout: u64,
        rectifier_config: RectifierConfig,
        optimizer_config: OptimizerConfig,
        copilot_optimizer_config: CopilotOptimizerConfig,
        max_retries: u32,
    ) -> Self {
        // max_retries 是「失败后重试次数」语义，attempt 上限 = retries + 1。
        // saturating_add 防止 u32::MAX + 1 溢出。
        let max_attempts = (max_retries as usize).saturating_add(1);
        Self {
            router,
            status,
            current_providers,
            gemini_shadow,
            codex_chat_history,
            failover_manager,
            app_handle,
            current_provider_id_at_start,
            session_id,
            session_client_provided,
            preserve_codex_client_originator,
            rectifier_config,
            optimizer_config,
            copilot_optimizer_config,
            codex_responses_lite_fallbacks: Arc::new(RwLock::new(HashMap::new())),
            non_streaming_timeout: std::time::Duration::from_secs(non_streaming_timeout),
            streaming_first_byte_timeout: std::time::Duration::from_secs(
                streaming_first_byte_timeout,
            ),
            max_attempts,
        }
    }

    /// 判断当前 Codex Responses-Lite fallback 负缓存是否仍然有效。
    ///
    /// 参数:
    /// - `key`: 已按 provider、上游 URL 与模型归一化后的缓存 key。
    ///   返回:
    /// - `true` 表示本次请求应直接去掉 Lite 头；`false` 表示应带头重新探测。
    ///   副作用:
    /// - 如果缓存条目已经过期，会顺手删除，避免内存里长期保留无效能力结果。
    async fn codex_responses_lite_fallback_active(&self, key: &str) -> bool {
        let mut fallbacks = self.codex_responses_lite_fallbacks.write().await;
        codex_responses_lite_fallback_active_at(&mut fallbacks, key, Instant::now())
    }

    /// 记录一个短期 Responses-Lite fallback 负缓存。
    ///
    /// 只有上游明确返回 Lite 不支持错误后才调用；缓存过期后下一次请求会重新带头
    /// 探测，防止第三方上游未来支持该协议后仍被永久去头。
    async fn mark_codex_responses_lite_fallback(&self, key: String) {
        let now = Instant::now();
        let mut fallbacks = self.codex_responses_lite_fallbacks.write().await;
        if fallbacks.len() > 512 {
            fallbacks.retain(|_, expires_at| *expires_at > now);
        }
        fallbacks.insert(key, now + CODEX_RESPONSES_LITE_FALLBACK_TTL);
    }

    async fn record_success_result(
        &self,
        circuit_provider_id: &str,
        health_provider_id: &str,
        app_type: &str,
        used_half_open_permit: bool,
    ) {
        if used_half_open_permit {
            let provider_id = health_provider_id;
            if let Err(e) = self
                .router
                .record_result_with_health_provider(
                    circuit_provider_id,
                    health_provider_id,
                    app_type,
                    true,
                    true,
                    None,
                )
                .await
            {
                log::warn!(
                    "[{app_type}] 记录 Provider 成功结果失败: provider_id={provider_id}, error={e}"
                );
            }
            return;
        }

        let router = self.router.clone();
        let circuit_provider_id = circuit_provider_id.to_string();
        let health_provider_id = health_provider_id.to_string();
        let app_type = app_type.to_string();
        tokio::spawn(async move {
            let provider_id = health_provider_id.clone();
            if let Err(e) = router
                .record_result_with_health_provider(
                    &circuit_provider_id,
                    &health_provider_id,
                    &app_type,
                    false,
                    true,
                    None,
                )
                .await
            {
                log::warn!(
                    "[{app_type}] 异步记录 Provider 成功结果失败: provider_id={provider_id}, error={e}"
                );
            }
        });
    }

    /// 整流（thinking signature 或 budget）重试失败后的统一收尾。
    ///
    /// `None` 表示已记录熔断器、累积 `last_error`/`last_provider`，
    /// 调用方应 `continue` 让下一家 provider 继续故障转移；
    /// `Some(ForwardError)` 表示是客户端错误，没有 provider 能修复，
    /// 调用方应直接 `return` 把错误返回给客户端。
    #[allow(clippy::too_many_arguments)]
    async fn handle_rectifier_retry_failure(
        &self,
        retry_err: ProxyError,
        provider: &Provider,
        app_type_str: &str,
        used_half_open_permit: bool,
        rectifier_label: &str,
        last_error: &mut Option<ProxyError>,
        last_provider: &mut Option<Provider>,
    ) -> Option<ForwardError> {
        // Provider 错误：本家上游/网络确实出问题，下一家 provider 可能可用 → 继续故障转移。
        // 客户端错误：整流后请求仍违法，下一家也修不好 → 直接返回。
        let is_provider_error = match &retry_err {
            ProxyError::Timeout(_) | ProxyError::ForwardFailed(_) => true,
            ProxyError::UpstreamError { status, .. } => *status >= 500,
            _ => false,
        };

        if is_provider_error {
            let (persistent_provider_id, _) =
                super::providers::codex_route_persistent_provider(provider);
            let _ = self
                .router
                .record_result_with_health_provider(
                    &provider.id,
                    persistent_provider_id,
                    app_type_str,
                    used_half_open_permit,
                    false,
                    Some(retry_err.to_string()),
                )
                .await;
            {
                let mut status = self.status.write().await;
                status.last_error = Some(format!(
                    "Provider {} {rectifier_label}重试失败: {}",
                    provider.name, retry_err
                ));
            }
            *last_error = Some(retry_err);
            *last_provider = Some(provider.clone());
            return None;
        }

        self.router
            .release_permit_neutral(&provider.id, app_type_str, used_half_open_permit)
            .await;
        let mut status = self.status.write().await;
        status.failed_requests += 1;
        status.last_error = Some(retry_err.to_string());
        if status.total_requests > 0 {
            status.success_rate =
                (status.success_requests as f32 / status.total_requests as f32) * 100.0;
        }
        Some(ForwardError {
            error: retry_err,
            provider: Some(provider.clone()),
        })
    }

    /// 转发请求（带故障转移）
    ///
    /// 这是 thin wrapper：在客户端请求维度记一次 `total_requests` / 调整
    /// `active_connections` / 刷新 `last_request_at`，无论 inner 走哪条出口路径，
    /// 出口处都会把 `active_connections` 回收。Per-attempt 维度（成功/失败/熔断
    /// 等）仍由 inner 内自行更新 `success_requests` / `failed_requests`。
    #[allow(clippy::too_many_arguments)]
    pub async fn forward_with_retry(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        body: Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        let guard = ActiveConnectionGuard::acquire(self.status.clone()).await;
        {
            let mut s = self.status.write().await;
            s.total_requests = s.total_requests.saturating_add(1);
            s.last_request_at = Some(chrono::Utc::now().to_rfc3339());
        }
        let result = self
            .forward_with_retry_inner(
                app_type, method, endpoint, body, headers, extensions, providers,
            )
            .await;
        // 把 guard 注入到 Ok 结果，让它随响应一起流转到 response_processor，
        // 在流式 body 的 future 内才真正 drop。
        // Err 路径：guard 在函数 scope 内随返回值落地时自动 drop。
        result.map(|mut fr| {
            fr.connection_guard = Some(guard);
            fr
        })
    }

    /// 转发未知 OpenAI-compatible endpoint 的原始请求体。
    ///
    /// 该入口用于 `/v1/*` 兜底：只用 `route_body` 做模型路由判断，真正发往
    /// 上游的是客户端原始 `raw_body`，因此 multipart、音频、文件上传或未来
    /// OpenAI endpoint 不会被本地 JSON 化。已知 `/responses` 等 endpoint 仍应
    /// 走专用 handler，本函数不做格式转换或 body 改写。
    #[allow(clippy::too_many_arguments)]
    pub async fn forward_raw_with_retry(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        route_body: Value,
        raw_body: Bytes,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        let guard = ActiveConnectionGuard::acquire(self.status.clone()).await;
        {
            let mut s = self.status.write().await;
            s.total_requests = s.total_requests.saturating_add(1);
            s.last_request_at = Some(chrono::Utc::now().to_rfc3339());
        }
        let result = self
            .forward_raw_with_retry_inner(
                app_type, method, endpoint, route_body, raw_body, headers, extensions, providers,
            )
            .await;
        result.map(|mut fr| {
            fr.connection_guard = Some(guard);
            fr
        })
    }

    /// 原始请求体转发的故障转移循环。
    ///
    /// 与 JSON 转换路径相比，这里刻意不触发 thinking/media rectifier：未知
    /// OpenAI-compatible endpoint 的 body 可能不是 JSON，强行整流会破坏载荷。
    #[allow(clippy::too_many_arguments)]
    async fn forward_raw_with_retry_inner(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        route_body: Value,
        raw_body: Bytes,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        let adapter = get_adapter(app_type);
        let app_type_str = app_type.as_str();

        if providers.is_empty() {
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }

        // Unknown OpenAI-compatible endpoints keep the router provider and let
        // `forward_raw` resolve only the explicitly matched route or official
        // Codex OAuth. Expanding the whole route chain here would let native
        // image/audio/file requests fail over to text-only DeepSeek/Qwen routes.
        let attempt_providers = providers.to_vec();
        let attempt_providers = self
            .expand_codex_account_pool(app_type, &headers, attempt_providers)
            .await;
        let bypass_circuit_breaker = attempt_providers.len() == 1;
        let mut last_error = None;
        let mut last_provider = None;
        let mut attempted_providers = 0usize;

        for provider in attempt_providers.iter() {
            if attempted_providers >= self.max_attempts {
                break;
            }

            let attempt_provider_id = provider.id.clone();
            let (persistent_provider_id, persistent_provider_name) =
                super::providers::codex_route_persistent_provider(provider);
            let persistent_provider_id = persistent_provider_id.to_string();
            let persistent_provider_name = persistent_provider_name.to_string();
            let (allowed, used_half_open_permit) = if bypass_circuit_breaker {
                (true, false)
            } else {
                let permit = self
                    .router
                    .allow_provider_request(&provider.id, app_type_str)
                    .await;
                (permit.allowed, permit.used_half_open_permit)
            };
            if !allowed {
                continue;
            }
            attempted_providers += 1;

            {
                let mut status = self.status.write().await;
                status.current_provider = Some(persistent_provider_name.clone());
                status.current_provider_id = Some(persistent_provider_id.clone());
            }

            match self
                .forward_raw(
                    app_type,
                    &method,
                    provider,
                    endpoint,
                    &route_body,
                    raw_body.clone(),
                    &headers,
                    &extensions,
                    adapter.as_ref(),
                )
                .await
            {
                Ok((response, effective_provider, outbound_model)) => {
                    self.record_codex_pool_attempt(provider, CodexPoolAttemptOutcome::Success)
                        .await;
                    self.record_success_result(
                        &attempt_provider_id,
                        &persistent_provider_id,
                        app_type_str,
                        used_half_open_permit,
                    )
                    .await;
                    {
                        let mut current_providers = self.current_providers.write().await;
                        current_providers.insert(
                            app_type_str.to_string(),
                            (
                                persistent_provider_id.clone(),
                                persistent_provider_name.clone(),
                            ),
                        );
                    }
                    {
                        let mut status = self.status.write().await;
                        status.success_requests += 1;
                        status.last_error = None;
                        if self.current_provider_id_at_start.as_str()
                            != persistent_provider_id.as_str()
                        {
                            status.failover_count += 1;
                            let fm = self.failover_manager.clone();
                            let ah = self.app_handle.clone();
                            let pid = persistent_provider_id.clone();
                            let pname = persistent_provider_name.clone();
                            let at = app_type_str.to_string();
                            tokio::spawn(async move {
                                let _ = fm.try_switch(ah.as_ref(), &at, &pid, &pname).await;
                            });
                        }
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                    }

                    return Ok(ForwardResult {
                        response,
                        provider: effective_provider,
                        claude_api_format: None,
                        outbound_model,
                        connection_guard: None,
                        stream_reconnect: None,
                    });
                }
                Err(error) => {
                    self.record_codex_pool_attempt(provider, classify_codex_pool_attempt(&error))
                        .await;
                    let category = self.categorize_proxy_error(&error, &provider);
                    if matches!(category, ErrorCategory::NonRetryable) {
                        self.router
                            .release_permit_neutral(
                                &provider.id,
                                app_type_str,
                                used_half_open_permit,
                            )
                            .await;
                        let mut status = self.status.write().await;
                        status.failed_requests += 1;
                        status.last_error = Some(error.to_string());
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                        return Err(ForwardError {
                            error,
                            provider: Some(provider.clone()),
                        });
                    }

                    if retryable_failure_affects_provider_health(provider, &error) {
                        let _ = self
                            .router
                            .record_result_with_health_provider(
                                &provider.id,
                                &persistent_provider_id,
                                app_type_str,
                                used_half_open_permit,
                                false,
                                Some(error.to_string()),
                            )
                            .await;
                    } else {
                        self.router
                            .release_permit_neutral(
                                &provider.id,
                                app_type_str,
                                used_half_open_permit,
                            )
                            .await;
                    }
                    {
                        let mut status = self.status.write().await;
                        status.last_error = Some(error.to_string());
                    }
                    last_error = Some(error);
                    last_provider = Some(provider.clone());
                }
            }
        }

        Err(ForwardError {
            error: last_error.unwrap_or(ProxyError::MaxRetriesExceeded),
            provider: last_provider,
        })
    }

    /// 实际转发逻辑（不包含客户端维度的入口/出口计数）
    ///
    /// # Arguments
    /// * `app_type` - 应用类型
    /// * `method` - 客户端请求的 HTTP 方法（透传给上游，支持 GET/POST 等）
    /// * `endpoint` - API 端点
    /// * `body` - 请求体
    /// * `headers` - 请求头
    /// * `providers` - 已选择的 Provider 列表（由 RequestContext 提供，避免重复调用 select_providers）
    #[allow(clippy::too_many_arguments)]
    async fn forward_with_retry_inner(
        &self,
        app_type: &AppType,
        method: http::Method,
        endpoint: &str,
        mut body: Value,
        headers: axum::http::HeaderMap,
        extensions: Extensions,
        providers: Vec<Provider>,
    ) -> Result<ForwardResult, ForwardError> {
        // 获取适配器
        let adapter = get_adapter(app_type);
        let app_type_str = app_type.as_str();

        if providers.is_empty() {
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }

        if matches!(app_type, AppType::Codex)
            && CodexRequestMetadataSummary::from_request(app_type, endpoint, &body, &headers)
                .request_kind
                == "compaction"
        {
            let current_model = crate::codex_state_db::codex_thread_model(&self.session_id);
            let original_model = body
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            if apply_codex_compaction_current_model(&mut body, current_model.as_deref()) {
                let routed_model = body
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                log::debug!(
                    "[CodexRouter] compaction model {} -> current session model {}",
                    original_model,
                    routed_model
                );
            }
        }

        if matches!(app_type, AppType::Codex) {
            super::providers::transform_codex_chat::restore_codex_compaction_summary_in_request(
                &mut body,
            );
        }

        let route_attempt_providers =
            build_forward_attempt_providers_preserving_codex_router_context(
                app_type, &providers, &body,
            );
        let attempt_providers = route_attempt_providers
            .iter()
            .map(|provider| self.materialize_codex_forward_attempt_provider(app_type, provider))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| ForwardError {
                error,
                provider: route_attempt_providers.first().cloned(),
            })?;
        let attempt_providers = self
            .expand_codex_account_pool(app_type, &headers, attempt_providers)
            .await;
        let mut last_error = None;
        let mut last_provider = None;
        let mut attempted_providers = 0usize;

        // 单 Provider 场景下跳过熔断器检查（故障转移关闭时）
        let bypass_circuit_breaker = attempt_providers.len() == 1;

        // 依次尝试每个供应商
        for provider in attempt_providers.iter() {
            let attempt_provider_id = provider.id.clone();
            let (persistent_provider_id, persistent_provider_name) =
                super::providers::codex_route_persistent_provider(provider);
            let persistent_provider_id = persistent_provider_id.to_string();
            let persistent_provider_name = persistent_provider_name.to_string();

            // 整流器重试标记：每个 provider 独立持有，避免标记跨 provider 短路故障转移
            // —— 首家 provider 整流后被 5xx/timeout 击落时，下家仍能用整流后的请求体走整流流程
            let mut rectifier_retried = false;
            let mut budget_rectifier_retried = false;
            let mut media_rectifier_retried = false;

            // 上限检查：尊重用户在 AppProxyConfig.max_retries 上配置的「重试次数」。
            // 放在熔断器 allow 检查之前，避免在已经超限时还占用 HalfOpen 探测名额。
            if attempted_providers >= self.max_attempts {
                log::warn!(
                    "[{app_type_str}] 已达最大尝试次数上限 ({}/{}), 停止故障转移",
                    attempted_providers,
                    self.max_attempts
                );
                break;
            }

            // 发起请求前先获取熔断器放行许可（HalfOpen 会占用探测名额）
            // 单 Provider 场景下跳过此检查，避免熔断器阻塞所有请求
            let (allowed, used_half_open_permit) = if bypass_circuit_breaker {
                (true, false)
            } else {
                let permit = self
                    .router
                    .allow_provider_request(&provider.id, app_type_str)
                    .await;
                (permit.allowed, permit.used_half_open_permit)
            };

            if !allowed {
                continue;
            }

            // PRE-SEND 优化器：每个 provider 独立决定是否优化
            // clone body 以避免 Bedrock 优化字段泄漏到非 Bedrock provider（failover 场景）
            let mut provider_body =
                if self.optimizer_config.enabled && is_bedrock_provider(provider) {
                    let mut b = body.clone();
                    if self.optimizer_config.thinking_optimizer {
                        super::thinking_optimizer::optimize(&mut b, &self.optimizer_config);
                    }
                    if self.optimizer_config.cache_injection {
                        super::cache_injector::inject(&mut b, &self.optimizer_config);
                    }
                    b
                } else {
                    body.clone()
                };

            attempted_providers += 1;

            // 更新状态中的当前 Provider 信息（per-attempt 维度的标识）
            //
            // total_requests / last_request_at / active_connections 已由
            // forward_with_retry wrapper 在客户端请求维度统一处理，这里只刷
            // 新「正在尝试哪个 provider」的展示字段。
            {
                let mut status = self.status.write().await;
                status.current_provider = Some(persistent_provider_name.clone());
                status.current_provider_id = Some(persistent_provider_id.clone());
            }

            // 转发请求（每个 Provider 只尝试一次，重试由客户端控制）
            match self
                .forward(
                    app_type,
                    &method,
                    provider,
                    endpoint,
                    &provider_body,
                    &headers,
                    &extensions,
                    adapter.as_ref(),
                )
                .await
            {
                Ok((
                    response,
                    claude_api_format,
                    effective_provider,
                    outbound_model,
                    stream_reconnect,
                )) => {
                    self.record_codex_pool_attempt(provider, CodexPoolAttemptOutcome::Success)
                        .await;
                    // 成功：普通闭合熔断状态异步记录，避免阻塞流式首包返回；
                    // HalfOpen 探测仍同步等待，保证 permit 与熔断状态及时释放。
                    self.record_success_result(
                        &attempt_provider_id,
                        &persistent_provider_id,
                        app_type_str,
                        used_half_open_permit,
                    )
                    .await;

                    // 更新当前应用类型使用的 provider
                    {
                        let mut current_providers = self.current_providers.write().await;
                        current_providers.insert(
                            app_type_str.to_string(),
                            (
                                persistent_provider_id.clone(),
                                persistent_provider_name.clone(),
                            ),
                        );
                    }

                    // 更新成功统计
                    {
                        let mut status = self.status.write().await;
                        status.success_requests += 1;
                        status.last_error = None;
                        let should_switch = self.current_provider_id_at_start.as_str()
                            != persistent_provider_id.as_str();
                        if should_switch {
                            status.failover_count += 1;

                            // 异步触发供应商切换，更新 UI/托盘，并把“当前供应商”同步为实际使用的 provider
                            let fm = self.failover_manager.clone();
                            let ah = self.app_handle.clone();
                            let pid = persistent_provider_id.clone();
                            let pname = persistent_provider_name.clone();
                            let at = app_type_str.to_string();

                            tokio::spawn(async move {
                                let _ = fm.try_switch(ah.as_ref(), &at, &pid, &pname).await;
                            });
                        }
                        // 重新计算成功率
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                    }

                    return Ok(ForwardResult {
                        response,
                        provider: effective_provider,
                        claude_api_format,
                        outbound_model,
                        connection_guard: None,
                        stream_reconnect,
                    });
                }
                Err(e) => {
                    self.record_codex_pool_attempt(provider, classify_codex_pool_attempt(&e))
                        .await;
                    // 检测是否需要触发整流器（仅 Claude/ClaudeAuth 供应商）
                    let provider_type = ProviderType::from_app_type_and_config(app_type, provider);
                    let is_anthropic_provider = matches!(
                        provider_type,
                        ProviderType::Claude | ProviderType::ClaudeAuth
                    );
                    let mut signature_rectifier_non_retryable_client_error = false;

                    if self.media_retry_should_trigger(
                        adapter.name(),
                        media_rectifier_retried,
                        &provider_body,
                        &e,
                    ) {
                        let mut media_body = provider_body.clone();
                        let replaced_images =
                            super::media_sanitizer::replace_image_blocks_with_marker(
                                &mut media_body,
                            );

                        if replaced_images > 0 {
                            let _ = std::mem::replace(&mut media_rectifier_retried, true);
                            let model = media_body
                                .get("model")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            log::info!(
                                "[{app_type_str}] [Media] Upstream rejected image input; retrying provider={} model={} with {replaced_images} image block(s) replaced by {}",
                                provider.id,
                                model,
                                super::media_sanitizer::UNSUPPORTED_IMAGE_MARKER
                            );

                            match self
                                .forward(
                                    app_type,
                                    &method,
                                    provider,
                                    endpoint,
                                    &media_body,
                                    &headers,
                                    &extensions,
                                    adapter.as_ref(),
                                )
                                .await
                            {
                                Ok((
                                    response,
                                    claude_api_format,
                                    routed_provider,
                                    outbound_model,
                                    stream_reconnect,
                                )) => {
                                    log::info!("[{app_type_str}] [Media] Image retry succeeded");
                                    self.record_success_result(
                                        &attempt_provider_id,
                                        &persistent_provider_id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;

                                    {
                                        let mut current_providers =
                                            self.current_providers.write().await;
                                        current_providers.insert(
                                            app_type_str.to_string(),
                                            (
                                                persistent_provider_id.clone(),
                                                persistent_provider_name.clone(),
                                            ),
                                        );
                                    }

                                    {
                                        let mut status = self.status.write().await;
                                        status.success_requests += 1;
                                        status.last_error = None;
                                        let should_switch =
                                            self.current_provider_id_at_start.as_str()
                                                != persistent_provider_id.as_str();
                                        if should_switch {
                                            status.failover_count += 1;
                                            let fm = self.failover_manager.clone();
                                            let ah = self.app_handle.clone();
                                            let pid = persistent_provider_id.clone();
                                            let pname = persistent_provider_name.clone();
                                            let at = app_type_str.to_string();

                                            tokio::spawn(async move {
                                                let _ = fm
                                                    .try_switch(ah.as_ref(), &at, &pid, &pname)
                                                    .await;
                                            });
                                        }
                                        if status.total_requests > 0 {
                                            status.success_rate = (status.success_requests as f32
                                                / status.total_requests as f32)
                                                * 100.0;
                                        }
                                    }

                                    return Ok(ForwardResult {
                                        response,
                                        provider: routed_provider,
                                        claude_api_format,
                                        outbound_model,
                                        connection_guard: None,
                                        stream_reconnect,
                                    });
                                }
                                Err(retry_err) => {
                                    log::warn!(
                                        "[{app_type_str}] [Media] Image retry still failed: {retry_err}"
                                    );
                                    if let Some(err) = self
                                        .handle_rectifier_retry_failure(
                                            retry_err,
                                            provider,
                                            app_type_str,
                                            used_half_open_permit,
                                            "media 降级",
                                            &mut last_error,
                                            &mut last_provider,
                                        )
                                        .await
                                    {
                                        return Err(err);
                                    }
                                    continue;
                                }
                            }
                        }
                    }

                    if is_anthropic_provider {
                        let error_message = extract_error_message(&e);
                        if should_rectify_thinking_signature(
                            error_message.as_deref(),
                            &self.rectifier_config,
                        ) {
                            // 已经重试过：直接返回错误（不可重试客户端错误）
                            if rectifier_retried {
                                log::warn!("[{app_type_str}] [RECT-005] 整流器已触发过，不再重试");
                                // 释放 HalfOpen permit（不记录熔断器，这是客户端兼容性问题）
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            // 首次触发：整流请求体
                            let rectified = rectify_anthropic_request(&mut provider_body);

                            // 整流未生效：继续尝试 budget 整流路径，避免误判后短路
                            if !rectified.applied {
                                log::warn!(
                                    "[{app_type_str}] [RECT-006] thinking 签名整流器触发但无可整流内容，继续检查 budget；若 budget 也未命中则按客户端错误返回"
                                );
                                signature_rectifier_non_retryable_client_error = true;
                            } else {
                                log::info!(
                                    "[{}] [RECT-001] thinking 签名整流器触发, 移除 {} thinking blocks, {} redacted_thinking blocks, {} signature fields",
                                    app_type_str,
                                    rectified.removed_thinking_blocks,
                                    rectified.removed_redacted_thinking_blocks,
                                    rectified.removed_signature_fields
                                );

                                // 标记已重试（当前逻辑下重试后必定 return，保留标记以备将来扩展）
                                let _ = std::mem::replace(&mut rectifier_retried, true);

                                // 使用同一供应商重试（不计入熔断器）
                                match self
                                    .forward(
                                        app_type,
                                        &method,
                                        provider,
                                        endpoint,
                                        &provider_body,
                                        &headers,
                                        &extensions,
                                        adapter.as_ref(),
                                    )
                                    .await
                                {
                                    Ok((
                                        response,
                                        claude_api_format,
                                        effective_provider,
                                        outbound_model,
                                        stream_reconnect,
                                    )) => {
                                        log::info!("[{app_type_str}] [RECT-002] 整流重试成功");
                                        self.record_success_result(
                                            &attempt_provider_id,
                                            &persistent_provider_id,
                                            app_type_str,
                                            used_half_open_permit,
                                        )
                                        .await;

                                        // 更新当前应用类型使用的 provider
                                        {
                                            let mut current_providers =
                                                self.current_providers.write().await;
                                            current_providers.insert(
                                                app_type_str.to_string(),
                                                (
                                                    persistent_provider_id.clone(),
                                                    persistent_provider_name.clone(),
                                                ),
                                            );
                                        }

                                        // 更新成功统计
                                        {
                                            let mut status = self.status.write().await;
                                            status.success_requests += 1;
                                            status.last_error = None;
                                            let should_switch =
                                                self.current_provider_id_at_start.as_str()
                                                    != persistent_provider_id.as_str();
                                            if should_switch {
                                                status.failover_count += 1;

                                                // 异步触发供应商切换，更新 UI/托盘
                                                let fm = self.failover_manager.clone();
                                                let ah = self.app_handle.clone();
                                                let pid = persistent_provider_id.clone();
                                                let pname = persistent_provider_name.clone();
                                                let at = app_type_str.to_string();

                                                tokio::spawn(async move {
                                                    let _ = fm
                                                        .try_switch(ah.as_ref(), &at, &pid, &pname)
                                                        .await;
                                                });
                                            }
                                            if status.total_requests > 0 {
                                                status.success_rate = (status.success_requests
                                                    as f32
                                                    / status.total_requests as f32)
                                                    * 100.0;
                                            }
                                        }

                                        return Ok(ForwardResult {
                                            response,
                                            provider: effective_provider,
                                            claude_api_format,
                                            outbound_model,
                                            connection_guard: None,
                                            stream_reconnect,
                                        });
                                    }
                                    Err(retry_err) => {
                                        log::warn!(
                                            "[{app_type_str}] [RECT-003] 整流重试仍失败: {retry_err}"
                                        );
                                        if let Some(err) = self
                                            .handle_rectifier_retry_failure(
                                                retry_err,
                                                provider,
                                                app_type_str,
                                                used_half_open_permit,
                                                "整流",
                                                &mut last_error,
                                                &mut last_provider,
                                            )
                                            .await
                                        {
                                            return Err(err);
                                        }
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // 检测是否需要触发 budget 整流器（仅 Claude/ClaudeAuth 供应商）
                    if is_anthropic_provider {
                        let error_message = extract_error_message(&e);
                        if should_rectify_thinking_budget(
                            error_message.as_deref(),
                            &self.rectifier_config,
                        ) {
                            // 已经重试过：直接返回错误（不可重试客户端错误）
                            if budget_rectifier_retried {
                                log::warn!(
                                    "[{app_type_str}] [RECT-013] budget 整流器已触发过，不再重试"
                                );
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            let budget_rectified = rectify_thinking_budget(&mut provider_body);
                            if !budget_rectified.applied {
                                log::warn!(
                                    "[{app_type_str}] [RECT-014] budget 整流器触发但无可整流内容，不做无意义重试"
                                );
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                                return Err(ForwardError {
                                    error: e,
                                    provider: Some(provider.clone()),
                                });
                            }

                            log::info!(
                                "[{}] [RECT-010] thinking budget 整流器触发, before={:?}, after={:?}",
                                app_type_str,
                                budget_rectified.before,
                                budget_rectified.after
                            );

                            let _ = std::mem::replace(&mut budget_rectifier_retried, true);

                            // 使用同一供应商重试（不计入熔断器）
                            match self
                                .forward(
                                    app_type,
                                    &method,
                                    provider,
                                    endpoint,
                                    &provider_body,
                                    &headers,
                                    &extensions,
                                    adapter.as_ref(),
                                )
                                .await
                            {
                                Ok((
                                    response,
                                    claude_api_format,
                                    effective_provider,
                                    outbound_model,
                                    stream_reconnect,
                                )) => {
                                    log::info!("[{app_type_str}] [RECT-011] budget 整流重试成功");
                                    self.record_success_result(
                                        &attempt_provider_id,
                                        &persistent_provider_id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;

                                    {
                                        let mut current_providers =
                                            self.current_providers.write().await;
                                        current_providers.insert(
                                            app_type_str.to_string(),
                                            (
                                                persistent_provider_id.clone(),
                                                persistent_provider_name.clone(),
                                            ),
                                        );
                                    }

                                    {
                                        let mut status = self.status.write().await;
                                        status.success_requests += 1;
                                        status.last_error = None;
                                        let should_switch =
                                            self.current_provider_id_at_start.as_str()
                                                != persistent_provider_id.as_str();
                                        if should_switch {
                                            status.failover_count += 1;
                                            let fm = self.failover_manager.clone();
                                            let ah = self.app_handle.clone();
                                            let pid = persistent_provider_id.clone();
                                            let pname = persistent_provider_name.clone();
                                            let at = app_type_str.to_string();
                                            tokio::spawn(async move {
                                                let _ = fm
                                                    .try_switch(ah.as_ref(), &at, &pid, &pname)
                                                    .await;
                                            });
                                        }
                                        if status.total_requests > 0 {
                                            status.success_rate = (status.success_requests as f32
                                                / status.total_requests as f32)
                                                * 100.0;
                                        }
                                    }

                                    return Ok(ForwardResult {
                                        response,
                                        provider: effective_provider,
                                        claude_api_format,
                                        outbound_model,
                                        connection_guard: None,
                                        stream_reconnect,
                                    });
                                }
                                Err(retry_err) => {
                                    log::warn!(
                                        "[{app_type_str}] [RECT-012] budget 整流重试仍失败: {retry_err}"
                                    );
                                    if let Some(err) = self
                                        .handle_rectifier_retry_failure(
                                            retry_err,
                                            provider,
                                            app_type_str,
                                            used_half_open_permit,
                                            "budget 整流",
                                            &mut last_error,
                                            &mut last_provider,
                                        )
                                        .await
                                    {
                                        return Err(err);
                                    }
                                    continue;
                                }
                            }
                        }
                    }

                    if signature_rectifier_non_retryable_client_error {
                        self.router
                            .release_permit_neutral(
                                &provider.id,
                                app_type_str,
                                used_half_open_permit,
                            )
                            .await;
                        let mut status = self.status.write().await;
                        status.failed_requests += 1;
                        status.last_error = Some(e.to_string());
                        if status.total_requests > 0 {
                            status.success_rate = (status.success_requests as f32
                                / status.total_requests as f32)
                                * 100.0;
                        }
                        return Err(ForwardError {
                            error: e,
                            provider: Some(provider.clone()),
                        });
                    }

                    // 先分类错误，决定是否计入 provider 健康度
                    // —— NonRetryable / ClientAbort 是客户端层错误，无论换哪家 provider 都会被拒绝，
                    //    不应污染熔断器和数据库健康度（与 release_permit_neutral 同语义）。
                    let category = self.categorize_proxy_error(&e, provider);

                    match category {
                        ErrorCategory::Retryable => {
                            // 账号池的 credential/quota/transient 属于临时候选账号，
                            // 只释放 permit 并切换池内下一账号，不污染持久 Router 健康。
                            if retryable_failure_affects_provider_health(provider, &e) {
                                let _ = self
                                    .router
                                    .record_result_with_health_provider(
                                        &provider.id,
                                        &persistent_provider_id,
                                        app_type_str,
                                        used_half_open_permit,
                                        false,
                                        Some(e.to_string()),
                                    )
                                    .await;
                            } else {
                                self.router
                                    .release_permit_neutral(
                                        &provider.id,
                                        app_type_str,
                                        used_half_open_permit,
                                    )
                                    .await;
                            }

                            {
                                let mut status = self.status.write().await;
                                status.last_error =
                                    Some(format!("Provider {} 失败: {}", provider.name, e));
                            }

                            let (log_code, log_message) = build_retryable_failure_log(
                                &provider.name,
                                attempted_providers,
                                attempt_providers.len(),
                                &e,
                            );
                            log::warn!("[{app_type_str}] [{log_code}] {log_message}");

                            last_error = Some(e);
                            last_provider = Some(provider.clone());
                            // 继续尝试下一个供应商
                            continue;
                        }
                        ErrorCategory::NonRetryable | ErrorCategory::ClientAbort => {
                            // 不可重试：客户端层错误或客户端断连 → 不污染健康度，仅释放 HalfOpen permit
                            self.router
                                .release_permit_neutral(
                                    &provider.id,
                                    app_type_str,
                                    used_half_open_permit,
                                )
                                .await;
                            {
                                let mut status = self.status.write().await;
                                status.failed_requests += 1;
                                status.last_error = Some(e.to_string());
                                if status.total_requests > 0 {
                                    status.success_rate = (status.success_requests as f32
                                        / status.total_requests as f32)
                                        * 100.0;
                                }
                            }
                            return Err(ForwardError {
                                error: e,
                                provider: Some(provider.clone()),
                            });
                        }
                    }
                }
            }
        }

        if attempted_providers == 0 {
            // providers 列表非空，但全部被熔断器拒绝（典型：HalfOpen 探测名额被占用）
            {
                let mut status = self.status.write().await;
                status.failed_requests += 1;
                status.last_error = Some("所有供应商暂时不可用（熔断器限制）".to_string());
                if status.total_requests > 0 {
                    status.success_rate =
                        (status.success_requests as f32 / status.total_requests as f32) * 100.0;
                }
            }
            return Err(ForwardError {
                error: ProxyError::NoAvailableProvider,
                provider: None,
            });
        }

        // 所有供应商都失败了
        {
            let mut status = self.status.write().await;
            status.failed_requests += 1;
            status.last_error = Some("所有供应商都失败".to_string());
            if status.total_requests > 0 {
                status.success_rate =
                    (status.success_requests as f32 / status.total_requests as f32) * 100.0;
            }
        }

        if let Some((log_code, log_message)) = build_terminal_failure_log(
            attempted_providers,
            attempt_providers.len(),
            last_error.as_ref(),
        ) {
            log::warn!("[{app_type_str}] [{log_code}] {log_message}");
        }

        Err(ForwardError {
            error: last_error.unwrap_or(ProxyError::MaxRetriesExceeded),
            provider: last_provider,
        })
    }

    /// 转发单个请求（使用适配器）
    ///
    /// 成功时返回 `(response, claude_api_format, effective_provider, outbound_model,
    /// stream_reconnect)`，其中 `outbound_model` 是最终发往上游的模型名
    /// （所有映射/改写之后），`stream_reconnect` 是流式 Responses 请求的上游重连工厂。
    #[allow(clippy::too_many_arguments)]
    async fn forward(
        &self,
        app_type: &AppType,
        method: &http::Method,
        provider: &Provider,
        endpoint: &str,
        body: &Value,
        headers: &axum::http::HeaderMap,
        extensions: &Extensions,
        adapter: &dyn ProviderAdapter,
    ) -> Result<
        (
            ProxyResponse,
            Option<String>,
            Provider,
            Option<String>,
            Option<StreamReconnector>,
        ),
        ProxyError,
    > {
        // enforce 只约束经过本机代理的 Codex 请求；未接入 CCSwitchMulti 的原生
        // 客户端无法被本地进程拦截，不能把该策略误描述为账号级强制控制。
        if matches!(app_type, AppType::Codex) {
            if let Some(reason) = crate::services::quota_collaboration::codex_enforcement_reason(
                chrono::Utc::now().timestamp(),
            ) {
                return Err(ProxyError::UpstreamError {
                    status: 429,
                    body: Some(
                        serde_json::json!({
                            "error": {
                                "message": reason,
                                "type": "quota_collaboration_enforced"
                            }
                        })
                        .to_string(),
                    ),
                });
            }
        }
        let codex_trace_id =
            matches!(app_type, AppType::Codex).then(|| uuid::Uuid::new_v4().to_string());
        let route_started_at = std::time::Instant::now();
        let request_model_for_log = body
            .get("model")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        let codex_request_metadata =
            CodexRequestMetadataSummary::from_request(app_type, endpoint, body, headers);
        let (outer_provider_id, outer_provider_name) = {
            let (id, name) = super::providers::codex_route_persistent_provider(provider);
            (id.to_string(), name.to_string())
        };

        // Codex v2 是一个复合 provider：Codex 客户端只看到一个 provider bucket，
        // Rust proxy 根据请求模型临时解析真实上游 provider，后续 base_url/auth/转换逻辑
        // 都使用这个 effective provider。
        let provider_is_resolved_codex_route = provider
            .settings_config
            .get("codexResolvedRouteId")
            .is_some();
        let codex_router_configured =
            matches!(app_type, AppType::Codex) && codex_provider_has_routing_config(provider);
        let codex_router_provider = provider;
        let routed_provider = if matches!(app_type, AppType::Codex) {
            (!provider_is_resolved_codex_route)
                .then(|| super::providers::resolve_codex_model_routed_provider(provider, body))
                .flatten()
        } else {
            None
        };
        let routed_provider = if let Some(route_provider) = routed_provider {
            if let Some(target_provider_id) =
                super::providers::codex_route_target_provider_id(&route_provider)
            {
                let Some(target_provider) = self
                    .router
                    .get_provider_by_id(target_provider_id, app_type.as_str())
                    .map_err(|err| {
                        ProxyError::ConfigError(format!(
                            "读取 Codex route 目标供应商 '{target_provider_id}' 失败: {err}"
                        ))
                    })?
                else {
                    return Err(ProxyError::ConfigError(format!(
                        "Codex route 引用了不存在的目标供应商 '{target_provider_id}'"
                    )));
                };
                Some(
                    super::providers::materialize_codex_routed_provider_from_target(
                        &route_provider,
                        &target_provider,
                    ),
                )
            } else {
                Some(route_provider)
            }
        } else {
            None
        };
        let codex_route_missed = codex_router_configured
            && !provider_is_resolved_codex_route
            && routed_provider.is_none();
        let provider = routed_provider.as_ref().unwrap_or(provider);

        if let Some(trace_id) = codex_trace_id.as_deref() {
            let route_id = provider
                .settings_config
                .get("codexResolvedRouteId")
                .and_then(|value| value.as_str())
                .unwrap_or("<none>");
            super::codex_router_log::append_event(
                "route_resolved",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("endpoint", endpoint.to_string()),
                    ("model", request_model_for_log.clone()),
                    (
                        "codex_request_kind",
                        codex_request_metadata.request_kind.clone(),
                    ),
                    ("outer_provider", outer_provider_id.clone()),
                    ("outer_name", outer_provider_name.clone()),
                    ("effective_provider", provider.id.clone()),
                    ("effective_name", provider.name.clone()),
                    ("route_id", route_id.to_string()),
                    ("routing_configured", codex_router_configured.to_string()),
                    ("route_missed", codex_route_missed.to_string()),
                    (
                        "elapsed_ms",
                        route_started_at.elapsed().as_millis().to_string(),
                    ),
                ],
            );
        }

        if let Some(routed_provider) = routed_provider.as_ref() {
            let request_model = body
                .get("model")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            log::debug!(
                "[CodexRouter] model={} routed to provider={} ({})",
                request_model,
                routed_provider.id,
                routed_provider.name
            );
        }
        // 使用适配器提取 base_url
        let mut base_url = adapter.extract_base_url(provider)?;
        if let Err(error) = reject_codex_effective_local_proxy_upstream(
            app_type,
            &base_url,
            &format!("model '{request_model_for_log}'"),
        ) {
            let request_model = body
                .get("model")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            if let Some(trace_id) = codex_trace_id.as_deref() {
                super::codex_router_log::append_event(
                    "route_error",
                    &[
                        ("trace", trace_id.to_string()),
                        ("session", self.session_id.clone()),
                        ("endpoint", endpoint.to_string()),
                        ("model", request_model.to_string()),
                        ("outer_provider", outer_provider_id.clone()),
                        ("fallback_base_url", base_url.clone()),
                        (
                            "reason",
                            "effective_upstream_local_proxy_self_loop".to_string(),
                        ),
                    ],
                );
            }
            return Err(error);
        }

        let is_full_url = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false)
            && !provider.is_codex_oauth()
            && !provider.is_xai_oauth();

        // GitHub Copilot API 使用 /chat/completions（无 /v1 前缀）
        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot")
            || base_url.contains("githubcopilot.com");

        // Codex upstream conversion mode — computed early because the [1m]-suffix strip
        // below must be skipped on the Anthropic path (the marker has to survive to
        // catalog matching and to the transform's own strip+beta detection).
        let codex_responses_to_chat = matches!(app_type, AppType::Codex | AppType::GrokBuild)
            && super::providers::should_convert_codex_responses_to_chat(provider, endpoint);
        let codex_responses_to_messages = matches!(app_type, AppType::Codex)
            && super::providers::should_convert_codex_responses_to_messages(provider, endpoint);
        let codex_responses_to_anthropic = matches!(app_type, AppType::Codex | AppType::GrokBuild)
            && super::providers::should_convert_codex_responses_to_anthropic(provider, endpoint);
        let codex_official_auth_passthrough =
            should_passthrough_codex_official_auth(app_type, provider, headers);

        if codex_official_auth_passthrough {
            validate_codex_official_authorization(headers)?;
        }

        // 应用模型映射（独立于格式转换）
        // Claude Desktop proxy 模式必须先把 Desktop 可见的 claude-* route
        // 映射成真实上游模型名，并且未知 route 要直接报错，不能使用默认模型兜底。
        let mapped_body = if matches!(app_type, AppType::ClaudeDesktop) {
            crate::claude_desktop_config::map_proxy_request_model(body.clone(), provider)
                .map_err(|e| ProxyError::InvalidRequest(e.to_string()))?
        } else {
            let (mapped_body, _original_model, _mapped_model) =
                super::model_mapper::apply_model_mapping(body.clone(), provider);
            mapped_body
        };

        // 与 CCH 对齐：请求前不做 thinking 主动改写（仅保留兼容入口）
        let mut mapped_body = normalize_thinking_type(mapped_body);

        if should_project_codex_agent_messages_for_provider(app_type, provider, endpoint) {
            let projected =
                super::providers::codex_multi_agent::project_codex_agent_messages_for_third_party(
                    &mut mapped_body,
                )?;
            if projected > 0 {
                log::debug!(
                    "[CodexRouter] Projected {projected} plaintext agent message(s) for third-party provider={} route",
                    provider.id
                );
            }
        }

        // Grok Build exposes a stable client-side model profile in config.toml.
        // Route requests to the provider's real upstream model before applying
        // the optional Responses -> Chat/Anthropic bridge.
        if matches!(app_type, AppType::GrokBuild) {
            super::providers::apply_codex_upstream_model(provider, &mut mapped_body);
        }

        if is_copilot {
            mapped_body =
                super::providers::copilot_model_map::apply_copilot_model_normalization(mapped_body);
            self.apply_copilot_live_model_resolution(provider, &mut mapped_body)
                .await;
            // Strip the [1M] context marker after Copilot normalization/resolve.
            // A user's mapped value (e.g. "gpt-5.6-sol[1M]") carries [1M] as a
            // Claude Code context-capability declaration that upstream APIs reject
            // as part of the model name. The preceding normalization step already
            // rewrites claude-xxx[1M] into the "-1m" dash form Copilot accepts, and
            // the strip helper only touches the "[1m]" bracket form, so "-1m"
            // variants pass through unchanged.
            mapped_body =
                super::model_mapper::strip_one_m_suffix_for_upstream_from_body(mapped_body);
        } else if !codex_responses_to_anthropic {
            // Skip on the Codex→Anthropic path: stripping [1m] here would break both the
            // model-catalog match (apply_codex_upstream_model) and the transform's own
            // strip+`context-1m` beta detection. The marker is stripped later, on the
            // final anthropic_body.
            mapped_body =
                super::model_mapper::strip_one_m_suffix_for_upstream_from_body(mapped_body);
        }

        // --- Copilot 优化器：分类 + 请求体优化（在格式转换之前执行） ---
        // 注意：确定性 ID 也在此处计算，因为 mapped_body 在格式转换时会被 move
        //
        // 执行顺序（与 copilot-api 对齐）：
        //   1. 先在原始 body 上分类（保留 tool_result 语义，避免误判为 user）
        //   2. 再清洗孤立 tool_result（防止上游 API 报错）
        //   3. 再合并 tool_result + text（减少 premium 计费）
        let copilot_optimization = if is_copilot && self.copilot_optimizer_config.enabled {
            // 1. 在原始 body 上分类 — 必须在清洗/合并之前执行
            //    孤立 tool_result 仍保持 tool_result 类型，分类能正确识别为 agent
            let has_anthropic_beta = headers.contains_key("anthropic-beta");
            let classification = super::copilot_optimizer::classify_request(
                &mapped_body,
                has_anthropic_beta,
                self.copilot_optimizer_config.compact_detection,
                self.copilot_optimizer_config.subagent_detection,
            );

            log::debug!(
                "[Copilot] 优化器分类: initiator={}, is_warmup={}, is_compact={}, is_subagent={}",
                classification.initiator,
                classification.is_warmup,
                classification.is_compact,
                classification.is_subagent
            );

            // 2. 孤立 tool_result 清理 — 分类完成后再清洗
            //    防止上游 API 因不匹配的 tool_result 报错导致重试/重复计费
            mapped_body = super::copilot_optimizer::sanitize_orphan_tool_results(mapped_body);

            // 3. Tool result 合并 — 将 [tool_result, text] 变为 [tool_result(含text)]
            if self.copilot_optimizer_config.tool_result_merging {
                mapped_body = super::copilot_optimizer::merge_tool_results(mapped_body);
            }

            // 3.5. 主动剥离 thinking block — Copilot 走 OpenAI 兼容端点不识别该块
            //      避免上游拒绝后由 rectifier 反应式重试（首次请求已消耗 quota）
            if self.copilot_optimizer_config.strip_thinking {
                mapped_body = super::copilot_optimizer::strip_thinking_blocks(mapped_body);
            }

            // 4. Warmup 小模型降级
            if self.copilot_optimizer_config.warmup_downgrade && classification.is_warmup {
                log::info!(
                    "[Copilot] Warmup 请求降级到模型: {}",
                    self.copilot_optimizer_config.warmup_model
                );
                mapped_body["model"] =
                    serde_json::json!(&self.copilot_optimizer_config.warmup_model);
            }

            // 预计算确定性 Request ID（在 body 被 move 之前）
            // Session 提取优先级（与 session.rs extract_from_metadata 对齐）：
            //   1. metadata.user_id 中的 _session_ 后缀
            //   2. metadata.session_id（直接字段）
            //   3. raw metadata.user_id（整串 fallback）
            //   4. x-session-id header
            let metadata = body.get("metadata");
            let session_id = metadata
                .and_then(|m| m.get("user_id"))
                .and_then(|v| v.as_str())
                .and_then(super::session::parse_session_from_user_id)
                .or_else(|| {
                    metadata
                        .and_then(|m| m.get("session_id"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    metadata
                        .and_then(|m| m.get("user_id"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    headers
                        .get("x-session-id")
                        .and_then(|v| v.to_str().ok())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();
            let det_request_id = if self.copilot_optimizer_config.deterministic_request_id {
                Some(super::copilot_optimizer::deterministic_request_id(
                    &mapped_body,
                    &session_id,
                ))
            } else {
                None
            };

            // 从 session ID 派生稳定的 interaction ID（同一主对话共享）
            let interaction_id =
                super::copilot_optimizer::deterministic_interaction_id(&session_id);

            Some((classification, det_request_id, interaction_id))
        } else {
            None
        };

        // GitHub Copilot 动态 endpoint 路由
        // 从 CopilotAuthManager 获取缓存的 API endpoint（支持企业版等非默认 endpoint）
        if is_copilot && !is_full_url {
            if let Some(app_handle) = &self.app_handle {
                let copilot_state = app_handle.state::<CopilotAuthState>();
                let copilot_auth = copilot_state.0.read().await;

                // 从 provider.meta 获取关联的 GitHub 账号 ID
                let account_id = provider
                    .meta
                    .as_ref()
                    .and_then(|m| m.managed_account_id_for("github_copilot"));

                let dynamic_endpoint = match &account_id {
                    Some(id) => copilot_auth.get_api_endpoint(id).await,
                    None => copilot_auth.get_default_api_endpoint().await,
                };

                // 只在动态 endpoint 与当前 base_url 不同时替换
                if dynamic_endpoint != base_url {
                    log::debug!(
                        "[Copilot] 使用动态 API endpoint: {} (原: {})",
                        dynamic_endpoint,
                        base_url
                    );
                    base_url = dynamic_endpoint;
                }
            }
        }
        let resolved_claude_api_format = if adapter.name() == "Claude" {
            Some(
                self.resolve_claude_api_format(provider, &mapped_body, is_copilot)
                    .await,
            )
        } else {
            None
        };
        if adapter.name() == "Claude" {
            if let Some(api_format) = resolved_claude_api_format.as_deref() {
                super::providers::normalize_anthropic_messages_for_provider(
                    &mut mapped_body,
                    provider,
                    api_format,
                );
                self.apply_media_prevention(&mut mapped_body, provider);
            }
        }
        let needs_transform = match resolved_claude_api_format.as_deref() {
            Some(api_format) => super::providers::claude_api_format_needs_transform(api_format),
            None => adapter.needs_transform(provider),
        };
        // Codex → Anthropic: Claude Code emulation is off by default and only
        // enabled when the user explicitly turns it on in the UI, so requests can
        // pass a gateway's "Claude Code only" fingerprint check (User-Agent /
        // anthropic-beta / x-app / system prompt first line). Defaulting to off
        // avoids leaking the Claude Code fingerprint and identity prompt to
        // general-purpose gateways.
        let codex_impersonate_claude_code = codex_responses_to_anthropic
            && provider
                .meta
                .as_ref()
                .and_then(|meta| meta.impersonate_claude_code)
                == Some(true);
        let (effective_endpoint, passthrough_query) = if codex_responses_to_chat {
            rewrite_codex_responses_endpoint_to_chat(endpoint)
        } else if codex_responses_to_messages {
            rewrite_codex_responses_endpoint_to_messages(endpoint)
        } else if codex_responses_to_anthropic {
            rewrite_codex_responses_endpoint_to_anthropic(endpoint)
        } else if needs_transform && adapter.name() == "Claude" {
            let api_format = resolved_claude_api_format
                .as_deref()
                .unwrap_or_else(|| super::providers::get_claude_api_format(provider));
            rewrite_claude_transform_endpoint(endpoint, api_format, is_copilot, &mapped_body)
        } else {
            (
                endpoint.to_string(),
                split_endpoint_and_query(endpoint)
                    .1
                    .map(ToString::to_string),
            )
        };

        let codex_chat_base_is_full_endpoint =
            codex_responses_to_chat && base_url_is_full_endpoint(&base_url, "/chat/completions");

        // Defensive fallback mirroring `codex_chat_base_is_full_endpoint`: if a user pastes
        // a base URL already ending in the Anthropic `/v1/messages` endpoint but leaves the
        // "full URL" switch off, treat it as a full endpoint so we don't double-append
        // `/v1/messages` (→ `.../v1/messages/v1/messages`, a non-retryable 400). Matches the
        // exact endpoint suffix, so prefixed gateways like `.../api/v1/messages` are covered.
        let codex_anthropic_base_is_full_endpoint =
            codex_responses_to_anthropic && base_url_is_full_endpoint(&base_url, "/v1/messages");

        let url = if matches!(resolved_claude_api_format.as_deref(), Some("gemini_native")) {
            super::gemini_url::resolve_gemini_native_url(
                &base_url,
                &effective_endpoint,
                is_full_url,
            )
        } else if is_full_url
            || codex_chat_base_is_full_endpoint
            || codex_anthropic_base_is_full_endpoint
        {
            append_query_to_full_url(&base_url, passthrough_query.as_deref())
        } else {
            adapter.build_url(&base_url, &effective_endpoint)
        };

        // 记录映射后的出站模型名（此时 mapped_body 已完成接管映射 / [1m] 剥离 /
        // Copilot 归一化）。格式转换后若 body 仍带 model 字段会在下方刷新覆盖；
        // gemini_native 等模型在 URL 中的格式则保留此处的转换前真值。
        let mut outbound_model = mapped_body
            .get("model")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .map(str::to_string);

        // Codex→Anthropic: when the model name carries the [1m] marker, strip the
        // suffix and add the context-1m beta header.
        let mut codex_anthropic_one_m = false;

        // 转换请求体（如果需要）
        let request_prepare_started_at = std::time::Instant::now();
        let mut codex_chat_tool_context: Option<CodexToolContext> = None;
        let client_requested_streaming =
            is_streaming_request(&effective_endpoint, &mapped_body, headers);
        let hosted_tool_loop_allowed = !codex_responses_to_chat
            || should_enable_hosted_tool_loop(&mapped_body, client_requested_streaming);
        let mut request_body = if codex_responses_to_chat || codex_responses_to_messages {
            let mut mapped_body = mapped_body;
            let explicit_prompt_cache_key = mapped_body
                .get("prompt_cache_key")
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
            let restored = self
                .codex_chat_history
                .enrich_request(&mut mapped_body)
                .await;
            if restored > 0 {
                log::debug!(
                    "[Codex] Restored or enriched {restored} cached function call item(s) for Chat upstream"
                );
            }
            super::providers::apply_codex_chat_upstream_model(provider, &mut mapped_body);
            let reasoning_config =
                super::providers::resolve_codex_chat_reasoning_config(provider, &mapped_body);
            let text_only_override = super::providers::codex_provider_text_only_input(provider);
            let cache_config = super::providers::resolve_codex_cache_config(provider, &mapped_body);
            if codex_responses_to_chat {
                codex_chat_tool_context = Some(
                    super::providers::transform_codex_chat::build_codex_tool_context_from_request(
                        &mapped_body,
                    ),
                );
            }
            if let Some(context) = codex_chat_tool_context.as_mut() {
                context.apply_hosted_tool_switches(
                    hosted_tool_loop_allowed
                        && hosted_tool_bridge_enabled(
                            &codex_router_provider.settings_config,
                            "webSearch",
                        ),
                    hosted_tool_loop_allowed
                        && hosted_tool_bridge_enabled(
                            &codex_router_provider.settings_config,
                            "imageGeneration",
                        ),
                );
            }
            let mut chat_body = super::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning_text_only_and_cache(
                mapped_body,
                reasoning_config.as_ref(),
                text_only_override,
                Some(&cache_config),
            )?;
            super::providers::inject_codex_chat_prompt_cache_key(
                provider,
                &mut chat_body,
                explicit_prompt_cache_key.as_deref(),
                self.session_client_provided
                    .then_some(self.session_id.as_str()),
            );
            chat_body
        } else if codex_responses_to_anthropic {
            let mut mapped_body = mapped_body;
            super::providers::apply_codex_upstream_model(provider, &mut mapped_body);
            // Per-provider output ceiling override. Codex does not forward its
            // `model_max_output_tokens` in the request body, so honor the value
            // configured on the provider here — it takes precedence over any
            // request-supplied `max_output_tokens` and over the default below.
            // Injecting it into the body (rather than overriding after transform)
            // lets the thinking-budget clamp size its headroom against the real
            // ceiling too. Kept per-provider to avoid a global large default that
            // would 400 on low-output-ceiling gateways.
            if let Some(max_out) = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.max_output_tokens)
                .filter(|v| *v > 0)
            {
                mapped_body["max_output_tokens"] = Value::from(max_out);
            }
            // Anthropic requires max_tokens; fall back to this default only when the
            // Codex request omits max_output_tokens (rare — Codex normally sends it).
            // Kept conservative so a low-output-ceiling model or relay does not hard-400
            // on the fallback (a too-high default 400s and is non-retryable); 8192 is
            // accepted by every current Claude model and virtually all gateways. The
            // transform clamps any thinking budget below this value.
            const DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS: u64 = 8192;
            let mut anthropic_body =
                super::providers::transform_codex_anthropic::responses_request_to_anthropic(
                    mapped_body,
                    DEFAULT_CODEX_ANTHROPIC_MAX_TOKENS,
                )?;
            // Handle the 1M-context marker [1m]: strip the model-name suffix (the
            // gateway doesn't recognize it) and set the flag so the beta header is
            // added. apply_codex_upstream_model may have just written back a model
            // name carrying [1m] from the provider config, so strip it once more on
            // the final body here.
            if let Some(model) = anthropic_body.get("model").and_then(|v| v.as_str()) {
                let stripped = super::model_mapper::strip_one_m_suffix_for_upstream(model);
                if stripped != model {
                    codex_anthropic_one_m = true;
                    anthropic_body["model"] = Value::String(stripped.to_string());
                }
            }
            if codex_impersonate_claude_code {
                prepend_claude_code_system_prompt(&mut anthropic_body);
            }
            // Enable Anthropic prompt caching (no beta header required). Reuse the
            // configured TTL rather than silently forcing 5m on this conversion path.
            // otherwise system/tools/history are re-sent at full price every round,
            // inflating cost and first-token latency. The injector handles the
            // string→array `system` conversion and the new-breakpoint budget.
            super::cache_injector::inject(
                &mut anthropic_body,
                &codex_anthropic_cache_config(&self.optimizer_config),
            );
            anthropic_body
        } else if needs_transform {
            if adapter.name() == "Claude" {
                let api_format = resolved_claude_api_format
                    .as_deref()
                    .unwrap_or_else(|| super::providers::get_claude_api_format(provider));
                super::providers::transform_claude_request_for_api_format(
                    mapped_body,
                    provider,
                    api_format,
                    self.session_client_provided
                        .then_some(self.session_id.as_str()),
                    Some(self.gemini_shadow.as_ref()),
                )?
            } else {
                adapter.transform_request(mapped_body, provider)?
            }
        } else {
            let mut mapped_body = mapped_body;
            if matches!(app_type, AppType::Codex) {
                super::providers::apply_codex_request_upstream_model(provider, &mut mapped_body);
            }
            mapped_body
        };

        // Native Responses passthrough to a strict third-party gateway (xAI):
        // flatten Codex's private `namespace`/plugin tool declarations into
        // top-level function tools so the upstream's strict serde parser does
        // not 422 on `unknown variant "namespace"`. The Chat/Anthropic paths
        // above already unwrap namespaces, so this only fires on the native
        // passthrough. The response handler restores the flat names using a map
        // re-derived from the same request tools.
        if matches!(app_type, AppType::Codex | AppType::GrokBuild)
            && !codex_responses_to_chat
            && !codex_responses_to_anthropic
            && super::providers::provider_needs_responses_namespace_flatten(provider)
            && super::providers::transform_codex_responses_namespace::flatten_request_namespaces(
                &mut request_body,
            )?
        {
            log::debug!(
                "[Codex] Flattened namespace tools for native Responses upstream (provider={})",
                provider.id
            );
        }

        // Same native-Responses path: scrub the OpenAI-backend-private fields
        // and tool carriers (`external_web_access`, `prompt_cache_retention`,
        // `additional_tools`, `tool_search`, …) that xAI's strict serde parser
        // rejects with 400/422. Deterministic field removals only, gated on the
        // xAI OAuth path, so the prompt-cache prefix stays stable and no other
        // provider is affected. Runs after the flatten above so lifted
        // `namespace` tools survive the tool-type whitelist.
        if matches!(app_type, AppType::Codex | AppType::GrokBuild)
            && !codex_responses_to_chat
            && !codex_responses_to_anthropic
            && super::providers::provider_needs_responses_namespace_flatten(provider)
            && super::providers::transform_codex_responses_xai_sanitize::sanitize_xai_responses_request(
                &mut request_body,
            )
        {
            log::debug!(
                "[Codex] Sanitized xAI-unsupported Responses fields (provider={})",
                provider.id
            );
        }

        if matches!(app_type, AppType::Codex | AppType::GrokBuild) {
            self.apply_media_prevention(&mut request_body, provider);
        }

        // 过滤私有参数（以 `_` 开头的字段），防止内部信息泄露到上游
        // 默认使用空白名单，过滤所有 _ 前缀字段
        let codex_responses_lite_requested = matches!(app_type, AppType::Codex)
            && headers.contains_key(http::HeaderName::from_static(
                "x-openai-internal-codex-responses-lite",
            ));
        let normalize_codex_oauth_responses =
            should_normalize_codex_oauth_responses_passthrough_body(
                app_type,
                provider,
                &url,
                needs_transform,
                codex_responses_to_chat,
                codex_responses_to_messages,
            );
        let mut request_body = if normalize_codex_oauth_responses {
            super::providers::openai_compat::normalize_codex_oauth_responses_request(
                request_body,
                codex_responses_lite_requested,
            )
        } else if should_normalize_codex_responses_passthrough_control_messages(
            app_type,
            provider,
            endpoint,
            needs_transform,
            codex_responses_to_chat,
            codex_responses_to_messages,
        ) {
            super::providers::openai_compat::normalize_codex_responses_passthrough_request_for_transport(
                request_body,
                codex_responses_lite_requested,
            )
        } else {
            request_body
        };
        request_body = normalize_lm_studio_responses_request(
            app_type,
            provider,
            endpoint,
            !needs_transform
                && !codex_responses_to_chat
                && !codex_responses_to_messages
                && !codex_responses_to_anthropic,
            request_body,
        );
        if should_make_codex_v2_agents_plaintext(
            app_type,
            codex_router_provider,
            normalize_codex_oauth_responses,
        ) {
            let changed = super::providers::openai_compat::make_codex_v2_agents_messages_plaintext(
                &mut request_body,
            );
            if changed > 0 {
                log::debug!(
                    "[CodexRouter] Kept {changed} V2 agents message parameter(s) plaintext for cross-provider delivery"
                );
            }
        }
        if matches!(app_type, AppType::Codex)
            && endpoint.contains("responses")
            && super::providers::is_codex_official_provider(provider)
        {
            let changed =
                super::providers::transform_codex_chat::normalize_replayed_item_ids_for_openai(
                    &mut request_body,
                );
            if changed > 0 {
                log::debug!(
                    "[Codex] Normalized {changed} noncanonical replayed item ID(s) for OpenAI Responses (provider={})",
                    provider.id
                );
            }
        }
        let mut filtered_body = prepare_upstream_request_body(request_body);
        if !is_copilot {
            if let Some(overrides) = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref())
            {
                if apply_local_proxy_body_overrides(&mut filtered_body, overrides) {
                    filtered_body = prepare_upstream_request_body(filtered_body);
                }
            }
        }
        let hosted_tool_loop_config =
            codex_chat_tool_context
                .as_ref()
                .map(|context| HostedToolLoopConfig {
                    web_search: context.hosted_web_search_config().cloned(),
                    image_generation: context.hosted_image_generation_config().cloned(),
                });
        let hosted_tool_loop_config = hosted_tool_loop_config.filter(|config| !config.is_empty());
        let hosted_tools_forced_non_stream =
            codex_responses_to_chat && hosted_tool_loop_config.is_some();
        if hosted_tools_forced_non_stream {
            if let Some(obj) = filtered_body.as_object_mut() {
                obj.insert("stream".to_string(), serde_json::json!(false));
                obj.remove("stream_options");
            }
        }
        // 出站 body 定稿后的真实上游流式状态。与 request_is_streaming（客户端
        // Accept/body.stream 语义）分开记录，避免 hosted web_search 强制非流式时
        // 路由日志里的 streaming=true 误导排障（issue #24）。
        let upstream_stream = filtered_body
            .get("stream")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        // 出站 body 定稿后刷新真值（覆盖 Codex chat 上游模型覆写、转换层模型改写）
        if let Some(m) = filtered_body
            .get("model")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
        {
            outbound_model = Some(m.to_string());
        }
        log_prompt_cache_trace(
            app_type,
            provider,
            &effective_endpoint,
            resolved_claude_api_format.as_deref(),
            &filtered_body,
            self.session_client_provided,
        );
        let request_is_streaming =
            is_streaming_request(&effective_endpoint, &filtered_body, headers);
        let force_identity_encoding = needs_transform
            || codex_responses_to_chat
            || codex_responses_to_messages
            || codex_responses_to_anthropic
            || request_is_streaming;

        let codex_chat_request_shape =
            codex_responses_to_chat.then(|| summarize_codex_chat_request_shape(&filtered_body));
        if let Some(trace_id) = codex_trace_id.as_deref() {
            let mut fields = vec![
                ("trace", trace_id.to_string()),
                ("session", self.session_id.clone()),
                ("endpoint", endpoint.to_string()),
                ("effective_endpoint", effective_endpoint.clone()),
                ("model", request_model_for_log.clone()),
                (
                    "codex_request_kind",
                    codex_request_metadata.request_kind.clone(),
                ),
                ("provider", provider.id.clone()),
                ("upstream_url", url.clone()),
                ("responses_to_chat", codex_responses_to_chat.to_string()),
                (
                    "responses_to_messages",
                    codex_responses_to_messages.to_string(),
                ),
                (
                    "responses_to_anthropic",
                    codex_responses_to_anthropic.to_string(),
                ),
                ("streaming", request_is_streaming.to_string()),
                ("upstream_stream", upstream_stream.to_string()),
                (
                    "elapsed_ms",
                    request_prepare_started_at.elapsed().as_millis().to_string(),
                ),
            ];
            if let Some(trigger) = codex_request_metadata.compaction_trigger.as_ref() {
                fields.push(("compaction_trigger", trigger.clone()));
            }
            if let Some(reason) = codex_request_metadata.compaction_reason.as_ref() {
                fields.push(("compaction_reason", reason.clone()));
            }
            if let Some(implementation) = codex_request_metadata.compaction_implementation.as_ref()
            {
                fields.push(("compaction_implementation", implementation.clone()));
            }
            if let Some(phase) = codex_request_metadata.compaction_phase.as_ref() {
                fields.push(("compaction_phase", phase.clone()));
            }
            if codex_request_metadata.request_kind == "compaction" {
                let transport = if codex_responses_to_chat {
                    "chat_completions"
                } else if codex_responses_to_messages {
                    "messages"
                } else if codex_responses_to_anthropic {
                    "anthropic_messages"
                } else {
                    "responses_compact"
                };
                fields.push(("compaction_transport", transport.to_string()));
            }
            if let Some(shape) = codex_chat_request_shape.as_ref() {
                fields.push(("request_shape", shape.clone()));
            }
            super::codex_router_log::append_event("request_prepared", &fields);
        }

        // Codex OAuth 需要注入的 ChatGPT-Account-Id（在动态 token 获取期间填充）
        let mut codex_oauth_account_id: Option<String> = None;
        let mut is_codex_oauth = false;

        // 获取认证头（提前准备，用于内联替换）
        let auth_started_at = std::time::Instant::now();
        let mut auth_strategy_for_log = "none".to_string();
        // 获取认证头（提前准备，用于内联替换），同时保留仅用于日志脱敏的
        // 精确认证材料。实际日志永远不输出这些值。
        let mut log_secrets: Vec<String> = Vec::new();
        let mut auth_headers = if let Some(mut auth) = adapter.extract_auth(provider) {
            // GitHub Copilot 特殊处理：从 CopilotAuthManager 获取真实 token
            if auth.strategy == AuthStrategy::GitHubCopilot {
                if let Some(app_handle) = &self.app_handle {
                    let copilot_state = app_handle.state::<CopilotAuthState>();
                    let copilot_auth: tokio::sync::RwLockReadGuard<'_, CopilotAuthManager> =
                        copilot_state.0.read().await;

                    // 从 provider.meta 获取关联的 GitHub 账号 ID（多账号支持）
                    let account_id = provider
                        .meta
                        .as_ref()
                        .and_then(|m| m.managed_account_id_for("github_copilot"));

                    // 根据账号 ID 获取对应 token（向后兼容：无账号 ID 时使用第一个账号）
                    let token_result = match &account_id {
                        Some(id) => {
                            log::debug!("[Copilot] 使用指定账号 {id} 获取 token");
                            copilot_auth.get_valid_token_for_account(id).await
                        }
                        None => {
                            log::debug!("[Copilot] 使用默认账号获取 token");
                            copilot_auth.get_valid_token().await
                        }
                    };

                    match token_result {
                        Ok(token) => {
                            auth = AuthInfo::new(token, AuthStrategy::GitHubCopilot);
                            log::debug!(
                                "[Copilot] 成功获取 Copilot token (account={})",
                                account_id.as_deref().unwrap_or("default")
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "[Copilot] 获取 Copilot token 失败 (account={}): {e}",
                                account_id.as_deref().unwrap_or("default")
                            );
                            return Err(ProxyError::AuthError(format!(
                                "GitHub Copilot 认证失败: {e}"
                            )));
                        }
                    }
                } else {
                    log::error!("[Copilot] AppHandle 不可用");
                    return Err(ProxyError::AuthError(
                        "GitHub Copilot 认证不可用（无 AppHandle）".to_string(),
                    ));
                }
            }

            // Codex OAuth 特殊处理：从 CodexOAuthManager 获取真实 access_token
            if auth.strategy == AuthStrategy::CodexOAuth {
                if let Some(app_handle) = &self.app_handle {
                    let codex_state = app_handle.state::<CodexOAuthState>();
                    let codex_auth: tokio::sync::RwLockReadGuard<'_, CodexOAuthManager> =
                        codex_state.0.read().await;

                    // 从 provider.meta 获取关联的 ChatGPT 账号 ID
                    let account_id = provider
                        .meta
                        .as_ref()
                        .and_then(|m| m.managed_account_id_for("codex_oauth"));

                    let token_result = match &account_id {
                        Some(id) => {
                            log::debug!("[CodexOAuth] 使用指定账号 {id} 获取 token");
                            codex_auth.get_valid_token_for_account(id).await
                        }
                        None => {
                            log::debug!("[CodexOAuth] 使用默认账号获取 token");
                            codex_auth.get_valid_token().await
                        }
                    };

                    match token_result {
                        Ok(token) => {
                            auth = AuthInfo::new(token, AuthStrategy::CodexOAuth);
                            is_codex_oauth = true;
                            // 解析使用的 account_id（用于注入 ChatGPT-Account-Id header）
                            codex_oauth_account_id = match account_id {
                                Some(id) => Some(id),
                                None => codex_auth.default_account_id().await,
                            };
                            log::debug!(
                                "[CodexOAuth] 成功获取 access_token (account={})",
                                codex_oauth_account_id.as_deref().unwrap_or("default")
                            );
                        }
                        Err(e) => {
                            log::error!("[CodexOAuth] 获取 access_token 失败: {e}");
                            return Err(ProxyError::AuthError(format!(
                                "Codex OAuth 认证失败: {e}"
                            )));
                        }
                    }
                } else {
                    log::error!("[CodexOAuth] AppHandle 不可用");
                    return Err(ProxyError::AuthError(
                        "Codex OAuth 认证不可用（无 AppHandle）".to_string(),
                    ));
                }
            }

            // xAI OAuth: resolve a managed account token immediately before
            // sending the request. Invalid refresh credentials are persisted as
            // requiring re-authentication by the manager.
            if auth.strategy == AuthStrategy::XaiOAuth {
                if let Some(app_handle) = &self.app_handle {
                    let xai_state = app_handle.state::<XaiOAuthState>();
                    let xai_auth: tokio::sync::RwLockReadGuard<'_, XaiOAuthManager> =
                        xai_state.0.read().await;
                    let account_id = provider
                        .meta
                        .as_ref()
                        .and_then(|meta| meta.managed_account_id_for("xai_oauth"));
                    let token_result = match &account_id {
                        Some(id) => xai_auth.get_valid_token_for_account(id).await,
                        None => xai_auth.get_valid_token().await,
                    };
                    match token_result {
                        Ok(token) => {
                            auth = AuthInfo::new(token, AuthStrategy::XaiOAuth);
                            log::debug!(
                                "[XaiOAuth] 成功获取 access_token (account={})",
                                account_id.as_deref().unwrap_or("default")
                            );
                        }
                        Err(error) => {
                            log::error!("[XaiOAuth] 获取 access_token 失败: {error}");
                            return Err(ProxyError::AuthError(format!(
                                "xAI OAuth 认证失败: {error}"
                            )));
                        }
                    }
                } else {
                    return Err(ProxyError::AuthError(
                        "xAI OAuth 认证不可用（无 AppHandle）".to_string(),
                    ));
                }
            }

            auth_strategy_for_log = format!("{:?}", auth.strategy);
            for secret in std::iter::once(&auth.api_key).chain(auth.access_token.iter()) {
                if !secret.is_empty() && !log_secrets.contains(secret) {
                    log_secrets.push(secret.clone());
                }
            }
            adapter.get_auth_headers(&auth)?
        } else {
            Vec::new()
        };

        // 注入 Codex OAuth 的 ChatGPT-Account-Id header（如果有 account_id）
        if let Some(ref account_id) = codex_oauth_account_id {
            if let Ok(hv) = http::HeaderValue::from_str(account_id) {
                auth_headers.push((http::HeaderName::from_static("chatgpt-account-id"), hv));
            }
        }

        let codex_oauth_session_headers = if is_codex_oauth && self.session_client_provided {
            build_codex_oauth_session_headers(&self.session_id)
        } else {
            Vec::new()
        };

        if let Some(trace_id) = codex_trace_id.as_deref() {
            super::codex_router_log::append_event(
                "auth_prepared",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("model", request_model_for_log.clone()),
                    ("provider", provider.id.clone()),
                    ("auth_strategy", auth_strategy_for_log.clone()),
                    ("auth_header_count", auth_headers.len().to_string()),
                    (
                        "oauth_session_header_count",
                        codex_oauth_session_headers.len().to_string(),
                    ),
                    (
                        "elapsed_ms",
                        auth_started_at.elapsed().as_millis().to_string(),
                    ),
                ],
            );
        }

        // 自定义 User-Agent：与 stream_check / model_fetch 共用 parse_custom_user_agent，
        // 运行时静默忽略非法值（前端在输入处给非阻断提示，不在保存时阻断）。
        // Copilot 指纹 UA 不可覆盖。
        let custom_user_agent = if is_copilot {
            None
        } else {
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.custom_user_agent_header().ok().flatten())
        };
        // Codex→Anthropic emulation: when there is no custom UA, override Codex's
        // codex_cli_rs UA with the Claude Code UA.
        let custom_user_agent = if custom_user_agent.is_none() && codex_impersonate_claude_code {
            Some(http::HeaderValue::from_static(CLAUDE_CODE_USER_AGENT))
        } else {
            custom_user_agent
        };

        // --- Copilot 优化器：动态 header 注入 ---
        if let Some((ref classification, ref det_request_id, ref interaction_id)) =
            copilot_optimization
        {
            for (name, value) in auth_headers.iter_mut() {
                match name.as_str() {
                    "x-initiator" if self.copilot_optimizer_config.request_classification => {
                        *value = http::HeaderValue::from_static(classification.initiator);
                    }
                    "x-interaction-type" if classification.is_subagent => {
                        // 子代理请求：conversation-subagent 不计 premium interaction
                        *value = http::HeaderValue::from_static("conversation-subagent");
                    }
                    "x-request-id" | "x-agent-task-id" => {
                        if let Some(ref det_id) = det_request_id {
                            if let Ok(hv) = http::HeaderValue::from_str(det_id) {
                                *value = hv;
                            }
                        }
                    }
                    _ => {}
                }
            }

            // x-interaction-id：仅在有 session 时注入（不在 get_auth_headers 中）
            if let Some(ref iid) = interaction_id {
                if let Ok(hv) = http::HeaderValue::from_str(iid) {
                    auth_headers.push((http::HeaderName::from_static("x-interaction-id"), hv));
                }
            }

            if classification.is_subagent {
                log::info!(
                    "[Copilot] 子代理请求: x-initiator=agent, x-interaction-type=conversation-subagent"
                );
            }
        }

        // Copilot 指纹头名（由 get_auth_headers 注入，需在原始头中去重）
        let copilot_fingerprint_headers: &[&str] = if is_copilot {
            &[
                "user-agent",
                "editor-version",
                "editor-plugin-version",
                "copilot-integration-id",
                "x-github-api-version",
                "openai-intent",
                // 新增 headers
                "x-initiator",
                "x-interaction-type",
                "x-interaction-id",
                "x-vscode-user-agent-library-version",
                "x-request-id",
                "x-agent-task-id",
            ]
        } else {
            &[]
        };

        // 预计算上游 host 值（用于在原位替换 host header）
        let upstream_host = url
            .parse::<http::Uri>()
            .ok()
            .and_then(|u| u.authority().map(|a| a.to_string()));

        let should_send_anthropic_headers = adapter.name() == "Claude"
            && matches!(resolved_claude_api_format.as_deref(), Some("anthropic"));

        // 预计算 anthropic-beta 值（仅 Claude）
        let anthropic_beta_value = if should_send_anthropic_headers {
            const CLAUDE_CODE_BETA: &str = "claude-code-20250219";
            Some(if let Some(beta) = headers.get("anthropic-beta") {
                if let Ok(beta_str) = beta.to_str() {
                    if beta_str.contains(CLAUDE_CODE_BETA) {
                        beta_str.to_string()
                    } else {
                        format!("{CLAUDE_CODE_BETA},{beta_str}")
                    }
                } else {
                    CLAUDE_CODE_BETA.to_string()
                }
            } else {
                CLAUDE_CODE_BETA.to_string()
            })
        } else if codex_impersonate_claude_code || codex_anthropic_one_m {
            // Codex→Anthropic: emulation injects the claude-code marker; a [1m]
            // model injects the context-1m marker.
            let mut betas: Vec<&str> = Vec::new();
            if codex_impersonate_claude_code {
                betas.push("claude-code-20250219");
            }
            if codex_anthropic_one_m {
                betas.push("context-1m-2025-08-07");
            }
            Some(betas.join(","))
        } else {
            None
        };

        // ============================================================
        // 构建有序 HeaderMap — 内联替换，保持客户端原始顺序
        // ============================================================
        let mut ordered_headers = http::HeaderMap::new();
        let mut saw_auth = false;
        let mut saw_accept_encoding = false;
        let mut saw_accept = false;
        let mut saw_user_agent = false;
        let mut saw_anthropic_beta = false;
        let mut saw_anthropic_version = false;

        for (key, value) in headers {
            let key_str = key.as_str();

            if outbound_header_is_local_only(key) {
                continue;
            }

            // --- host — 原位替换为上游 host（保持客户端原始位置） ---
            if key_str.eq_ignore_ascii_case("host") {
                if let Some(ref host_val) = upstream_host {
                    if let Ok(hv) = http::HeaderValue::from_str(host_val) {
                        ordered_headers.append(key.clone(), hv);
                    }
                }
                continue;
            }

            // --- 连接 / 追踪 / CDN 类 — 无条件跳过 ---
            if matches!(
                key_str,
                "content-length"
                    | "transfer-encoding"
                    | "x-forwarded-host"
                    | "x-forwarded-port"
                    | "x-forwarded-proto"
                    | "forwarded"
                    | "cf-connecting-ip"
                    | "cf-ipcountry"
                    | "cf-ray"
                    | "cf-visitor"
                    | "true-client-ip"
                    | "fastly-client-ip"
                    | "x-azure-clientip"
                    | "x-azure-fdid"
                    | "x-azure-ref"
                    | "akamai-origin-hop"
                    | "x-akamai-config-log-detail"
                    | "x-request-id"
                    | "x-correlation-id"
                    | "x-trace-id"
                    | "x-amzn-trace-id"
                    | "x-b3-traceid"
                    | "x-b3-spanid"
                    | "x-b3-parentspanid"
                    | "x-b3-sampled"
                    | "traceparent"
                    | "tracestate"
            ) {
                continue;
            }

            // --- 认证类 — 用 adapter 提供的认证头替换（在原始位置） ---
            if key_str.eq_ignore_ascii_case("authorization")
                || key_str.eq_ignore_ascii_case("x-api-key")
                || key_str.eq_ignore_ascii_case("x-goog-api-key")
            {
                // The built-in Codex official provider deliberately has no
                // credential in CC Switch. `requires_openai_auth = true` makes
                // Codex send its native ChatGPT authorization, which must reach
                // the fixed official upstream unchanged. Other credential
                // headers are still discarded.
                if codex_official_auth_passthrough && key_str.eq_ignore_ascii_case("authorization")
                {
                    saw_auth = true;
                    ordered_headers.append(key.clone(), value.clone());
                    continue;
                }
                if !saw_auth {
                    saw_auth = true;
                    for (ah_name, ah_value) in &auth_headers {
                        ordered_headers.append(ah_name.clone(), ah_value.clone());
                    }
                }
                continue;
            }

            // --- x-app — during Codex→Anthropic emulation, `cli` is injected uniformly below ---
            if codex_impersonate_claude_code && key_str.eq_ignore_ascii_case("x-app") {
                continue;
            }

            // --- Codex/OpenAI fingerprint headers — never leak to an Anthropic upstream ---
            // These are client/session identifiers from the incoming Codex request,
            // not Anthropic protocol headers. Forwarding them both leaks identity and
            // can defeat strict gateway fingerprint checks.
            // The full set lives in `is_codex_client_fingerprint_header` so it stays in one
            // place. (HeaderName is lowercased by the http crate, so a direct match is safe.)
            if codex_responses_to_anthropic && is_codex_client_fingerprint_header(key_str) {
                continue;
            }

            // --- accept — force application/json on the Codex→Anthropic path ---
            // The Codex CLI sends `Accept: text/event-stream`, whereas a native
            // Anthropic client sends `application/json` (streaming is driven by
            // the body's stream:true). Strict Anthropic gateways return 406 Not
            // Acceptable for an event-stream Accept, so normalize it here.
            if codex_responses_to_anthropic && key_str.eq_ignore_ascii_case("accept") {
                if !saw_accept {
                    saw_accept = true;
                    ordered_headers.append(
                        http::header::ACCEPT,
                        http::HeaderValue::from_static("application/json"),
                    );
                }
                continue;
            }

            // --- accept-encoding — transform / SSE 路径强制 identity，其余保留原值 ---
            if key_str.eq_ignore_ascii_case("accept-encoding") {
                if !saw_accept_encoding {
                    saw_accept_encoding = true;
                    if force_identity_encoding {
                        ordered_headers.append(
                            http::header::ACCEPT_ENCODING,
                            http::HeaderValue::from_static("identity"),
                        );
                    } else {
                        ordered_headers.append(key.clone(), value.clone());
                    }
                }
                continue;
            }

            // --- user-agent: provider-level override for local proxy routing ---
            if !is_copilot && key_str.eq_ignore_ascii_case("user-agent") {
                if !saw_user_agent {
                    saw_user_agent = true;
                    if let Some(ref ua) = custom_user_agent {
                        ordered_headers.append(http::header::USER_AGENT, ua.clone());
                    } else {
                        ordered_headers.append(key.clone(), value.clone());
                    }
                }
                continue;
            }

            // --- anthropic-beta — 用重建值替换（确保含 claude-code 标记） ---
            if key_str.eq_ignore_ascii_case("anthropic-beta") {
                if !saw_anthropic_beta {
                    saw_anthropic_beta = true;
                    if let Some(ref beta_val) = anthropic_beta_value {
                        if let Ok(hv) = http::HeaderValue::from_str(beta_val) {
                            ordered_headers.append("anthropic-beta", hv);
                        }
                    }
                }
                continue;
            }

            // --- anthropic-version — 透传客户端值 ---
            if key_str.eq_ignore_ascii_case("anthropic-version") {
                if should_send_anthropic_headers {
                    saw_anthropic_version = true;
                    ordered_headers.append(key.clone(), value.clone());
                }
                continue;
            }

            // --- Copilot 指纹头 — 跳过（由 auth_headers 提供） ---
            if copilot_fingerprint_headers
                .iter()
                .any(|h| key_str.eq_ignore_ascii_case(h))
            {
                continue;
            }

            // --- 默认：透传 ---
            ordered_headers.append(key.clone(), value.clone());
        }

        // 如果原始请求中没有认证头，在末尾追加
        if !saw_auth && !auth_headers.is_empty() {
            for (ah_name, ah_value) in &auth_headers {
                ordered_headers.append(ah_name.clone(), ah_value.clone());
            }
        }

        // transform / SSE 路径在缺失时补 identity；普通透传不主动补 accept-encoding
        if !saw_accept_encoding && force_identity_encoding {
            ordered_headers.append(
                http::header::ACCEPT_ENCODING,
                http::HeaderValue::from_static("identity"),
            );
        }

        // On the Codex→Anthropic path, add application/json when Accept is missing (matching a native Anthropic client).
        if codex_responses_to_anthropic && !saw_accept {
            ordered_headers.append(
                http::header::ACCEPT,
                http::HeaderValue::from_static("application/json"),
            );
        }

        // Codex→Anthropic emulation: inject Claude Code's x-app: cli
        if codex_impersonate_claude_code {
            ordered_headers.append("x-app", http::HeaderValue::from_static("cli"));
        }

        if !saw_user_agent {
            if let Some(ref ua) = custom_user_agent {
                ordered_headers.append(http::header::USER_AGENT, ua.clone());
            }
        }

        // 如果原始请求中没有 anthropic-beta 且有值需要添加，追加
        if !saw_anthropic_beta {
            if let Some(ref beta_val) = anthropic_beta_value {
                if let Ok(hv) = http::HeaderValue::from_str(beta_val) {
                    ordered_headers.append("anthropic-beta", hv);
                }
            }
        }

        // anthropic-version: add the default only when it is missing.
        // The Codex→Anthropic path also needs this header. Note this is independent
        // of anthropic-beta: the Claude Code-specific beta is only sent when
        // impersonation is on (handled above); on the plain Codex→Anthropic path
        // (impersonation off) anthropic-version is still required but no beta is sent.
        if (should_send_anthropic_headers || codex_responses_to_anthropic) && !saw_anthropic_version
        {
            ordered_headers.append(
                "anthropic-version",
                http::HeaderValue::from_static("2023-06-01"),
            );
        }

        // Codex OAuth 反代尽量对齐官方 Codex CLI 的会话路由信号。
        // 只发送客户端提供的 session_id；生成的 UUID 每次不同，反而会破坏前缀缓存。
        for (name, value) in codex_oauth_session_headers {
            if !ordered_headers.contains_key(&name) {
                ordered_headers.insert(name, value);
            }
        }

        // 序列化请求体。GET/HEAD 是 idempotent/safe 方法，按 HTTP 语义不应携带 body；
        // 强行附带 JSON body 会让某些上游（如 Google Gemini 的 models.list）拒绝请求。
        let mut body_bytes = if matches!(method, &http::Method::GET | &http::Method::HEAD) {
            Vec::new()
        } else {
            serde_json::to_vec(&filtered_body).map_err(|e| {
                ProxyError::Internal(format!("Failed to serialize request body: {e}"))
            })?
        };
        // 确保 content-type 存在
        if !ordered_headers.contains_key(http::header::CONTENT_TYPE) {
            ordered_headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
        }

        apply_local_proxy_header_overrides(
            &mut ordered_headers,
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref()),
            is_copilot,
        );
        enforce_codex_oauth_originator(
            &mut ordered_headers,
            is_codex_oauth || codex_official_auth_passthrough,
            self.preserve_codex_client_originator,
        );

        reject_proxy_placeholder_for_managed_account_upstream(&url, &ordered_headers)?;

        let responses_lite_request_model = filtered_body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("<none>")
            .to_string();
        let responses_lite_fallback_key =
            codex_responses_lite_fallback_key(&provider.id, &url, &responses_lite_request_model);
        let responses_lite_fallback_cached = codex_responses_lite_requested
            && self
                .codex_responses_lite_fallback_active(&responses_lite_fallback_key)
                .await;
        if responses_lite_fallback_cached {
            filtered_body =
                super::providers::openai_compat::normalize_codex_responses_lite_fallback_request(
                    filtered_body,
                );
            body_bytes = serde_json::to_vec(&filtered_body).map_err(|error| {
                ProxyError::Internal(format!(
                    "Failed to serialize cached Responses-Lite fallback body: {error}"
                ))
            })?;
        }

        let zstd_compress_codex_official_upstream = should_zstd_compress_codex_official_upstream(
            app_type,
            method,
            &url,
            is_codex_oauth || codex_official_auth_passthrough,
            needs_transform
                || codex_responses_to_chat
                || codex_responses_to_messages
                || codex_responses_to_anthropic,
        );
        body_bytes = encode_codex_official_upstream_body(
            &mut ordered_headers,
            body_bytes,
            zstd_compress_codex_official_upstream,
        )?;
        let request_bytes_len = body_bytes.len();

        // 日志目标 URL 的脱敏分两种情形：
        // - 有已知密钥(log_secrets 非空)：记录脱敏后的完整 URL，剥 userinfo/query
        //   并抹掉已知密钥值，保留 host+path 便于诊断 base_url 配错路径导致的 404。
        // - 无已知密钥：凭据可能整个内嵌在 path 里且无从脱敏，只记 origin，
        //   避免默认 Info 级把形如 https://gw/<KEY>/v1 的 path 完整落盘。
        let target_for_log = if log_secrets.is_empty() {
            crate::redact_url_origin_for_log(&url)
        } else {
            crate::redact_url_for_log_with_secrets(&url, &log_secrets)
        };

        // 输出请求信息日志
        let tag = adapter.name();
        let request_model = filtered_body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("<none>");
        if responses_lite_fallback_cached {
            ordered_headers.remove(http::HeaderName::from_static(
                "x-openai-internal-codex-responses-lite",
            ));
            log::info!(
                "[{tag}] 命中 Codex Responses-Lite fallback 缓存，按 provider/url/model 直接去头发送 (model={request_model})"
            );
            if let Some(trace_id) = codex_trace_id.as_deref() {
                super::codex_router_log::append_event(
                    "responses_lite_fallback_cache_hit",
                    &[
                        ("trace", trace_id.to_string()),
                        ("session", self.session_id.clone()),
                        ("model", request_model.to_string()),
                        ("provider", provider.id.clone()),
                    ],
                );
            }
        }
        log::info!("[{tag}] >>> 请求目标: {target_for_log} (model={request_model})");
        log::debug!(
            "[{tag}] >>> 请求体已准备: bytes={}, hash={} (content omitted)",
            request_bytes_len,
            short_value_hash(Some(&filtered_body))
        );

        // 确定超时
        let timeout = if self.non_streaming_timeout.is_zero() {
            std::time::Duration::from_secs(600) // 默认 600 秒
        } else {
            self.non_streaming_timeout
        };

        // 获取全局代理 URL
        let upstream_proxy_url: Option<String> = super::http_client::get_current_proxy_url();

        // SOCKS5 代理不支持 CONNECT 隧道，需要用 reqwest
        let is_socks_proxy = upstream_proxy_url
            .as_deref()
            .map(|u| u.starts_with("socks5"))
            .unwrap_or(false);

        let preserve_exact_header_case = should_preserve_exact_header_case(
            adapter.name(),
            provider,
            resolved_claude_api_format.as_deref(),
            is_copilot,
        );
        let transport_for_log = if is_socks_proxy || !preserve_exact_header_case {
            "reqwest"
        } else {
            "hyper"
        };

        // 流式 Responses 请求提交给下游后，上游仍可能中途掐断 SSE。
        // 在 headers/body 被移动之前捕获一份重放工厂，交给下游转换层做
        // 有界重连（见 providers::streaming_retry）。工厂只是重放同一
        // HTTP 请求；是否重试、重试多少次由转换层根据下游状态决定。
        let stream_reconnect = if should_create_responses_stream_reconnector(
            app_type,
            endpoint,
            request_is_streaming,
            resolved_claude_api_format.as_deref(),
        ) {
            let first_byte_timeout = if self.streaming_first_byte_timeout.is_zero() {
                timeout
            } else {
                self.streaming_first_byte_timeout
            };
            let connect: super::providers::streaming_retry::ConnectFn =
                if is_socks_proxy || !preserve_exact_header_case {
                    let url = url.clone();
                    let method = method.clone();
                    let headers = ordered_headers.clone();
                    let body = body_bytes.clone();
                    Box::new(move || {
                        let url = url.clone();
                        let method = method.clone();
                        let headers = headers.clone();
                        let body = body.clone();
                        Box::pin(async move {
                            let client = super::http_client::get();
                            let mut request = client
                                .request(method, &url)
                                .timeout(std::time::Duration::from_secs(24 * 60 * 60));
                            for (key, value) in &headers {
                                request = request.header(key, value);
                            }
                            let response = request
                                .body(body)
                                .send()
                                .await
                                .map_err(map_reqwest_send_error)?;
                            Ok(ProxyResponse::Reqwest(response))
                        })
                    })
                } else {
                    let url = url.clone();
                    let target_for_log = target_for_log.clone();
                    let method = method.clone();
                    let headers = ordered_headers.clone();
                    let extensions = extensions.clone();
                    let body = body_bytes.clone();
                    let upstream_proxy_url = upstream_proxy_url.clone();
                    Box::new(move || {
                        let url = url.clone();
                        let target_for_log = target_for_log.clone();
                        let method = method.clone();
                        let headers = headers.clone();
                        let extensions = extensions.clone();
                        let body = body.clone();
                        let upstream_proxy_url = upstream_proxy_url.clone();
                        Box::pin(async move {
                            let uri: http::Uri = url.parse().map_err(|e| {
                                ProxyError::ForwardFailed(format!(
                                    "Invalid upstream URL ({target_for_log}): {e}"
                                ))
                            })?;
                            super::hyper_client::send_request(
                                uri,
                                &target_for_log,
                                method,
                                headers,
                                extensions,
                                body,
                                timeout,
                                upstream_proxy_url.as_deref(),
                            )
                            .await
                        })
                    })
                };
            Some(StreamReconnector::new(connect, first_byte_timeout))
        } else {
            None
        };

        let upstream_started_at = std::time::Instant::now();
        if let Some(trace_id) = codex_trace_id.as_deref() {
            super::codex_router_log::append_event(
                "upstream_send",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("model", request_model_for_log.clone()),
                    ("provider", provider.id.clone()),
                    ("transport", transport_for_log.to_string()),
                    ("request_bytes", request_bytes_len.to_string()),
                    ("header_count", ordered_headers.len().to_string()),
                    ("streaming", request_is_streaming.to_string()),
                    ("timeout_ms", timeout.as_millis().to_string()),
                    (
                        "uses_upstream_proxy",
                        upstream_proxy_url.is_some().to_string(),
                    ),
                ],
            );
        }

        // 发送请求。默认保留 Codex Responses-Lite 协商头；只有上游明确返回
        // Lite 不支持错误时，才在错误响应体读取后剥头重发一次。
        let send_upstream_request = |headers: http::HeaderMap, body_bytes: Vec<u8>| {
            let method = method.clone();
            let url = url.clone();
            let extensions = extensions.clone();
            let upstream_proxy_url = upstream_proxy_url.clone();
            let target_for_log = target_for_log.clone();
            async move {
                send_forwarder_upstream_request(
                    method,
                    url,
                    target_for_log,
                    headers,
                    extensions,
                    body_bytes,
                    timeout,
                    request_is_streaming,
                    self.non_streaming_timeout,
                    self.streaming_first_byte_timeout,
                    is_socks_proxy,
                    preserve_exact_header_case,
                    upstream_proxy_url.as_deref(),
                )
                .await
            }
        };

        let send_once = || {
            send_upstream_request_with_transport_retry(adapter.name(), || {
                send_upstream_request(ordered_headers.clone(), body_bytes.clone())
            })
        };
        let mut response = if matches!(app_type, AppType::Codex) {
            send_codex_request_with_rate_limit_retry(adapter.name(), send_once).await
        } else {
            send_once().await
        }
        .inspect_err(|err| {
            if let Some(trace_id) = codex_trace_id.as_deref() {
                let transport = if is_socks_proxy || !preserve_exact_header_case {
                    "reqwest"
                } else {
                    "hyper"
                };
                super::codex_router_log::append_event(
                    "upstream_send_error",
                    &[
                        ("trace", trace_id.to_string()),
                        ("session", self.session_id.clone()),
                        ("model", request_model_for_log.clone()),
                        ("provider", provider.id.clone()),
                        ("transport", transport.to_string()),
                        (
                            "elapsed_ms",
                            upstream_started_at.elapsed().as_millis().to_string(),
                        ),
                        ("error", err.to_string()),
                    ],
                );
            }
        })?;

        // 检查响应状态
        let mut status = response.status();
        let upstream_elapsed_ms = upstream_started_at.elapsed().as_millis().to_string();
        if let Some(trace_id) = codex_trace_id.as_deref() {
            super::codex_router_log::append_event(
                "upstream_status",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("model", request_model_for_log.clone()),
                    ("provider", provider.id.clone()),
                    ("status", status.as_u16().to_string()),
                    ("streaming", request_is_streaming.to_string()),
                    ("elapsed_ms", upstream_elapsed_ms.clone()),
                ],
            );
        }

        if !status.is_success() {
            let status_code = status.as_u16();
            let body_text = read_decoded_error_body(response).await?;
            if let Some(trace_id) = codex_trace_id.as_deref() {
                append_upstream_error_event(
                    trace_id,
                    &self.session_id,
                    &request_model_for_log,
                    &provider.id,
                    status_code,
                    body_text.as_deref(),
                    codex_chat_request_shape.as_deref(),
                );
            }

            if should_retry_without_codex_responses_lite_header(
                app_type,
                &ordered_headers,
                status_code,
                body_text.as_deref(),
            ) {
                self.mark_codex_responses_lite_fallback(responses_lite_fallback_key.clone())
                    .await;
                let mut retry_headers = ordered_headers.clone();
                retry_headers.remove(http::HeaderName::from_static(
                    "x-openai-internal-codex-responses-lite",
                ));
                log::warn!(
                    "[{tag}] 上游拒绝 Codex Responses-Lite，剥离内部协商头后重试一次 (model={request_model})"
                );
                if let Some(trace_id) = codex_trace_id.as_deref() {
                    super::codex_router_log::append_event(
                        "upstream_retry_without_responses_lite",
                        &[
                            ("trace", trace_id.to_string()),
                            ("session", self.session_id.clone()),
                            ("model", request_model_for_log.clone()),
                            ("provider", provider.id.clone()),
                            ("status", status_code.to_string()),
                            (
                                "body_summary",
                                body_text
                                    .as_deref()
                                    .map(summarize_upstream_body)
                                    .unwrap_or_else(|| "<empty>".to_string()),
                            ),
                        ],
                    );
                }
                let retry_body =
                    super::providers::openai_compat::normalize_codex_responses_lite_fallback_request(
                        filtered_body.clone(),
                    );
                let retry_body_bytes = serde_json::to_vec(&retry_body).map_err(|error| {
                    ProxyError::Internal(format!(
                        "Failed to serialize Responses-Lite fallback body: {error}"
                    ))
                })?;
                let retry_body_bytes = encode_codex_official_upstream_body(
                    &mut retry_headers,
                    retry_body_bytes,
                    zstd_compress_codex_official_upstream,
                )?;
                response = send_upstream_request(retry_headers, retry_body_bytes)
                    .await
                    .inspect_err(|err| {
                        if let Some(trace_id) = codex_trace_id.as_deref() {
                            let transport = if is_socks_proxy || !preserve_exact_header_case {
                                "reqwest"
                            } else {
                                "hyper"
                            };
                            super::codex_router_log::append_event(
                                "upstream_send_error",
                                &[
                                    ("trace", trace_id.to_string()),
                                    ("session", self.session_id.clone()),
                                    ("model", request_model_for_log.clone()),
                                    ("provider", provider.id.clone()),
                                    ("transport", transport.to_string()),
                                    (
                                        "elapsed_ms",
                                        upstream_started_at.elapsed().as_millis().to_string(),
                                    ),
                                    ("error", err.to_string()),
                                ],
                            );
                        }
                    })?;
                status = response.status();
                if let Some(trace_id) = codex_trace_id.as_deref() {
                    super::codex_router_log::append_event(
                        "upstream_status",
                        &[
                            ("trace", trace_id.to_string()),
                            ("session", self.session_id.clone()),
                            ("model", request_model_for_log.clone()),
                            ("provider", provider.id.clone()),
                            ("status", status.as_u16().to_string()),
                            ("streaming", request_is_streaming.to_string()),
                            (
                                "elapsed_ms",
                                upstream_started_at.elapsed().as_millis().to_string(),
                            ),
                            ("retry", "without_responses_lite".to_string()),
                        ],
                    );
                }
            } else {
                return Err(ProxyError::UpstreamError {
                    status: status_code,
                    body: body_text,
                });
            }
        }

        if status.is_success() {
            let response_prepare_started_at = std::time::Instant::now();
            let mut response = self
                .prepare_success_response_for_failover(response, request_is_streaming)
                .await?;
            if codex_responses_to_anthropic && (!request_is_streaming || response.is_json()) {
                response = self
                    .validate_codex_anthropic_success_response(response)
                    .await?;
            } else if matches!(
                resolved_claude_api_format.as_deref(),
                Some("openai_responses")
            ) {
                if !request_is_streaming || response.is_json() {
                    response = self.validate_responses_success_response(response).await?;
                } else {
                    response = self.validate_responses_stream_start(response).await?;
                }
            }
            let response = if let Some(config) = hosted_tool_loop_config.as_ref() {
                let hosted_tool_client =
                    resolve_hosted_tool_client(self.app_handle.as_ref(), provider, headers).await;
                run_hosted_tool_chat_loop(
                    response,
                    &mut filtered_body,
                    config,
                    &hosted_tool_client,
                    |body| {
                        let headers = ordered_headers.clone();
                        let body_bytes = serde_json::to_vec(body).map_err(|e| {
                            ProxyError::Internal(format!(
                                "Failed to serialize hosted tool loop request body: {e}"
                            ))
                        });
                        let trace_id = codex_trace_id.clone();
                        let session_id = self.session_id.clone();
                        let model = request_model_for_log.clone();
                        let provider_id = provider.id.clone();
                        async move {
                            let body_bytes = body_bytes?;
                            let body_len = body_bytes.len();
                            if let Some(trace_id) = trace_id.as_deref() {
                                super::codex_router_log::append_event(
                                    "hosted_tool_loop_upstream_send",
                                    &[
                                        ("trace", trace_id.to_string()),
                                        ("session", session_id.clone()),
                                        ("model", model.clone()),
                                        ("provider", provider_id.clone()),
                                        ("request_bytes", body_len.to_string()),
                                    ],
                                );
                            }
                            let send = send_upstream_request(headers, body_bytes);
                            send.await
                        }
                    },
                    codex_trace_id.as_deref(),
                    hosted_tools_forced_non_stream,
                )
                .await?
            } else {
                response
            };
            if let Some(trace_id) = codex_trace_id.as_deref() {
                super::codex_router_log::append_event(
                    "response_ready",
                    &[
                        ("trace", trace_id.to_string()),
                        ("session", self.session_id.clone()),
                        ("model", request_model_for_log.clone()),
                        ("provider", provider.id.clone()),
                        ("status", status.as_u16().to_string()),
                        ("streaming", request_is_streaming.to_string()),
                        ("upstream_stream", upstream_stream.to_string()),
                        (
                            "elapsed_ms",
                            response_prepare_started_at
                                .elapsed()
                                .as_millis()
                                .to_string(),
                        ),
                    ],
                );
            }
            Ok((
                response,
                resolved_claude_api_format,
                provider.clone(),
                outbound_model,
                stream_reconnect,
            ))
        } else {
            let status_code = status.as_u16();
            let body_text = read_decoded_error_body(response).await?;
            if let Some(trace_id) = codex_trace_id.as_deref() {
                append_upstream_error_event(
                    trace_id,
                    &self.session_id,
                    &request_model_for_log,
                    &provider.id,
                    status_code,
                    body_text.as_deref(),
                    codex_chat_request_shape.as_deref(),
                );
            }

            Err(ProxyError::UpstreamError {
                status: status_code,
                body: body_text,
            })
        }
    }

    /// 转发单个未知 OpenAI-compatible 请求，保持原始请求体不变。
    ///
    /// `route_body` 只参与日志和 MultiRouter 路由；`raw_body` 是最终上游载荷。
    /// 该函数不做 JSON 序列化、不注入默认 content-type，也不执行 Responses/Chat
    /// 转换，专门用于未来 `/v1/*` endpoint 的通用兜底。
    #[allow(clippy::too_many_arguments)]
    async fn forward_raw(
        &self,
        app_type: &AppType,
        method: &http::Method,
        provider: &Provider,
        endpoint: &str,
        route_body: &Value,
        raw_body: Bytes,
        headers: &axum::http::HeaderMap,
        extensions: &Extensions,
        adapter: &dyn ProviderAdapter,
    ) -> Result<(ProxyResponse, Provider, Option<String>), ProxyError> {
        let codex_trace_id =
            matches!(app_type, AppType::Codex).then(|| uuid::Uuid::new_v4().to_string());
        let route_started_at = std::time::Instant::now();
        let request_model_for_log = route_body
            .get("model")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        let (outer_provider_id, outer_provider_name) = {
            let (id, name) = super::providers::codex_route_persistent_provider(provider);
            (id.to_string(), name.to_string())
        };

        let provider_is_resolved_codex_route = provider
            .settings_config
            .get("codexResolvedRouteId")
            .is_some();
        let codex_router_configured =
            matches!(app_type, AppType::Codex) && codex_provider_has_routing_config(provider);
        let routed_provider =
            if matches!(app_type, AppType::Codex) && !provider_is_resolved_codex_route {
                resolve_codex_raw_passthrough_route_provider(provider, route_body)
            } else {
                None
            };
        let routed_provider = if let Some(route_provider) = routed_provider {
            if let Some(target_provider_id) =
                super::providers::codex_route_target_provider_id(&route_provider)
            {
                let Some(target_provider) = self
                    .router
                    .get_provider_by_id(target_provider_id, app_type.as_str())
                    .map_err(|err| {
                        ProxyError::ConfigError(format!(
                            "读取 Codex raw route 目标供应商 '{target_provider_id}' 失败: {err}"
                        ))
                    })?
                else {
                    return Err(ProxyError::ConfigError(format!(
                        "Codex raw route 引用了不存在的目标供应商 '{target_provider_id}'"
                    )));
                };
                Some(
                    super::providers::materialize_codex_routed_provider_from_target(
                        &route_provider,
                        &target_provider,
                    ),
                )
            } else {
                Some(route_provider)
            }
        } else {
            None
        };
        let codex_route_missed = codex_router_configured
            && !provider_is_resolved_codex_route
            && routed_provider.is_none();
        let provider = routed_provider.as_ref().unwrap_or(provider);

        if let Some(trace_id) = codex_trace_id.as_deref() {
            let route_id = provider
                .settings_config
                .get("codexResolvedRouteId")
                .and_then(|value| value.as_str())
                .unwrap_or("<none>");
            super::codex_router_log::append_event(
                "raw_route_resolved",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("endpoint", endpoint.to_string()),
                    ("model", request_model_for_log.clone()),
                    ("outer_provider", outer_provider_id.clone()),
                    ("outer_name", outer_provider_name.clone()),
                    ("effective_provider", provider.id.clone()),
                    ("effective_name", provider.name.clone()),
                    ("route_id", route_id.to_string()),
                    ("routing_configured", codex_router_configured.to_string()),
                    ("route_missed", codex_route_missed.to_string()),
                    (
                        "elapsed_ms",
                        route_started_at.elapsed().as_millis().to_string(),
                    ),
                ],
            );
        }

        let base_url = adapter.extract_base_url(provider)?;
        if let Err(error) = reject_codex_effective_local_proxy_upstream(
            app_type,
            &base_url,
            &format!("raw endpoint '{endpoint}'"),
        ) {
            if let Some(trace_id) = codex_trace_id.as_deref() {
                super::codex_router_log::append_event(
                    "raw_route_error",
                    &[
                        ("trace", trace_id.to_string()),
                        ("session", self.session_id.clone()),
                        ("endpoint", endpoint.to_string()),
                        ("model", request_model_for_log.clone()),
                        ("outer_provider", outer_provider_id.clone()),
                        ("fallback_base_url", base_url.clone()),
                        (
                            "reason",
                            "effective_upstream_local_proxy_self_loop".to_string(),
                        ),
                    ],
                );
            }
            return Err(error);
        }

        let is_full_url = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.is_full_url)
            .unwrap_or(false);
        let (endpoint_path, passthrough_query) = split_endpoint_and_query(endpoint);
        let effective_endpoint = match passthrough_query {
            Some(query) if !query.is_empty() => format!("{endpoint_path}?{query}"),
            _ => endpoint_path.to_string(),
        };
        let mut url = if is_full_url {
            append_query_to_full_url(&base_url, passthrough_query)
        } else {
            adapter.build_url(&base_url, &effective_endpoint)
        };
        let mut request_body = raw_body.clone();
        let mut realtime_content_type: Option<&'static str> = None;
        if method == http::Method::POST
            && codex_realtime_live_call_path(endpoint_path)
            && base_url.contains("/backend-api")
        {
            let content_type = headers
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_string();
            let (sdp, session) = codex_realtime_multipart_payload(&raw_body, &content_type)?;
            let mut backend_url = adapter.build_url(&base_url, "/realtime/calls");
            let separator = if backend_url.contains('?') { '&' } else { '?' };
            backend_url.push(separator);
            backend_url.push_str("intent=quicksilver&architecture=avas");
            url = backend_url.clone();
            let backend_body = serde_json::to_vec(&serde_json::json!({
                "sdp": sdp,
                "session": session,
            }))
            .map_err(|err| {
                ProxyError::Internal(format!("failed to encode Codex realtime call: {err}"))
            })?;
            request_body = Bytes::from(backend_body);
            realtime_content_type = Some("application/json");
            if let Some(trace_id) = codex_trace_id.as_deref() {
                super::codex_router_log::append_event(
                    "realtime_call_backend_translated",
                    &[
                        ("trace", trace_id.to_string()),
                        ("session", self.session_id.clone()),
                        ("provider", provider.id.clone()),
                        (
                            "upstream_url",
                            crate::redact_url_origin_for_log(&backend_url),
                        ),
                    ],
                );
            }
        }

        let auth_started_at = std::time::Instant::now();
        let mut auth_strategy_for_log = "none".to_string();
        let mut log_secrets: Vec<String> = Vec::new();
        let mut codex_oauth_account_id: Option<String> = None;
        let mut is_codex_oauth = false;
        let codex_official_auth_passthrough =
            should_passthrough_codex_official_auth(app_type, provider, headers);
        if codex_official_auth_passthrough {
            validate_codex_official_authorization(headers)?;
        }
        let mut auth_headers = if let Some(mut auth) = adapter.extract_auth(provider) {
            if auth.strategy == AuthStrategy::CodexOAuth {
                if let Some(app_handle) = &self.app_handle {
                    let codex_state = app_handle.state::<CodexOAuthState>();
                    let codex_auth: tokio::sync::RwLockReadGuard<'_, CodexOAuthManager> =
                        codex_state.0.read().await;
                    let account_id = provider
                        .meta
                        .as_ref()
                        .and_then(|m| m.managed_account_id_for("codex_oauth"));
                    let token_result = match &account_id {
                        Some(id) => codex_auth.get_valid_token_for_account(id).await,
                        None => codex_auth.get_valid_token().await,
                    };
                    match token_result {
                        Ok(token) => {
                            auth = AuthInfo::new(token, AuthStrategy::CodexOAuth);
                            is_codex_oauth = true;
                            codex_oauth_account_id = match account_id {
                                Some(id) => Some(id),
                                None => codex_auth.default_account_id().await,
                            };
                        }
                        Err(err) => {
                            return Err(ProxyError::AuthError(format!(
                                "Codex OAuth 认证失败: {err}"
                            )));
                        }
                    }
                } else {
                    return Err(ProxyError::AuthError(
                        "Codex OAuth 认证不可用（无 AppHandle）".to_string(),
                    ));
                }
            }
            auth_strategy_for_log = format!("{:?}", auth.strategy);
            for secret in std::iter::once(&auth.api_key).chain(auth.access_token.iter()) {
                if !secret.is_empty() && !log_secrets.contains(secret) {
                    log_secrets.push(secret.clone());
                }
            }
            adapter.get_auth_headers(&auth)?
        } else {
            Vec::new()
        };

        if codex_official_auth_passthrough {
            replace_with_native_codex_auth_headers(&mut auth_headers, headers);
        }

        if let Some(ref account_id) = codex_oauth_account_id {
            if let Ok(hv) = http::HeaderValue::from_str(account_id) {
                auth_headers.push((http::HeaderName::from_static("chatgpt-account-id"), hv));
            }
        }
        let codex_oauth_session_headers = if is_codex_oauth && self.session_client_provided {
            build_codex_oauth_session_headers(&self.session_id)
        } else {
            Vec::new()
        };

        if let Some(trace_id) = codex_trace_id.as_deref() {
            super::codex_router_log::append_event(
                "raw_auth_prepared",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("model", request_model_for_log.clone()),
                    ("provider", provider.id.clone()),
                    ("auth_strategy", auth_strategy_for_log.clone()),
                    ("auth_header_count", auth_headers.len().to_string()),
                    (
                        "oauth_session_header_count",
                        codex_oauth_session_headers.len().to_string(),
                    ),
                    (
                        "elapsed_ms",
                        auth_started_at.elapsed().as_millis().to_string(),
                    ),
                ],
            );
        }

        let upstream_host = url
            .parse::<http::Uri>()
            .ok()
            .and_then(|uri| uri.authority().map(|authority| authority.to_string()));
        let custom_user_agent = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.custom_user_agent_header().ok().flatten());
        let mut ordered_headers = build_raw_passthrough_headers(
            headers,
            &auth_headers,
            upstream_host.as_deref(),
            custom_user_agent.as_ref(),
        );
        for (name, value) in codex_oauth_session_headers {
            if !ordered_headers.contains_key(&name) {
                ordered_headers.insert(name, value);
            }
        }
        if let Some(content_type) = realtime_content_type {
            ordered_headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static(content_type),
            );
        }
        apply_local_proxy_header_overrides(
            &mut ordered_headers,
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref()),
            false,
        );
        enforce_codex_oauth_originator(
            &mut ordered_headers,
            is_codex_oauth || codex_official_auth_passthrough,
            self.preserve_codex_client_originator,
        );
        reject_proxy_placeholder_for_managed_account_upstream(&url, &ordered_headers)?;

        let target_for_log = if log_secrets.is_empty() {
            crate::redact_url_origin_for_log(&url)
        } else {
            crate::redact_url_for_log_with_secrets(&url, &log_secrets)
        };

        let request_is_streaming = raw_passthrough_request_is_streaming(route_body, headers);
        let timeout = if self.non_streaming_timeout.is_zero() {
            std::time::Duration::from_secs(600)
        } else {
            self.non_streaming_timeout
        };
        let upstream_proxy_url = super::http_client::get_current_proxy_url();
        let is_socks_proxy = upstream_proxy_url
            .as_deref()
            .map(|url| url.starts_with("socks5"))
            .unwrap_or(false);
        let preserve_exact_header_case =
            should_preserve_exact_header_case(adapter.name(), provider, None, false);
        let transport_for_log = if is_socks_proxy || !preserve_exact_header_case {
            "reqwest"
        } else {
            "hyper"
        };
        let body_bytes = request_body.to_vec();
        let request_bytes_len = body_bytes.len();
        let upstream_started_at = std::time::Instant::now();

        if let Some(trace_id) = codex_trace_id.as_deref() {
            super::codex_router_log::append_event(
                "raw_request_prepared",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("endpoint", endpoint.to_string()),
                    ("effective_endpoint", effective_endpoint.clone()),
                    ("model", request_model_for_log.clone()),
                    ("provider", provider.id.clone()),
                    ("upstream_url", target_for_log.clone()),
                    ("streaming", request_is_streaming.to_string()),
                    ("request_bytes", request_bytes_len.to_string()),
                    ("header_count", ordered_headers.len().to_string()),
                    ("transport", transport_for_log.to_string()),
                ],
            );
        }

        log::info!(
            "[{}] >>> raw passthrough target: {} (endpoint={}, model={})",
            adapter.name(),
            target_for_log,
            endpoint,
            request_model_for_log
        );
        let response = send_upstream_request_with_transport_retry(adapter.name(), || {
            let method = method.clone();
            let url = url.clone();
            let target_for_log = target_for_log.clone();
            let ordered_headers = ordered_headers.clone();
            let extensions = extensions.clone();
            let body_bytes = body_bytes.clone();
            let upstream_proxy_url = upstream_proxy_url.clone();
            async move {
                send_forwarder_upstream_request(
                    method,
                    url,
                    target_for_log,
                    ordered_headers,
                    extensions,
                    body_bytes,
                    timeout,
                    request_is_streaming,
                    self.non_streaming_timeout,
                    self.streaming_first_byte_timeout,
                    is_socks_proxy,
                    preserve_exact_header_case,
                    upstream_proxy_url.as_deref(),
                )
                .await
            }
        })
        .await
        .inspect_err(|err| {
            if let Some(trace_id) = codex_trace_id.as_deref() {
                super::codex_router_log::append_event(
                    "raw_upstream_send_error",
                    &[
                        ("trace", trace_id.to_string()),
                        ("session", self.session_id.clone()),
                        ("model", request_model_for_log.clone()),
                        ("provider", provider.id.clone()),
                        ("transport", transport_for_log.to_string()),
                        (
                            "elapsed_ms",
                            upstream_started_at.elapsed().as_millis().to_string(),
                        ),
                        ("error", err.to_string()),
                    ],
                );
            }
        })?;

        let status = response.status();
        if let Some(trace_id) = codex_trace_id.as_deref() {
            super::codex_router_log::append_event(
                "raw_upstream_status",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("model", request_model_for_log.clone()),
                    ("provider", provider.id.clone()),
                    ("status", status.as_u16().to_string()),
                    ("streaming", request_is_streaming.to_string()),
                    (
                        "elapsed_ms",
                        upstream_started_at.elapsed().as_millis().to_string(),
                    ),
                ],
            );
        }

        if !status.is_success() {
            let status_code = status.as_u16();
            let body_text = read_decoded_error_body(response).await?;
            if let Some(trace_id) = codex_trace_id.as_deref() {
                append_upstream_error_event(
                    trace_id,
                    &self.session_id,
                    &request_model_for_log,
                    &provider.id,
                    status_code,
                    body_text.as_deref(),
                    None,
                );
            }
            return Err(ProxyError::UpstreamError {
                status: status_code,
                body: body_text,
            });
        }

        let response = self
            .prepare_success_response_for_failover(response, request_is_streaming)
            .await?;
        if let Some(trace_id) = codex_trace_id.as_deref() {
            super::codex_router_log::append_event(
                "raw_response_ready",
                &[
                    ("trace", trace_id.to_string()),
                    ("session", self.session_id.clone()),
                    ("model", request_model_for_log.clone()),
                    ("provider", provider.id.clone()),
                    ("status", status.as_u16().to_string()),
                    ("streaming", request_is_streaming.to_string()),
                ],
            );
        }

        Ok((
            response,
            provider.clone(),
            route_body
                .get("model")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
        ))
    }

    /// 连接 Codex GPT-Live `/v1/live` 的上游 WebSocket。
    ///
    /// 与普通 raw HTTP 转发不同，这里必须先完成上游 WebSocket 握手，再返回 101
    /// 给 Codex；否则 WebSocket Upgrade 会被 Axum/HTTP body 处理吞掉。
    pub async fn open_codex_realtime_websocket(
        &self,
        app_type: &AppType,
        endpoint: &str,
        route_body: &Value,
        headers: &http::HeaderMap,
    ) -> Result<CodexRealtimeWebSocketStream, ProxyError> {
        let adapter = get_adapter(app_type);
        let canonical_path = codex_realtime_live_path(endpoint).ok_or_else(|| {
            ProxyError::InvalidRequest(format!(
                "endpoint '{endpoint}' is not a Codex GPT-Live websocket path"
            ))
        })?;
        let codex_trace_id = uuid::Uuid::new_v4().to_string();
        let request_model_for_log = route_body
            .get("model")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown")
            .to_string();
        let provider = self
            .resolve_codex_raw_endpoint_provider(app_type, endpoint, route_body)
            .await?;
        let base_url = adapter.extract_base_url(&provider)?;
        let backend_shape = base_url.contains("/backend-api");
        let (endpoint_path, passthrough_query) = split_endpoint_and_query(endpoint);
        let effective_endpoint = match passthrough_query {
            Some(query) if !query.is_empty() => format!("{endpoint_path}?{query}"),
            _ => endpoint_path.to_string(),
        };
        let upstream_url = if backend_shape {
            let call_suffix = canonical_path
                .strip_prefix("/v1/live/")
                .map(|call_id| format!("/{call_id}"))
                .unwrap_or_default();
            let mut url = adapter.build_url(&base_url, &format!("/realtime/calls{call_suffix}"));
            if let Some(query) = passthrough_query {
                if !query.is_empty() {
                    url.push(if url.contains('?') { '&' } else { '?' });
                    url.push_str(query);
                }
            }
            url
        } else {
            adapter.build_url(&base_url, &effective_endpoint)
        };

        let mut auth_strategy_for_log = "none".to_string();
        let mut log_secrets: Vec<String> = Vec::new();
        let mut codex_oauth_account_id: Option<String> = None;
        let mut is_codex_oauth = false;
        let codex_official_auth_passthrough =
            should_passthrough_codex_official_auth(app_type, &provider, headers);
        if codex_official_auth_passthrough {
            validate_codex_official_authorization(headers)?;
        }
        let mut auth_headers = if let Some(mut auth) = adapter.extract_auth(&provider) {
            if auth.strategy == AuthStrategy::CodexOAuth {
                if let Some(app_handle) = &self.app_handle {
                    let codex_state = app_handle.state::<CodexOAuthState>();
                    let codex_auth: tokio::sync::RwLockReadGuard<'_, CodexOAuthManager> =
                        codex_state.0.read().await;
                    let account_id = provider
                        .meta
                        .as_ref()
                        .and_then(|m| m.managed_account_id_for("codex_oauth"));
                    let token_result = match &account_id {
                        Some(id) => codex_auth.get_valid_token_for_account(id).await,
                        None => codex_auth.get_valid_token().await,
                    };
                    match token_result {
                        Ok(token) => {
                            auth = AuthInfo::new(token, AuthStrategy::CodexOAuth);
                            is_codex_oauth = true;
                            codex_oauth_account_id = match account_id {
                                Some(id) => Some(id),
                                None => codex_auth.default_account_id().await,
                            };
                        }
                        Err(err) => {
                            return Err(ProxyError::AuthError(format!(
                                "Codex OAuth 认证失败: {err}"
                            )));
                        }
                    }
                } else {
                    return Err(ProxyError::AuthError(
                        "Codex OAuth 认证不可用（无 AppHandle）".to_string(),
                    ));
                }
            }
            auth_strategy_for_log = format!("{:?}", auth.strategy);
            for secret in std::iter::once(&auth.api_key).chain(auth.access_token.iter()) {
                if !secret.is_empty() && !log_secrets.contains(secret) {
                    log_secrets.push(secret.clone());
                }
            }
            adapter.get_auth_headers(&auth)?
        } else {
            Vec::new()
        };
        if codex_official_auth_passthrough {
            replace_with_native_codex_auth_headers(&mut auth_headers, headers);
        }
        if let Some(account_id) = codex_oauth_account_id {
            if let Ok(value) = http::HeaderValue::from_str(&account_id) {
                auth_headers.push((http::HeaderName::from_static("chatgpt-account-id"), value));
            }
        }
        let codex_oauth_session_headers = if is_codex_oauth && self.session_client_provided {
            build_codex_oauth_session_headers(&self.session_id)
        } else {
            Vec::new()
        };

        let upstream_host = upstream_url
            .parse::<http::Uri>()
            .ok()
            .and_then(|uri| uri.authority().map(|authority| authority.to_string()));
        let custom_user_agent = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.custom_user_agent_header().ok().flatten());
        let mut ordered_headers = build_raw_passthrough_headers(
            headers,
            &auth_headers,
            upstream_host.as_deref(),
            custom_user_agent.as_ref(),
        );
        for (name, value) in codex_oauth_session_headers {
            if !ordered_headers.contains_key(&name) {
                ordered_headers.insert(name, value);
            }
        }
        for name in [
            http::header::SEC_WEBSOCKET_KEY,
            http::header::SEC_WEBSOCKET_VERSION,
            http::header::SEC_WEBSOCKET_EXTENSIONS,
            http::header::SEC_WEBSOCKET_PROTOCOL,
        ] {
            ordered_headers.remove(name);
        }
        apply_local_proxy_header_overrides(
            &mut ordered_headers,
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.local_proxy_request_overrides.as_ref()),
            false,
        );
        enforce_codex_oauth_originator(
            &mut ordered_headers,
            is_codex_oauth || codex_official_auth_passthrough,
            self.preserve_codex_client_originator,
        );
        reject_proxy_placeholder_for_managed_account_upstream(&upstream_url, &ordered_headers)?;

        let target_for_log = if log_secrets.is_empty() {
            crate::redact_url_origin_for_log(&upstream_url)
        } else {
            crate::redact_url_for_log_with_secrets(&upstream_url, &log_secrets)
        };
        super::codex_router_log::append_event(
            "realtime_websocket_connecting",
            &[
                ("trace", codex_trace_id.clone()),
                ("session", self.session_id.clone()),
                ("endpoint", endpoint.to_string()),
                ("model", request_model_for_log.clone()),
                ("provider", provider.id.clone()),
                ("upstream_url", target_for_log.clone()),
                ("auth_strategy", auth_strategy_for_log),
            ],
        );

        let mut request = upstream_url.as_str().into_client_request().map_err(|err| {
            ProxyError::ForwardFailed(format!(
                "failed to build Codex realtime websocket request: {err}"
            ))
        })?;
        request.headers_mut().extend(ordered_headers);
        let (stream, response) =
            tokio_tungstenite::connect_async(request)
                .await
                .map_err(|err| {
                    ProxyError::ForwardFailed(format!(
                        "failed to connect Codex realtime websocket: {err}"
                    ))
                })?;
        let status = response.status();
        if status.as_u16() != 101 {
            return Err(ProxyError::UpstreamError {
                status: status.as_u16(),
                body: None,
            });
        }
        super::codex_router_log::append_event(
            "realtime_websocket_connected",
            &[
                ("trace", codex_trace_id),
                ("session", self.session_id.clone()),
                ("provider", provider.id),
                ("status", status.as_u16().to_string()),
            ],
        );
        Ok(CodexRealtimeWebSocketStream(stream))
    }

    /// 解析 Codex raw endpoint 的真实 route provider。
    ///
    /// 与 `forward_raw` 共用同一套规则：显式模型命中优先，未知官方原生 endpoint
    /// 固定落到 official/Codex OAuth route，绝不交给 DeepSeek/Qwen 文本路由。
    async fn resolve_codex_raw_endpoint_provider(
        &self,
        app_type: &AppType,
        endpoint: &str,
        route_body: &Value,
    ) -> Result<Provider, ProxyError> {
        let providers = self
            .router
            .select_providers(app_type.as_str())
            .await
            .map_err(|err| ProxyError::DatabaseError(err.to_string()))?;
        let provider = providers
            .first()
            .cloned()
            .ok_or(ProxyError::NoAvailableProvider)?;
        let provider_is_resolved_codex_route = provider
            .settings_config
            .get("codexResolvedRouteId")
            .is_some();
        let routed_provider =
            if matches!(app_type, AppType::Codex) && !provider_is_resolved_codex_route {
                resolve_codex_raw_passthrough_route_provider(&provider, route_body)
            } else {
                None
            };
        let routed_provider = if let Some(route_provider) = routed_provider {
            if let Some(target_provider_id) =
                super::providers::codex_route_target_provider_id(&route_provider)
            {
                let Some(target_provider) = self
                    .router
                    .get_provider_by_id(target_provider_id, app_type.as_str())
                    .map_err(|err| {
                        ProxyError::ConfigError(format!(
                            "读取 Codex raw route 目标供应商 '{target_provider_id}' 失败: {err}"
                        ))
                    })?
                else {
                    return Err(ProxyError::ConfigError(format!(
                        "Codex raw route 引用了不存在的目标供应商 '{target_provider_id}'"
                    )));
                };
                Some(
                    super::providers::materialize_codex_routed_provider_from_target(
                        &route_provider,
                        &target_provider,
                    ),
                )
            } else {
                Some(route_provider)
            }
        } else {
            None
        };
        let provider = routed_provider.unwrap_or(provider);
        let adapter = get_adapter(app_type);
        let base_url = adapter.extract_base_url(&provider)?;
        reject_codex_effective_local_proxy_upstream(
            app_type,
            &base_url,
            &format!("raw endpoint '{endpoint}'"),
        )?;
        Ok(provider)
    }

    /// 故障转移开启时，成功不能只看上游响应头。
    ///
    /// - 非流式：先把完整 body 读到内存，读超时/连接中断会回到 retry loop 尝试下一家。
    /// - 流式：至少等首个 chunk 到达，避免上游返回 200 后一直不吐 SSE 时被误记成功。
    async fn prepare_success_response_for_failover(
        &self,
        response: ProxyResponse,
        request_is_streaming: bool,
    ) -> Result<ProxyResponse, ProxyError> {
        if request_is_streaming {
            return self.prime_streaming_response(response).await;
        }

        if self.non_streaming_timeout.is_zero() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let body_timeout = self.non_streaming_timeout;
        let body = super::response_grace::await_with_response_grace(
            response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES),
            body_timeout,
            super::response_grace::RESPONSE_PENDING_GRACE,
            || {
                ProxyError::ResponsePending(format!(
                    "响应体读取超时: {}s（上游发完响应头后 body 未到达）",
                    body_timeout.as_secs()
                ))
            },
        )
        .await?;

        Ok(ProxyResponse::buffered(status, headers, body))
    }

    /// Some Anthropic-compatible gateways return an Anthropic error envelope with
    /// HTTP 2xx. Validate it inside the retry loop so the request can fail over to
    /// the next provider; the response transformer runs too late for that.
    async fn validate_codex_anthropic_success_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        let status = response.status();
        let headers = response.headers().clone();
        let encoding = get_content_encoding(&headers);
        let raw = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await?;
        let decoded = match encoding {
            Some(encoding) => {
                match decompress_body_with_limit(&encoding, &raw, MAX_RESPONSE_BODY_BYTES) {
                    Ok(Some(decompressed)) => decompressed,
                    _ => raw.to_vec(),
                }
            }
            None => raw.to_vec(),
        };

        if let Some(message) = codex_anthropic_error_envelope_message(&decoded) {
            return Err(ProxyError::TransformError(format!(
                "Anthropic upstream returned a 2xx error envelope: {message}"
            )));
        }

        Ok(ProxyResponse::buffered(status, headers, raw))
    }

    async fn validate_responses_success_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        let status = response.status();
        let headers = response.headers().clone();
        let encoding = get_content_encoding(&headers);
        let raw = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await?;
        let decoded = match encoding {
            Some(encoding) => {
                match decompress_body_with_limit(&encoding, &raw, MAX_RESPONSE_BODY_BYTES) {
                    Ok(Some(decompressed)) => decompressed,
                    _ => raw.to_vec(),
                }
            }
            None => raw.to_vec(),
        };

        if let Some(message) = responses_error_envelope_message(&decoded) {
            return Err(ProxyError::TransformError(format!(
                "Responses upstream returned a 2xx failure: {message}"
            )));
        }

        Ok(ProxyResponse::buffered(status, headers, raw))
    }

    async fn validate_responses_stream_start(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        const MAX_PRIME_BYTES: usize = 256 * 1024;

        let status = response.status();
        let headers = response.headers().clone();
        let mut stream = Box::pin(response.bytes_stream());
        let mut replay_chunks: Vec<Bytes> = Vec::new();
        let mut parse_buffer = String::new();
        let mut utf8_remainder = Vec::new();

        loop {
            let next_result = async {
                match stream.next().await {
                    Some(chunk) => Ok(Some(chunk.map_err(|error| {
                        ProxyError::ForwardFailed(format!(
                            "Failed while validating Responses stream start: {error}"
                        ))
                    })?)),
                    None => Ok(None),
                }
            };
            let next = if self.streaming_first_byte_timeout.is_zero() {
                next_result.await?
            } else {
                super::response_grace::await_with_response_grace(
                    next_result,
                    self.streaming_first_byte_timeout,
                    super::response_grace::RESPONSE_PENDING_GRACE,
                    || {
                        ProxyError::ResponsePending(format!(
                            "Responses stream produced no semantic output within {}s",
                            self.streaming_first_byte_timeout.as_secs()
                        ))
                    },
                )
                .await?
            };

            let Some(chunk) = next else {
                if let Some(outcome) = inspect_responses_json_document(&parse_buffer) {
                    outcome?;
                    let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                    return Ok(ProxyResponse::streamed(status, headers, replay));
                }
                if !parse_buffer.trim().is_empty() {
                    if let Some(outcome) = inspect_responses_start_event(parse_buffer.trim()) {
                        outcome?;
                        let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok));
                        return Ok(ProxyResponse::streamed(status, headers, replay));
                    }
                }
                return Err(ProxyError::ForwardFailed(
                    "Responses stream ended before producing output or a terminal event"
                        .to_string(),
                ));
            };
            crate::proxy::sse::append_utf8_safe(&mut parse_buffer, &mut utf8_remainder, &chunk);
            replay_chunks.push(chunk);

            // Some compatible gateways ignore `stream:true` and return a complete
            // Responses JSON document without a JSON content-type. Recognize that
            // shape before looking for SSE delimiters; pretty-printed JSON may itself
            // contain blank lines and must stay intact.
            if let Some(outcome) = inspect_responses_json_document(&parse_buffer) {
                outcome?;
                let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
                return Ok(ProxyResponse::streamed(status, headers, replay));
            }

            while let Some(block) = crate::proxy::sse::take_sse_block(&mut parse_buffer) {
                if let Some(outcome) = inspect_responses_start_event(&block) {
                    outcome?;
                    let replay =
                        futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
                    return Ok(ProxyResponse::streamed(status, headers, replay));
                }
            }

            if replay_chunks.iter().map(Bytes::len).sum::<usize>() >= MAX_PRIME_BYTES {
                log::warn!(
                    "[Claude/Responses] semantic stream priming exceeded {MAX_PRIME_BYTES} bytes; committing buffered stream"
                );
                let replay = futures::stream::iter(replay_chunks.into_iter().map(Ok)).chain(stream);
                return Ok(ProxyResponse::streamed(status, headers, replay));
            }
        }
    }

    async fn prime_streaming_response(
        &self,
        response: ProxyResponse,
    ) -> Result<ProxyResponse, ProxyError> {
        if self.streaming_first_byte_timeout.is_zero() {
            return Ok(response);
        }

        let status = response.status();
        let headers = response.headers().clone();
        let timeout = self.streaming_first_byte_timeout;
        let mut stream = Box::pin(response.bytes_stream());

        let first_result = async {
            match stream.next().await {
                Some(chunk) => Ok(Some(chunk.map_err(|e| {
                    ProxyError::ForwardFailed(format!("读取流式响应首包失败: {e}"))
                })?)),
                None => Ok(None),
            }
        };
        let first = if timeout.is_zero() {
            first_result.await?
        } else {
            super::response_grace::await_with_response_grace(
                first_result,
                timeout,
                super::response_grace::RESPONSE_PENDING_GRACE,
                || {
                    ProxyError::ResponsePending(format!(
                        "流式响应首包超时: {}s（上游已返回响应头但未返回数据）",
                        timeout.as_secs()
                    ))
                },
            )
            .await?
        };

        let Some(first) = first else {
            return Err(ProxyError::ForwardFailed(
                "流式响应在首包到达前结束".to_string(),
            ));
        };

        if let Some(message) = retryable_error_from_primed_sse_chunk(&first) {
            return Err(ProxyError::UpstreamError {
                status: 503,
                body: Some(message),
            });
        }

        let replay = futures::stream::once(async move { Ok(first) }).chain(stream);
        Ok(ProxyResponse::streamed(status, headers, replay))
    }

    async fn resolve_claude_api_format(
        &self,
        provider: &Provider,
        body: &Value,
        is_copilot: bool,
    ) -> String {
        if !is_copilot {
            return super::providers::get_claude_api_format(provider).to_string();
        }

        let model = body.get("model").and_then(|value| value.as_str());
        if let Some(model_id) = model {
            if self
                .is_copilot_openai_vendor_model(provider, model_id)
                .await
            {
                return "openai_responses".to_string();
            }
        }

        "openai_chat".to_string()
    }

    /// 用 Copilot live `/models` 列表确认 model ID 真实可用，找不到时按 family 降级。
    /// 命中缓存后是同步的；首次请求或 5 min 缓存过期后会触发一次 HTTP。
    async fn apply_copilot_live_model_resolution(
        &self,
        provider: &Provider,
        body: &mut serde_json::Value,
    ) {
        let Some(model_id) = body.get("model").and_then(|v| v.as_str()) else {
            return;
        };
        let model_id = model_id.to_string();

        let Some(app_handle) = &self.app_handle else {
            return;
        };
        let copilot_state = app_handle.state::<CopilotAuthState>();
        let copilot_auth = copilot_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        let models_result = match account_id.as_deref() {
            Some(id) => copilot_auth.fetch_models_for_account(id).await,
            None => copilot_auth.fetch_models().await,
        };

        let models = match models_result {
            Ok(m) => m,
            Err(err) => {
                log::debug!("[Copilot] live model list unavailable, skip resolution: {err}");
                return;
            }
        };

        if let Some(resolved) =
            super::providers::copilot_model_map::resolve_against_models(&model_id, &models)
        {
            log::info!("[Copilot] live-model resolve: {model_id} → {resolved}");
            body["model"] = serde_json::Value::String(resolved);
        }
    }

    async fn is_copilot_openai_vendor_model(&self, provider: &Provider, model_id: &str) -> bool {
        let Some(app_handle) = &self.app_handle else {
            log::debug!("[Copilot] AppHandle unavailable, fallback to chat/completions");
            return false;
        };

        let copilot_state = app_handle.state::<CopilotAuthState>();
        let copilot_auth = copilot_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|m| m.managed_account_id_for("github_copilot"));

        let vendor_result = match account_id.as_deref() {
            Some(id) => {
                copilot_auth
                    .get_model_vendor_for_account(id, model_id)
                    .await
            }
            None => copilot_auth.get_model_vendor(model_id).await,
        };

        match vendor_result {
            Ok(Some(vendor)) => vendor.eq_ignore_ascii_case("openai"),
            Ok(None) => {
                log::debug!(
                    "[Copilot] Model vendor unavailable for {model_id}, fallback to chat/completions"
                );
                false
            }
            Err(err) => {
                log::warn!(
                    "[Copilot] Failed to resolve model vendor for {model_id}, fallback to chat/completions: {err}"
                );
                false
            }
        }
    }

    fn categorize_proxy_error(&self, error: &ProxyError, provider: &Provider) -> ErrorCategory {
        if provider_codex_pool_account(provider).is_some()
            && matches!(
                classify_codex_pool_attempt(error),
                CodexPoolAttemptOutcome::Credential { .. }
                    | CodexPoolAttemptOutcome::Quota { .. }
                    | CodexPoolAttemptOutcome::Transient { .. }
            )
        {
            return ErrorCategory::Retryable;
        }

        // Authentication belongs to the Codex client for the built-in official
        // route. Retrying another provider would silently move the conversation
        // away from the selected official account and poison its health state.
        if super::providers::is_codex_official_provider(provider)
            && (matches!(error, ProxyError::AuthError(_))
                || matches!(
                    error,
                    ProxyError::UpstreamError {
                        status: 401 | 403,
                        ..
                    }
                ))
        {
            return ErrorCategory::NonRetryable;
        }

        // xAI OAuth mirrors the same rule for token acquisition: a local
        // AuthError means the managed account needs re-login. Failing over
        // would silently move the conversation off the selected Grok account
        // and poison the provider's health state for an account-level issue.
        if provider.is_xai_oauth() && matches!(error, ProxyError::AuthError(_)) {
            return ErrorCategory::NonRetryable;
        }

        match error {
            // 网络和上游错误：都应该尝试下一个供应商
            ProxyError::Timeout(_) => ErrorCategory::Retryable,
            ProxyError::ResponsePending(_) => ErrorCategory::NonRetryable,
            ProxyError::ForwardFailed(_) => ErrorCategory::Retryable,
            ProxyError::ProviderUnhealthy(_) => ErrorCategory::Retryable,
            // 上游 HTTP 错误：按状态码分桶。
            //
            // 客户端请求自身有问题的状态码无论换哪个 provider 都会被拒绝，
            // 继续轮询只会放大错误率、污染熔断器健康度、浪费配额：
            //   400 Bad Request / 422 Unprocessable Entity   ← 请求体格式或语义错误
            //   405 Method Not Allowed / 406 Not Acceptable  ← 方法或 Accept 错误
            //   413 Payload Too Large / 414 URI Too Long     ← 客户端构造超限
            //   415 Unsupported Media Type                    ← Content-Type 错误
            //   501 Not Implemented                           ← 上游协议确实不支持
            //
            // 其他 4xx（401/403/404/408/409/429/451 等）和全部 5xx 都保留
            // Retryable —— 换一家 provider 可能持有不同的 key、配额、地域或模型映射。
            ProxyError::UpstreamError { status, .. } => match *status {
                400 | 405 | 406 | 413 | 414 | 415 | 422 | 501 => ErrorCategory::NonRetryable,
                _ => ErrorCategory::Retryable,
            },
            // Provider 级配置/转换问题：换一个 Provider 可能就能成功
            ProxyError::ConfigError(_) => ErrorCategory::Retryable,
            ProxyError::TransformError(_) => ErrorCategory::Retryable,
            ProxyError::AuthError(_) => ErrorCategory::Retryable,
            ProxyError::StreamIdleTimeout(_) => ErrorCategory::Retryable,
            // 无可用供应商：所有供应商都试过了，无法重试
            ProxyError::NoAvailableProvider => ErrorCategory::NonRetryable,
            // 其他错误（数据库/内部错误等）：不是换供应商能解决的问题
            _ => ErrorCategory::NonRetryable,
        }
    }
}

/// 从 ProxyError 中提取错误消息
fn extract_error_message(error: &ProxyError) -> Option<String> {
    match error {
        ProxyError::UpstreamError { body, .. } => body.clone(),
        _ => Some(error.to_string()),
    }
}

/// 检测 Provider 是否为 Bedrock（通过 CLAUDE_CODE_USE_BEDROCK 环境变量判断）
fn is_bedrock_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("env")
        .and_then(|e| e.get("CLAUDE_CODE_USE_BEDROCK"))
        .and_then(|v| v.as_str())
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn build_retryable_failure_log(
    provider_name: &str,
    attempted_providers: usize,
    total_providers: usize,
    error: &ProxyError,
) -> (&'static str, String) {
    let error_summary = summarize_proxy_error(error);

    if total_providers <= 1 {
        (
            log_fwd::SINGLE_PROVIDER_FAILED,
            format!("Provider {provider_name} 请求失败: {error_summary}"),
        )
    } else {
        (
            log_fwd::PROVIDER_FAILED_RETRY,
            format!(
                "Provider {provider_name} 失败，继续尝试下一个 ({attempted_providers}/{total_providers}): {error_summary}"
            ),
        )
    }
}

fn build_terminal_failure_log(
    attempted_providers: usize,
    total_providers: usize,
    last_error: Option<&ProxyError>,
) -> Option<(&'static str, String)> {
    if total_providers <= 1 {
        return None;
    }

    let error_summary = last_error
        .map(summarize_proxy_error)
        .unwrap_or_else(|| "未知错误".to_string());

    Some((
        log_fwd::ALL_PROVIDERS_FAILED,
        format!(
            "已尝试 {attempted_providers}/{total_providers} 个 Provider，均失败。最后错误: {error_summary}"
        ),
    ))
}

fn summarize_proxy_error(error: &ProxyError) -> String {
    match error {
        ProxyError::UpstreamError { status, body } => {
            let body_summary = body
                .as_deref()
                .map(summarize_upstream_body)
                .filter(|summary| !summary.is_empty());

            match body_summary {
                Some(summary) => format!("上游 HTTP {status}: {summary}"),
                None => format!("上游 HTTP {status}"),
            }
        }
        ProxyError::Timeout(message) => {
            format!("请求超时: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ForwardFailed(message) => {
            format!("请求转发失败: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::TransformError(message) => {
            format!("响应转换失败: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::ConfigError(message) => {
            format!("配置错误: {}", summarize_text_for_log(message, 180))
        }
        ProxyError::AuthError(message) => {
            format!("认证失败: {}", summarize_text_for_log(message, 180))
        }
        _ => summarize_text_for_log(&error.to_string(), 180),
    }
}

/// 从已经预读到的首个 SSE 分块里识别“上游还没真正开始生成就失败”的错误。
///
/// 这类错误常见于 ChatGPT/Codex OAuth 在高负载时返回 HTTP 200 + `event: error`
/// 或 `event: response.failed`。如果此时直接把响应头交给 Codex，后续已经无法在同一个
/// HTTP 请求里切换到下一条路由；在首包阶段把它还原为 503，才能复用现有 failover/retry
/// 机制。普通 `response.created` / delta 事件必须原样放行。
fn retryable_error_from_primed_sse_chunk(first: &Bytes) -> Option<String> {
    let text = std::str::from_utf8(first).ok()?;
    for block in text.split("\n\n") {
        let mut event_name: Option<&str> = None;
        let mut data_lines = Vec::new();

        for line in block.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event_name = Some(value.trim());
            } else if let Some(value) = line.strip_prefix("data:") {
                data_lines.push(value.trim());
            }
        }

        if data_lines.is_empty() {
            continue;
        }

        let data = data_lines.join("\n");
        let parsed = serde_json::from_str::<Value>(&data).ok();
        let event_is_error = matches!(
            event_name,
            Some("error" | "response.failed" | "response.error")
        );
        let payload_is_error = parsed.as_ref().is_some_and(|value| {
            value.get("error").is_some()
                || value
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| matches!(kind, "error" | "response.failed"))
                || value
                    .pointer("/response/status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status == "failed")
        });

        if event_is_error || payload_is_error {
            return Some(extract_sse_error_message(parsed.as_ref()).unwrap_or(data));
        }
    }

    None
}

/// 提取 SSE 错误体里最适合写入日志/返回给重试分类器的消息。
fn extract_sse_error_message(value: Option<&Value>) -> Option<String> {
    let value = value?;
    for pointer in [
        "/error/message",
        "/message",
        "/response/error/message",
        "/response/incomplete_details/reason",
    ] {
        if let Some(message) = value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty())
        {
            return Some(message.to_string());
        }
    }

    Some(value.to_string())
}

fn summarize_upstream_body(body: &str) -> String {
    if let Ok(json_body) = serde_json::from_str::<Value>(body) {
        if let Some(message) = extract_json_error_message(&json_body) {
            return summarize_text_for_log(&message, 180);
        }

        if let Ok(compact_json) = serde_json::to_string(&json_body) {
            return summarize_text_for_log(&compact_json, 180);
        }
    }

    summarize_text_for_log(body, 180)
}

fn extract_json_error_message(body: &Value) -> Option<String> {
    let candidates = [
        body.pointer("/error/message"),
        body.pointer("/message"),
        body.pointer("/detail"),
        body.pointer("/error"),
    ];

    candidates
        .into_iter()
        .flatten()
        .find_map(|value| value.as_str().map(ToString::to_string))
}

fn split_endpoint_and_query(endpoint: &str) -> (&str, Option<&str>) {
    endpoint
        .split_once('?')
        .map_or((endpoint, None), |(path, query)| (path, Some(query)))
}

fn strip_beta_query(query: Option<&str>) -> Option<String> {
    let filtered = query.map(|query| {
        query
            .split('&')
            .filter(|pair| !pair.is_empty() && !pair.starts_with("beta="))
            .collect::<Vec<_>>()
            .join("&")
    });

    match filtered.as_deref() {
        Some("") | None => None,
        Some(_) => filtered,
    }
}

fn is_claude_messages_path(path: &str) -> bool {
    matches!(path, "/v1/messages" | "/claude/v1/messages")
}

fn rewrite_codex_responses_endpoint_to_chat(endpoint: &str) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let target_path = "/chat/completions";
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

fn rewrite_codex_responses_endpoint_to_messages(endpoint: &str) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let target_path = "/v1/messages";
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

/// Claude Code client fingerprint (used for Codex→Anthropic emulation to pass a
/// gateway's "Claude Code only" check).
const CLAUDE_CODE_USER_AGENT: &str = "claude-cli/1.0.119 (external, cli)";
const CLAUDE_CODE_SYSTEM_IDENTITY: &str =
    "You are Claude Code, Anthropic's official CLI for Claude.";

/// Insert the Claude Code identity as the first line before the `system` field in
/// the Anthropic request body.
///
/// Anthropic subscription/OAuth plans require the first system block to be exactly
/// this identity line. After conversion `system` is a string (from Codex
/// instructions); normalize it into an array here: [identity line, original system...].
fn prepend_claude_code_system_prompt(body: &mut Value) {
    let identity = serde_json::json!({ "type": "text", "text": CLAUDE_CODE_SYSTEM_IDENTITY });
    let mut blocks: Vec<Value> = vec![identity];
    match body.get("system") {
        Some(Value::String(existing)) if !existing.is_empty() => {
            blocks.push(serde_json::json!({ "type": "text", "text": existing }));
        }
        Some(Value::Array(existing)) => {
            // Idempotent: skip re-injection if the first block is already the identity line.
            if existing
                .first()
                .and_then(|b| b.get("text"))
                .and_then(|t| t.as_str())
                == Some(CLAUDE_CODE_SYSTEM_IDENTITY)
            {
                return;
            }
            blocks.extend(existing.iter().cloned());
        }
        _ => {}
    }
    body["system"] = Value::Array(blocks);
}

/// Headers a native Claude Code client never sends but the Codex/OpenAI CLI (and its
/// stainless SDK layer) do. Dropped for every Codex→Anthropic request so the upstream sees a
/// clean Anthropic client fingerprint. Centralized here so the set stays in one place and future
/// additions can't miss a code path. `key_str` is already lowercased by the http crate.
/// Whether `base_url` already ends in `endpoint_suffix` (e.g. `/v1/messages` or
/// `/chat/completions`), ignoring surrounding whitespace, any `?query`/`#fragment`, and a
/// trailing slash. Used to avoid double-appending the endpoint when a user pastes a full
/// URL but leaves the "full URL" switch off (`.../v1/messages` → `.../v1/messages/v1/messages`,
/// a non-retryable 400). `endpoint_suffix` must be lowercase.
fn base_url_is_full_endpoint(base_url: &str, endpoint_suffix: &str) -> bool {
    let trimmed = base_url.trim();
    // Match against the path only: a `?query`/`#fragment` on a full endpoint URL must not
    // hide the suffix (`.../v1/messages?beta=true` still ends in the endpoint).
    let path = match trimmed.split_once(['?', '#']) {
        Some((head, _)) => head,
        None => trimmed,
    };
    path.trim_end_matches('/')
        .to_ascii_lowercase()
        .ends_with(endpoint_suffix)
}

fn is_codex_client_fingerprint_header(key_str: &str) -> bool {
    matches!(
        key_str,
        "originator"
            | "session_id"
            | "session-id"
            | "thread-id"
            | "conversation_id"
            | "chatgpt-account-id"
            | "x-openai-subagent"
            | "x-client-request-id"
            | "openai-beta"
            | "openai-organization"
            | "openai-project"
    ) || key_str.starts_with("x-stainless-")
        || key_str.starts_with("x-codex-")
}

fn codex_anthropic_error_envelope_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("error") && value.get("error").is_none() {
        return None;
    }
    let error = value.get("error").unwrap_or(&value);
    let error_type = error.get("type").and_then(Value::as_str).unwrap_or("error");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| error.to_string());
    Some(format!("{error_type}: {message}"))
}

fn responses_error_envelope_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let status = value.get("status").and_then(Value::as_str);
    let has_error = value.get("error").is_some_and(|error| !error.is_null());
    if !matches!(status, Some("failed" | "cancelled")) && !has_error {
        return None;
    }

    let error = value.get("error").unwrap_or(&value);
    let error_type = error
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| error.get("code").and_then(Value::as_str))
        .unwrap_or_else(|| status.unwrap_or("error"));
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| error.as_str())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or(match status {
            Some("cancelled") => "response generation was cancelled",
            _ => "response generation failed",
        });
    Some(format!("{error_type}: {message}"))
}

/// Prompt caching is part of the Codex→Anthropic protocol bridge rather than an
/// optional Bedrock optimizer. Codex requests do not contain Anthropic
/// `cache_control`, so keep bridge caching on by default while still honoring the
/// dedicated cache-injection switch. Injected breakpoints always use Anthropic's
/// standard 5-minute TTL.
fn codex_anthropic_cache_config(config: &OptimizerConfig) -> OptimizerConfig {
    OptimizerConfig {
        enabled: true,
        thinking_optimizer: false,
        cache_injection: config.cache_injection,
    }
}

/// A streaming request may receive a whole JSON document even when the gateway
/// omits `application/json`. `None` means either "not JSON" or "not complete yet";
/// a parsed document is safe to commit unless it is a semantic failure envelope.
fn inspect_responses_json_document(buffer: &str) -> Option<Result<(), ProxyError>> {
    let trimmed = buffer.trim();
    if !matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'[')) {
        return None;
    }
    let _: Value = serde_json::from_str(trimmed).ok()?;
    if let Some(message) = responses_error_envelope_message(trimmed.as_bytes()) {
        return Some(Err(ProxyError::TransformError(format!(
            "Responses upstream returned a 2xx failure: {message}"
        ))));
    }
    Some(Ok(()))
}

/// Inspect one complete Responses SSE block while the response is still inside
/// the retry loop. `None` means the event is lifecycle-only and priming should
/// continue; `Some(Ok(()))` means it is safe to commit/replay the stream.
fn inspect_responses_start_event(block: &str) -> Option<Result<(), ProxyError>> {
    let mut named_event = None;
    let mut data_lines = Vec::new();
    for line in block.lines() {
        if let Some(event) = crate::proxy::sse::strip_sse_field(line, "event") {
            named_event = Some(event.trim().to_string());
        } else if let Some(data) = crate::proxy::sse::strip_sse_field(line, "data") {
            data_lines.push(data);
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let value: Value = match serde_json::from_str(&data_lines.join("\n")) {
        Ok(value) => value,
        Err(_) => return None,
    };
    let event = named_event
        .as_deref()
        .filter(|event| !event.is_empty())
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("");

    let response = value.get("response").unwrap_or(&value);
    if matches!(
        response.get("status").and_then(Value::as_str),
        Some("failed" | "cancelled")
    ) || response.get("error").is_some_and(|error| !error.is_null())
    {
        let error = response.get("error").unwrap_or(response);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("Responses upstream failed before output");
        let error_type = error
            .get("type")
            .and_then(Value::as_str)
            .or_else(|| error.get("code").and_then(Value::as_str))
            .or_else(|| response.get("status").and_then(Value::as_str))
            .unwrap_or("upstream_error");
        return Some(Err(ProxyError::TransformError(format!(
            "Responses upstream {error_type}: {message}"
        ))));
    }

    match event {
        "response.failed" | "error" => {
            let error = response.get("error").unwrap_or(response);
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
                .unwrap_or("Responses upstream emitted an error before output");
            let error_type = error
                .get("type")
                .and_then(Value::as_str)
                .or_else(|| error.get("code").and_then(Value::as_str))
                .unwrap_or("upstream_error");
            Some(Err(ProxyError::TransformError(format!(
                "Responses upstream {error_type}: {message}"
            ))))
        }
        "response.created" | "response.in_progress" | "response.queued" => None,
        "" => None,
        // Productive output, incomplete, and completed terminals are all safe to
        // expose. Mid-stream failures after this point are surfaced by the converter
        // but intentionally do not switch providers.
        _ => Some(Ok(())),
    }
}

/// Rewrite Codex's `/responses` (and variants) to Anthropic's `/v1/messages`, preserving the query.
fn rewrite_codex_responses_endpoint_to_anthropic(endpoint: &str) -> (String, Option<String>) {
    let (_path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = query.map(ToString::to_string);
    let target_path = "/v1/messages";
    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexRequestMetadataSummary {
    request_kind: String,
    compaction_trigger: Option<String>,
    compaction_reason: Option<String>,
    compaction_implementation: Option<String>,
    compaction_phase: Option<String>,
}

impl CodexRequestMetadataSummary {
    fn from_request(
        app_type: &AppType,
        endpoint: &str,
        body: &Value,
        headers: &http::HeaderMap,
    ) -> Self {
        if !matches!(app_type, AppType::Codex) {
            return Self::default_turn();
        }

        let metadata = codex_turn_metadata_from_headers(headers)
            .or_else(|| codex_turn_metadata_from_body(body));
        let mut summary = metadata
            .as_ref()
            .map(Self::from_metadata)
            .unwrap_or_else(Self::default_turn);

        if super::providers::is_codex_remote_compact_endpoint(endpoint)
            && summary.request_kind == "turn"
        {
            summary.request_kind = "compaction".to_string();
        }

        summary
    }

    fn from_metadata(metadata: &Value) -> Self {
        let request_kind = metadata
            .get("request_kind")
            .and_then(Value::as_str)
            .unwrap_or("turn")
            .to_string();
        let compaction = metadata.get("compaction");

        Self {
            request_kind,
            compaction_trigger: metadata_string_field(compaction, "trigger"),
            compaction_reason: metadata_string_field(compaction, "reason"),
            compaction_implementation: metadata_string_field(compaction, "implementation"),
            compaction_phase: metadata_string_field(compaction, "phase"),
        }
    }

    fn default_turn() -> Self {
        Self {
            request_kind: "turn".to_string(),
            compaction_trigger: None,
            compaction_reason: None,
            compaction_implementation: None,
            compaction_phase: None,
        }
    }
}

pub(crate) fn codex_request_is_v2_compaction(
    app_type: &AppType,
    endpoint: &str,
    body: &Value,
    headers: &http::HeaderMap,
) -> bool {
    let summary = CodexRequestMetadataSummary::from_request(app_type, endpoint, body, headers);
    if summary.request_kind != "compaction" {
        return false;
    }
    if summary.compaction_implementation.as_deref() == Some("responses_compaction_v2") {
        return true;
    }
    // Older metadata may omit the implementation field. Remote compaction v2 is the
    // only current variant that rides a normal /responses stream with a trigger item;
    // legacy /responses/compact and local compaction keep their own output contracts.
    !super::providers::is_codex_remote_compact_endpoint(endpoint)
        && summary.compaction_implementation.is_none()
        && summary.compaction_trigger.is_some()
}

fn metadata_string_field(metadata: Option<&Value>, key: &str) -> Option<String> {
    metadata?
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn codex_turn_metadata_from_headers(headers: &http::HeaderMap) -> Option<Value> {
    headers
        .get("x-codex-turn-metadata")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
}

fn codex_turn_metadata_from_body(body: &Value) -> Option<Value> {
    body.get("client_metadata")
        .and_then(|metadata| metadata.get("x-codex-turn-metadata"))
        .and_then(|value| match value {
            Value::String(raw) => serde_json::from_str::<Value>(raw).ok(),
            Value::Object(_) => Some(value.clone()),
            _ => None,
        })
}
fn rewrite_claude_transform_endpoint(
    endpoint: &str,
    api_format: &str,
    is_copilot: bool,
    body: &Value,
) -> (String, Option<String>) {
    let (path, query) = split_endpoint_and_query(endpoint);
    let passthrough_query = if is_claude_messages_path(path) {
        strip_beta_query(query)
    } else {
        query.map(ToString::to_string)
    };

    if !is_claude_messages_path(path) {
        return (endpoint.to_string(), passthrough_query);
    }

    if api_format == "gemini_native" {
        let model =
            super::providers::transform_gemini::extract_gemini_model(body).unwrap_or("unknown");
        // Accept both bare ids (`gemini-2.5-pro`) and the resource-name
        // form (`models/gemini-2.5-pro`) that Gemini SDKs emit. See
        // `normalize_gemini_model_id` for rationale.
        let model = super::gemini_url::normalize_gemini_model_id(model);
        let is_stream = body
            .get("stream")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let target_path = if is_stream {
            format!("/v1beta/models/{model}:streamGenerateContent")
        } else {
            format!("/v1beta/models/{model}:generateContent")
        };

        let rewritten_query = merge_query_params(
            passthrough_query.as_deref(),
            if is_stream { Some("alt=sse") } else { None },
        );

        let rewritten = match rewritten_query.as_deref() {
            Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
            _ => target_path,
        };

        return (rewritten, rewritten_query);
    }

    let target_path = if is_copilot && api_format == "openai_responses" {
        "/v1/responses"
    } else if is_copilot {
        "/chat/completions"
    } else if api_format == "openai_responses" {
        "/v1/responses"
    } else {
        "/v1/chat/completions"
    };

    let rewritten = match passthrough_query.as_deref() {
        Some(query) if !query.is_empty() => format!("{target_path}?{query}"),
        _ => target_path.to_string(),
    };

    (rewritten, passthrough_query)
}

fn merge_query_params(base_query: Option<&str>, extra_param: Option<&str>) -> Option<String> {
    let mut params: Vec<String> = base_query
        .into_iter()
        .flat_map(|query| query.split('&'))
        .filter(|pair| !pair.is_empty())
        .filter(|pair| !pair.starts_with("alt="))
        .map(ToString::to_string)
        .collect();

    if let Some(extra_param) = extra_param {
        params.push(extra_param.to_string());
    }

    if params.is_empty() {
        None
    } else {
        Some(params.join("&"))
    }
}

fn append_query_to_full_url(base_url: &str, query: Option<&str>) -> String {
    match query {
        Some(query) if !query.is_empty() => {
            if base_url.contains('?') {
                format!("{base_url}&{query}")
            } else {
                format!("{base_url}?{query}")
            }
        }
        _ => base_url.to_string(),
    }
}

fn build_codex_oauth_session_headers(
    session_id: &str,
) -> Vec<(http::HeaderName, http::HeaderValue)> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Vec::new();
    }

    let mut headers = Vec::new();
    if let Ok(value) = http::HeaderValue::from_str(session_id) {
        headers.push((http::HeaderName::from_static("session-id"), value.clone()));
        headers.push((http::HeaderName::from_static("thread-id"), value.clone()));
        headers.push((http::HeaderName::from_static("x-client-request-id"), value));
    }

    let window_id = format!("{session_id}:0");
    if let Ok(value) = http::HeaderValue::from_str(&window_id) {
        headers.push((http::HeaderName::from_static("x-codex-window-id"), value));
    }

    headers
}

/// 判断 originator 是否属于官方 Codex 已声明的 first-party 客户端集合。
///
/// 该集合与官方 Codex `is_first_party_originator` 和
/// `is_first_party_chat_originator` 保持一致。`codex_exec` 虽是官方命令入口，但不在
/// 官方 first-party 分类中，因此不作为可保留值。
fn is_first_party_codex_originator(value: &str) -> bool {
    value == super::providers::CODEX_OAUTH_ORIGINATOR
        || value == "codex-tui"
        || value == "codex_vscode"
        || value.starts_with("Codex ")
        || value == "codex_atlas"
        || value == "codex_chatgpt_desktop"
}

/// 从官方 Codex User-Agent 中提取真实构建版本。
///
/// 官方格式为 `<process-originator>/<cargo-version> (<os...>) ...`。线程级 originator
/// 可以覆盖进程 originator，因此这里分别校验 User-Agent 自带的进程身份和版本，不能
/// 要求它与请求头中的线程来源相同。
fn codex_client_version_from_user_agent(user_agent: &str) -> Option<&str> {
    let (process_originator, remainder) = user_agent.split_once('/')?;
    if !is_first_party_codex_originator(process_originator) {
        return None;
    }
    let version = remainder.split_whitespace().next()?;
    (!version.is_empty()
        && version.len() <= 64
        && version.as_bytes()[0].is_ascii_digit()
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+')))
    .then_some(version)
}

/// 规范发往官方 ChatGPT Codex 后端的客户端身份。
///
/// 可信本地 Codex 请求只有在恰好携带一个官方 first-party 值时才保留原值；缺失、
/// 重复、未知值以及 External API/协议转换请求统一回退到官方 CLI 默认值。这样既避免
/// `originator=cc-switch` 触发模型准入差异，也不会把 Desktop/VS Code 误报成 CLI。
/// `version` 只信任 first-party User-Agent 中的进程构建版本：旧配置、外部 API 或
/// 客户端自报的独立 version 都先移除，再按可信 UA 重建。
fn enforce_codex_oauth_originator(
    headers: &mut http::HeaderMap,
    is_codex_official_upstream: bool,
    preserve_client_originator: bool,
) {
    if !is_codex_official_upstream {
        return;
    }

    let originator_name = http::HeaderName::from_static("originator");
    headers.remove("version");
    if !preserve_client_originator {
        headers.remove("x-oai-attestation");
    }
    if preserve_client_originator {
        let user_agent = headers
            .get(http::header::USER_AGENT)
            .and_then(|value| value.to_str().ok());
        if let Some(version) = user_agent.and_then(codex_client_version_from_user_agent) {
            if let Ok(version) = http::HeaderValue::from_str(version) {
                headers.insert(http::HeaderName::from_static("version"), version);
            }
        }

        let preserved_value = {
            let mut values = headers.get_all(&originator_name).iter();
            match (values.next(), values.next()) {
                (Some(value), None)
                    if value.to_str().is_ok_and(is_first_party_codex_originator) =>
                {
                    Some(value.clone())
                }
                _ => None,
            }
        };
        if let Some(value) = preserved_value {
            headers.insert(originator_name, value);
            return;
        }
    }

    headers.insert(
        originator_name,
        http::HeaderValue::from_static(super::providers::CODEX_OAUTH_ORIGINATOR),
    );
}

/// Native/Mixed route 的凭据真值来自进入 CCSM 的 Codex Desktop 请求。
/// raw endpoint 会统一丢弃来向认证头，因此必须在重建阶段显式放回官方 Bearer
/// 与账号头；External API 不会进入这个分支。
fn replace_with_native_codex_auth_headers(
    auth_headers: &mut Vec<(http::HeaderName, http::HeaderValue)>,
    source_headers: &http::HeaderMap,
) {
    auth_headers.clear();
    for name in [
        http::header::AUTHORIZATION,
        http::HeaderName::from_static("chatgpt-account-id"),
    ] {
        for value in source_headers.get_all(&name).iter() {
            auth_headers.push((name.clone(), value.clone()));
        }
    }
}

fn reject_proxy_placeholder_for_managed_account_upstream(
    url: &str,
    headers: &http::HeaderMap,
) -> Result<(), ProxyError> {
    if !is_managed_account_upstream_url(url) || !headers_contain_proxy_placeholder(headers) {
        return Ok(());
    }

    Err(ProxyError::AuthError(
        "Managed account proxy auth was not resolved; PROXY_MANAGED must not be sent upstream"
            .to_string(),
    ))
}

fn is_managed_account_upstream_url(url: &str) -> bool {
    let Ok(uri) = url.parse::<http::Uri>() else {
        return false;
    };

    let Some(host) = uri.host().map(str::to_ascii_lowercase) else {
        return false;
    };

    host == "githubcopilot.com"
        || host.ends_with(".githubcopilot.com")
        || (host == "chatgpt.com" && uri.path().starts_with("/backend-api/codex"))
        || (host == "api.x.ai" && uri.path().starts_with("/v1/"))
}

/// 读取 MultiRouter 级 hosted tool 开关；未配置时默认开启。
fn hosted_tool_bridge_enabled(settings: &Value, tool: &str) -> bool {
    settings
        .pointer(&format!("/hostedTools/{tool}/enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Decide whether the buffered Chat hosted-tool loop owns this request.
///
/// Streaming `auto` requests prioritize incremental agent progress: hosted
/// tools are omitted from the Chat projection, while ordinary client tools
/// remain available. An explicit hosted selection is safe to buffer because
/// the caller requested that exact bridge. Non-streaming requests retain the
/// existing automatic hosted-tool loop.
fn should_enable_hosted_tool_loop(request: &Value, client_requested_streaming: bool) -> bool {
    if !client_requested_streaming {
        return true;
    }

    let Some(choice) = request.get("tool_choice").and_then(Value::as_object) else {
        return false;
    };
    let choice_type = choice.get("type").and_then(Value::as_str);
    if matches!(choice_type, Some("web_search" | "image_generation")) {
        return true;
    }
    choice_type == Some("function")
        && matches!(
            choice.get("name").and_then(Value::as_str),
            Some("web_search" | "generate_image")
        )
}

/// 解析 hosted tool 调用凭据：优先显式 API Key，再回退请求自带的 Codex OAuth，最后用 CCSM 托管 OAuth。
async fn resolve_hosted_tool_client(
    app_handle: Option<&tauri::AppHandle>,
    provider: &Provider,
    source_headers: &http::HeaderMap,
) -> Result<OpenAiHostedToolClient, String> {
    if !OpenAiHostedToolClient::hosted_tool_bridge_env_enabled() {
        return Err("OpenAI hosted tool bridge is disabled by environment".to_string());
    }
    if let Some(Ok(client)) = OpenAiHostedToolClient::from_env_if_enabled() {
        return Ok(client);
    }

    if let Some((token, account_id)) = source_codex_oauth_credentials(source_headers) {
        return Ok(OpenAiHostedToolClient::from_codex_oauth(token, account_id));
    }

    if let Some(app_handle) = app_handle {
        let codex_state = app_handle.state::<CodexOAuthState>();
        let codex_auth: tokio::sync::RwLockReadGuard<'_, CodexOAuthManager> =
            codex_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"));
        let token_result = match &account_id {
            Some(id) => codex_auth.get_valid_token_for_account(id).await,
            None => codex_auth.get_valid_token().await,
        };
        return match token_result {
            Ok(token) => {
                let resolved_account_id = match &account_id {
                    Some(_) => account_id,
                    None => codex_auth.default_account_id().await,
                };
                Ok(OpenAiHostedToolClient::from_codex_oauth(
                    token,
                    resolved_account_id,
                ))
            }
            Err(err) => Err(format!(
                "OpenAI hosted tool bridge failed to obtain Codex OAuth token: {err}"
            )),
        };
    }

    Err(
        "OpenAI hosted tool bridge requires CCSWITCH_HOSTED_TOOLS_OPENAI_API_KEY or a logged-in Codex OAuth request"
            .to_string(),
    )
}

/// 从当前 Codex 请求头提取 native OAuth 凭据，避免为 hosted tools 单独配置 API Key。
fn source_codex_oauth_credentials(headers: &http::HeaderMap) -> Option<(String, Option<String>)> {
    let authorization = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())?;
    let token = authorization.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        return None;
    }
    let account_id = headers
        .get("chatgpt-account-id")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Some((token.to_string(), account_id))
}

/// 运行 Chat 上游上的 hosted tools 本地工具循环。
///
/// 参数:
/// - `response`: 第一轮 Chat 上游响应，调用方已完成成功响应预处理。
/// - `chat_request`: 本轮 Chat 请求体；函数会追加 assistant/tool messages。
/// - `config`: Codex 原始 hosted tools 的安全配置子集。
/// - `send_chat_request`: 复用 forwarder 已完成认证/代理/超时配置的发送闭包。
/// - `trace_id`: 可选 Codex 路由 trace id，只写脱敏诊断日志。
/// - `force_response_header`: 原始请求可能是流式，强制非流式上游时需要标记给 handler。
/// 返回:
/// - 最终 Chat response，仍交给现有 Chat→Responses 转换器处理。
/// 副作用:
/// - 可能调用 OpenAI hosted tools，并可能向同一第三方 Chat 上游追加多轮请求。
async fn run_hosted_tool_chat_loop<F, Fut>(
    mut response: ProxyResponse,
    chat_request: &mut Value,
    config: &HostedToolLoopConfig,
    client: &Result<OpenAiHostedToolClient, String>,
    mut send_chat_request: F,
    trace_id: Option<&str>,
    force_response_header: bool,
) -> Result<ProxyResponse, ProxyError>
where
    F: FnMut(&Value) -> Fut,
    Fut: std::future::Future<Output = Result<ProxyResponse, ProxyError>>,
{
    let mut loop_executed = false;

    for iteration in 0..=MAX_HOSTED_TOOL_ITERATIONS {
        let (status, mut headers, body_bytes) = read_decoded_proxy_response(response).await?;
        if force_response_header || loop_executed {
            mark_hosted_tool_loop_response(&mut headers);
        }
        if !status.is_success() {
            return Ok(ProxyResponse::buffered(status, headers, body_bytes));
        }

        let chat_response: Value = match serde_json::from_slice(&body_bytes) {
            Ok(value) => value,
            Err(_) => {
                log::warn!("[Codex] Hosted tool loop skipped because Chat response is not JSON");
                return Ok(ProxyResponse::buffered(status, headers, body_bytes));
            }
        };

        let calls = match scan_hosted_tool_calls(&chat_response) {
            HostedToolCallScan::NoToolCalls => {
                return Ok(ProxyResponse::buffered(status, headers, body_bytes));
            }
            HostedToolCallScan::ContainsUnsupportedToolCalls => {
                return Ok(ProxyResponse::buffered(status, headers, body_bytes));
            }
            HostedToolCallScan::OnlyHosted(calls) if calls.is_empty() => {
                return Ok(ProxyResponse::buffered(status, headers, body_bytes));
            }
            HostedToolCallScan::OnlyHosted(calls) => calls,
        };

        if iteration >= MAX_HOSTED_TOOL_ITERATIONS {
            log::warn!(
                "[Codex] Hosted tool loop reached max iterations ({MAX_HOSTED_TOOL_ITERATIONS})"
            );
            return Ok(ProxyResponse::buffered(status, headers, body_bytes));
        }

        let tool_messages = execute_hosted_tool_calls(&calls, config, client, trace_id).await;
        if !append_tool_outputs_to_chat_request(chat_request, &chat_response, tool_messages) {
            log::warn!("[Codex] Hosted tool loop skipped because Chat messages are missing");
            return Ok(ProxyResponse::buffered(status, headers, body_bytes));
        }

        loop_executed = true;
        response = send_chat_request(chat_request).await?;
    }

    unreachable!("hosted tool loop always returns inside bounded iteration")
}

/// 读取并按 content-encoding 解压一个 ProxyResponse。
///
/// 参数:
/// - `response`: 待消费的上游响应。
/// 返回:
/// - HTTP 状态、已清理实体头的 headers，以及明文字节。
/// 副作用:
/// - 消费 response body；如果执行了解压，会移除 content-encoding/content-length。
async fn read_decoded_proxy_response(
    response: ProxyResponse,
) -> Result<(http::StatusCode, http::HeaderMap, Bytes), ProxyError> {
    let status = response.status();
    let mut headers = response.headers().clone();
    let encoding = get_content_encoding(&headers);
    let raw = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await?;
    let decoded = match encoding {
        Some(encoding) => {
            match decompress_body_with_limit(&encoding, &raw, MAX_RESPONSE_BODY_BYTES) {
                Ok(Some(decompressed)) => {
                    strip_proxy_response_entity_headers(&mut headers);
                    Bytes::from(decompressed)
                }
                _ => raw,
            }
        }
        None => raw,
    };

    Ok((status, headers, decoded))
}

/// 给经过 hosted tool loop 的响应打内部标记，供 handler 修正原始流式语义。
fn mark_hosted_tool_loop_response(headers: &mut http::HeaderMap) {
    headers.insert(
        http::HeaderName::from_static(HOSTED_TOOL_LOOP_HEADER),
        http::HeaderValue::from_static("web_search"),
    );
}

/// 移除已经不再可信的实体头。
fn strip_proxy_response_entity_headers(headers: &mut http::HeaderMap) {
    headers.remove(http::header::CONTENT_ENCODING);
    headers.remove(http::header::CONTENT_LENGTH);
    headers.remove(http::header::TRANSFER_ENCODING);
}

/// 判断某个 Codex 客户端私有头是否应该在转发到上游前移除。
///
/// 这个策略只处理 CCSwitchMulti 作为 Codex 本地代理时的上游边界：
/// - 非 Codex app 流量不处理，避免误删其它客户端自定义 header；
/// - 托管 ChatGPT Codex OAuth 是官方后端协议路径，保留内部协商头；
/// - 第三方 OpenAI-compatible / MultiRouter 目标不承诺支持官方私有头，默认剥离。
fn should_retry_without_codex_responses_lite_header(
    app_type: &AppType,
    headers: &http::HeaderMap,
    status: u16,
    body: Option<&str>,
) -> bool {
    matches!(app_type, AppType::Codex)
        && matches!(status, 400 | 404 | 422 | 501)
        && headers.contains_key(http::HeaderName::from_static(
            "x-openai-internal-codex-responses-lite",
        ))
        && body
            .map(|body| {
                body.contains(
                    "This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite",
                )
            })
            .unwrap_or(false)
}

/// 原生 Codex `/responses` 直通与 Claude 的 Responses 转换都能在未产生语义输出
/// 前安全重连。其它端点即使是 streaming，也不能假定遵守 Responses SSE 语义。
fn should_create_responses_stream_reconnector(
    app_type: &AppType,
    endpoint: &str,
    request_is_streaming: bool,
    resolved_claude_api_format: Option<&str>,
) -> bool {
    request_is_streaming
        && (resolved_claude_api_format == Some("openai_responses")
            || (matches!(app_type, AppType::Codex)
                && super::providers::is_codex_responses_endpoint(endpoint)))
}

/// 生成 Codex Responses-Lite fallback 的能力缓存 key。
///
/// 参数:
/// - `provider_id`: 已解析后的 effective provider id，避免不同上游互相污染。
/// - `url`: 实际请求 URL，只保留 scheme/host/port/path，忽略 query 中可能出现的敏感参数。
/// - `model`: 实际请求模型；Lite 支持通常是模型维度能力，不能只按 provider 缓存。
///   返回:
/// - 稳定字符串 key，用于短期负缓存。
fn codex_responses_lite_fallback_key(provider_id: &str, url: &str, model: &str) -> String {
    let upstream_scope = url
        .parse::<http::Uri>()
        .ok()
        .and_then(|uri| {
            let scheme = uri.scheme_str().unwrap_or("http").to_ascii_lowercase();
            let host = uri.host()?.to_ascii_lowercase();
            let port = uri
                .port_u16()
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            Some(format!("{scheme}://{host}{port}{}", uri.path()))
        })
        .unwrap_or_else(|| url.trim().to_ascii_lowercase());
    format!(
        "{}|{}|{}",
        provider_id.trim(),
        upstream_scope,
        model.trim().to_ascii_lowercase()
    )
}

/// 判断 fallback 负缓存条目在指定时间点是否有效。
///
/// 副作用:
/// - 过期条目会被删除，避免缓存随着模型/上游组合不断增长。
fn codex_responses_lite_fallback_active_at(
    fallbacks: &mut HashMap<String, Instant>,
    key: &str,
    now: Instant,
) -> bool {
    match fallbacks.get(key).copied() {
        Some(expires_at) if expires_at > now => true,
        Some(_) => {
            fallbacks.remove(key);
            false
        }
        None => false,
    }
}

/// 发送一次上游请求，不做业务级重试。
///
/// 调用方负责决定是否根据错误体重放请求；这里只封装 reqwest/hyper 两条传输路径，
/// 避免 Responses-Lite fallback 和常规发送逻辑出现分叉。
#[allow(clippy::too_many_arguments)]
async fn send_forwarder_upstream_request(
    method: http::Method,
    url: String,
    target_for_log: String,
    headers: http::HeaderMap,
    extensions: Extensions,
    body_bytes: Vec<u8>,
    timeout: std::time::Duration,
    request_is_streaming: bool,
    non_streaming_timeout: std::time::Duration,
    streaming_first_byte_timeout: std::time::Duration,
    is_socks_proxy: bool,
    preserve_exact_header_case: bool,
    upstream_proxy_url: Option<&str>,
) -> Result<ProxyResponse, ProxyError> {
    if is_socks_proxy || !preserve_exact_header_case {
        log::debug!(
            "[Forwarder] Using pooled reqwest client (preserve_exact_header_case={preserve_exact_header_case}, socks_proxy={is_socks_proxy})"
        );
        let client = super::http_client::get();
        let mut request = client.request(method.clone(), &url);
        if request_is_streaming {
            request = request.timeout(std::time::Duration::from_secs(24 * 60 * 60));
        } else if !non_streaming_timeout.is_zero() {
            request = request.timeout(non_streaming_timeout);
        }
        for (key, value) in &headers {
            request = request.header(key, value);
        }
        let send = async {
            request
                .body(body_bytes)
                .send()
                .await
                .map_err(map_reqwest_send_error)
        };
        let send_result = if request_is_streaming {
            let header_timeout = if streaming_first_byte_timeout.is_zero() {
                timeout
            } else {
                streaming_first_byte_timeout
            };
            if header_timeout.is_zero() {
                send.await
            } else {
                super::response_grace::await_with_response_grace(
                    send,
                    header_timeout,
                    super::response_grace::RESPONSE_PENDING_GRACE,
                    || {
                        ProxyError::ResponsePending(format!(
                            "流式响应首包超时: {}s（上游未返回响应头）",
                            header_timeout.as_secs()
                        ))
                    },
                )
                .await
            }
        } else {
            send.await
        };
        return send_result.map(ProxyResponse::Reqwest);
    }

    let uri: http::Uri = url.parse().map_err(|e| {
        ProxyError::ForwardFailed(format!("Invalid upstream URL ({target_for_log}): {e}"))
    })?;
    super::hyper_client::send_request(
        uri,
        &target_for_log,
        method,
        headers,
        extensions,
        body_bytes,
        timeout,
        upstream_proxy_url,
    )
    .await
}

/// 对“还没有收到上游 HTTP 状态”的传输失败做有界重试。
///
/// 只重试 `ForwardFailed`（连接失败、请求构造失败、hyper/reqwest 发送阶段失败）；
/// `ResponsePending` 表示请求可能已经到达上游，不能重发。每个 attempt 都重新执行
/// 传入的发送闭包，因此重试使用的是同一份已经转换好的 headers/body。
async fn send_upstream_request_with_transport_retry<F, Fut>(
    app_tag: &str,
    mut send: F,
) -> Result<ProxyResponse, ProxyError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<ProxyResponse, ProxyError>>,
{
    let mut attempt = 0usize;
    loop {
        match send().await {
            Ok(response) => return Ok(response),
            Err(error) if attempt < UPSTREAM_TRANSPORT_RETRY_LIMIT => {
                if !matches!(error, ProxyError::ForwardFailed(_)) {
                    return Err(error);
                }
                let backoff = upstream_transport_retry_backoff(attempt);
                log::warn!(
                    "[{app_tag}] 上游传输阶段失败，同一 Provider 重试 {}/{}（等待 {}ms）: {error}",
                    attempt + 1,
                    UPSTREAM_TRANSPORT_RETRY_LIMIT,
                    backoff.as_millis()
                );
                tokio::time::sleep(backoff).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// 对真实上游 HTTP 429 做有界、可等待的同请求重放。
///
/// 这里与 `ResponsePending` 严格分离：只有已经拿到上游明确 HTTP 429 的请求才会
/// 进入本循环；请求是否到达上游仍不确定的 send/header/body timeout 继续禁止重放。
/// HTTP 429 表示服务端拒绝处理当前请求，因此复用完全相同的 headers/body 不会重复
/// 已开始的模型采样或工具调用。明确的订阅/余额耗尽则立即交回外层账号池或路由降级，
/// 避免在同一账号上等待和空转。
async fn send_codex_request_with_rate_limit_retry<F, Fut>(
    app_tag: &str,
    mut send: F,
) -> Result<ProxyResponse, ProxyError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<ProxyResponse, ProxyError>>,
{
    let mut retry_count = 0usize;
    let mut total_delay = Duration::ZERO;

    loop {
        let response = send().await?;
        if response.status() != http::StatusCode::TOO_MANY_REQUESTS {
            return Ok(response);
        }

        let mut response_headers = response.headers().clone();
        let retry_after = parse_retry_after_delay(&response_headers);
        let body_text = read_decoded_error_body(response).await?;
        let terminal_quota = is_terminal_codex_quota_429(body_text.as_deref());

        if terminal_quota || retry_count >= CODEX_RATE_LIMIT_RETRY_LIMIT {
            return Ok(rebuild_consumed_error_response(
                http::StatusCode::TOO_MANY_REQUESTS,
                &mut response_headers,
                body_text,
            ));
        }

        let requested_delay = retry_after.unwrap_or_else(|| codex_rate_limit_backoff(retry_count));
        let remaining_budget = CODEX_RATE_LIMIT_TOTAL_DELAY_BUDGET.saturating_sub(total_delay);
        if remaining_budget.is_zero() {
            return Ok(rebuild_consumed_error_response(
                http::StatusCode::TOO_MANY_REQUESTS,
                &mut response_headers,
                body_text,
            ));
        }
        let delay = requested_delay
            .min(CODEX_RATE_LIMIT_MAX_SINGLE_DELAY)
            .min(remaining_budget);
        log::warn!(
            "[{app_tag}] 上游明确返回 HTTP 429，CCSM 将重发同一 Codex 请求 {}/{}（等待 {}ms，Retry-After={}）",
            retry_count + 1,
            CODEX_RATE_LIMIT_RETRY_LIMIT,
            delay.as_millis(),
            retry_after
                .map(|value| format!("{}ms", value.as_millis()))
                .unwrap_or_else(|| "missing".to_string())
        );
        tokio::time::sleep(delay).await;
        total_delay += delay;
        retry_count += 1;
    }
}

fn parse_retry_after_delay(headers: &http::HeaderMap) -> Option<Duration> {
    let value = headers
        .get(http::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    let retry_at = chrono::DateTime::parse_from_rfc2822(value)
        .ok()?
        .with_timezone(&chrono::Utc);
    let delay = retry_at.signed_duration_since(chrono::Utc::now());
    delay.to_std().ok()
}

fn codex_rate_limit_backoff(retry_count: usize) -> Duration {
    Duration::from_secs(1u64 << retry_count.min(5))
}

fn is_terminal_codex_quota_429(body: Option<&str>) -> bool {
    let Some(body) = body else {
        return false;
    };
    let normalized = body.to_ascii_lowercase();
    [
        "usage_limit_reached",
        "insufficient_quota",
        "billing_hard_limit_reached",
        "the usage limit has been reached",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn rebuild_consumed_error_response(
    status: http::StatusCode,
    headers: &mut http::HeaderMap,
    body: Option<String>,
) -> ProxyResponse {
    headers.remove(http::header::CONTENT_ENCODING);
    headers.remove(http::header::CONTENT_LENGTH);
    ProxyResponse::buffered(
        status,
        headers.clone(),
        Bytes::from(body.unwrap_or_default()),
    )
}

fn upstream_transport_retry_backoff(attempt: usize) -> Duration {
    match attempt {
        0 => Duration::from_millis(200),
        1 => Duration::from_millis(600),
        _ => Duration::from_millis(1500),
    }
}

/// 读取并解压上游错误响应体，保留可读错误摘要给日志、fallback 判断和客户端。
async fn read_decoded_error_body(response: ProxyResponse) -> Result<Option<String>, ProxyError> {
    let encoding = get_content_encoding(response.headers());
    let raw = response.bytes_with_limit(MAX_RESPONSE_BODY_BYTES).await?;
    let decoded = match encoding {
        Some(encoding) => {
            match decompress_body_with_limit(&encoding, &raw, MAX_RESPONSE_BODY_BYTES) {
                Ok(Some(decompressed)) => decompressed,
                _ => raw.to_vec(),
            }
        }
        None => raw.to_vec(),
    };
    Ok(String::from_utf8(decoded).ok())
}

/// 记录上游错误响应。body 只进入摘要，避免把完整 prompt 或大响应写入日志。
fn append_upstream_error_event(
    trace_id: &str,
    session_id: &str,
    request_model: &str,
    provider_id: &str,
    status: u16,
    body_text: Option<&str>,
    request_shape: Option<&str>,
) {
    let mut fields = vec![
        ("trace", trace_id.to_string()),
        ("session", session_id.to_string()),
        ("model", request_model.to_string()),
        ("provider", provider_id.to_string()),
        ("status", status.to_string()),
        (
            "body_summary",
            body_text
                .map(summarize_upstream_body)
                .unwrap_or_else(|| "<empty>".to_string()),
        ),
    ];
    if let Some(shape) = request_shape {
        fields.push(("request_shape", shape.to_string()));
    }
    super::codex_router_log::append_event("upstream_error", &fields);
}

/// 识别会触发上游 Responses-Lite 分支的 Codex 内部请求头。
#[allow(dead_code)]
fn is_codex_responses_lite_header(name: &http::HeaderName) -> bool {
    name.as_str()
        .eq_ignore_ascii_case("x-openai-internal-codex-responses-lite")
}

fn headers_contain_proxy_placeholder(headers: &http::HeaderMap) -> bool {
    headers.values().any(|value| {
        value
            .to_str()
            .map(|value| value.contains(PROXY_AUTH_PLACEHOLDER))
            .unwrap_or(false)
    })
}

fn should_preserve_exact_header_case(
    adapter_name: &str,
    provider: &Provider,
    resolved_claude_api_format: Option<&str>,
    is_copilot: bool,
) -> bool {
    if matches!(adapter_name, "Codex" | "Gemini") {
        return false;
    }

    if is_copilot || provider.is_codex_oauth() || provider.is_xai_oauth() {
        return false;
    }

    matches!(resolved_claude_api_format, None | Some("anthropic"))
}

/// 判断本次请求是否是 ChatGPT Codex 官方后端的 Responses 透传路径。
///
/// 参数:
/// - `app_type`: 当前客户端应用类型，只有 Codex Desktop/CLI 请求需要该兼容层。
/// - `provider`: 已经由 MultiRouter 或顶层切换解析后的 effective provider。
/// - `url`: forwarder 最终要访问的上游 URL。
/// - `needs_transform`: 是否已经走了 Claude/Anthropic 转换管线。
/// - `codex_responses_to_chat`: 是否已经被改写到 Chat Completions 上游。
/// - `codex_responses_to_messages`: 是否已经被改写到 Messages 上游。
///   返回:
/// - `true` 表示需要在透传前投影成 ChatGPT Codex backend 接受的回放形态。
///   副作用:
/// - 无。该函数只读入参，用来把修复范围限制在 managed Codex OAuth 或内置
///   native-auth OpenAI Official 到 ChatGPT Codex Responses 的真实 transport。
fn should_normalize_codex_oauth_responses_passthrough_body(
    app_type: &AppType,
    provider: &Provider,
    url: &str,
    needs_transform: bool,
    codex_responses_to_chat: bool,
    codex_responses_to_messages: bool,
) -> bool {
    matches!(app_type, AppType::Codex)
        && (provider.is_codex_oauth() || super::providers::is_codex_official_provider(provider))
        && !needs_transform
        && !codex_responses_to_chat
        && !codex_responses_to_messages
        && is_chatgpt_codex_responses_upstream_url(url)
}

/// 判断是否需要规整第三方 Responses 透传请求中的 Codex 控制消息。
///
/// 参数:
/// - `app_type`: 当前客户端应用类型，只有 Codex 的 Responses 历史会携带这类角色。
/// - `provider`: 已经由 MultiRouter 解析后的 effective provider。
/// - `endpoint`: 本地代理收到的 endpoint。
/// - `needs_transform`: 是否已进入其它格式转换管线。
/// - `codex_responses_to_chat`: 是否已转成 Chat Completions。
/// - `codex_responses_to_messages`: 是否已转成 Messages。
///   返回:
/// - `true` 表示该请求会原生透传到第三方 Responses API，需要把 developer/system
///   input item 提升到 instructions。
///   副作用:
/// - 无。
fn should_normalize_codex_responses_passthrough_control_messages(
    app_type: &AppType,
    provider: &Provider,
    endpoint: &str,
    needs_transform: bool,
    codex_responses_to_chat: bool,
    codex_responses_to_messages: bool,
) -> bool {
    matches!(app_type, AppType::Codex)
        && !provider.is_codex_oauth()
        && !needs_transform
        && !codex_responses_to_chat
        && !codex_responses_to_messages
        && super::providers::is_codex_responses_endpoint(endpoint)
}

fn normalize_lm_studio_responses_request(
    app_type: &AppType,
    provider: &Provider,
    endpoint: &str,
    native_responses: bool,
    request_body: Value,
) -> Value {
    if !matches!(app_type, AppType::Codex)
        || !native_responses
        || !super::providers::is_codex_responses_endpoint(endpoint)
        || !is_lm_studio_provider(provider)
    {
        return request_body;
    }

    let Value::Object(mut body) = request_body else {
        return request_body;
    };
    let text = body
        .entry("text".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if let Value::Object(text) = text {
        text.entry("format".to_string())
            .or_insert_with(|| serde_json::json!({"type": "text"}));
    }

    Value::Object(body)
}

fn is_lm_studio_provider(provider: &Provider) -> bool {
    if provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        .is_some_and(|provider_type| provider_type.eq_ignore_ascii_case("lmstudio"))
    {
        return true;
    }

    [&provider.id, &provider.name].into_iter().any(|identity| {
        identity
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect::<String>()
            .eq_ignore_ascii_case("lmstudio")
    })
}

/// 判断 URL 是否指向 ChatGPT 的 Codex Responses backend。
///
/// 参数:
/// - `url`: 已拼接完成的上游 URL。
///   返回:
/// - `true` 表示 host/path 是 `chatgpt.com/backend-api/codex/responses` 系列。
///   副作用:
/// - 无。解析失败时保守返回 `false`，避免影响普通 OpenAI/兼容厂商。
fn is_chatgpt_codex_responses_upstream_url(url: &str) -> bool {
    let Ok(uri) = url.parse::<http::Uri>() else {
        return false;
    };

    let Some(host) = uri.host().map(str::to_ascii_lowercase) else {
        return false;
    };
    if host != "chatgpt.com" {
        return false;
    }

    matches!(
        uri.path().trim_end_matches('/'),
        "/backend-api/codex/responses" | "/backend-api/codex/responses/compact"
    )
}

fn is_streaming_request(endpoint: &str, body: &Value, headers: &axum::http::HeaderMap) -> bool {
    if body
        .get("stream")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return true;
    }

    if endpoint.contains("streamGenerateContent") || endpoint.contains("alt=sse") {
        return true;
    }

    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false)
}

#[cfg(test)]
fn should_force_identity_encoding(
    endpoint: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
) -> bool {
    is_streaming_request(endpoint, body, headers)
}

fn map_reqwest_send_error(error: reqwest::Error) -> ProxyError {
    // `reqwest::Error::to_string()` 常常只留下 "error sending request"；真正能区分
    // TLS/HTTP2/连接复用/对端断开的原因在 source 链。先移除 URL，再展开 source 链，
    // 既保留可操作的网络诊断，又不会把带 query 的上游地址写进 router 日志或客户端错误。
    let error_without_url = error.without_url();
    map_reqwest_send_error_class(
        error_without_url.is_connect(),
        error_without_url.is_timeout(),
        error_without_url.is_builder(),
        error_without_url.is_request(),
        error_chain_message(&error_without_url),
    )
}

fn map_reqwest_send_error_class(
    is_connect: bool,
    is_timeout: bool,
    is_builder: bool,
    _is_request: bool,
    detail: String,
) -> ProxyError {
    if is_connect {
        // 连接阶段失败，包括 DNS/TCP/TLS 和 connect_timeout：请求尚未发出，可以安全重试。
        ProxyError::ForwardFailed(format!("上游连接失败: {detail}"))
    } else if is_timeout {
        // 超时发生在请求发出后，无法确定上游是否已经开始处理，不能自动重发。
        ProxyError::ResponsePending(format!("上游响应等待超时（请求可能已在处理中）: {detail}"))
    } else if is_builder {
        // 请求构造阶段失败，没有发出任何字节。
        ProxyError::ForwardFailed(format!("上游请求构造失败: {detail}"))
    } else {
        // 请求体发送/响应读取阶段失败：可能已经部分或全部发出。
        ProxyError::ResponsePending(format!(
            "上游请求发送或响应读取中断（请求可能已在处理中）: {detail}"
        ))
    }
}

fn summarize_text_for_log(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();

    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let truncated: String = trimmed.chars().take(max_chars).collect();
    let truncated = truncated.trim_end();
    format!("{truncated}...")
}

fn apply_local_proxy_body_overrides(
    body: &mut Value,
    overrides: &LocalProxyRequestOverrides,
) -> bool {
    let Some(override_body) = overrides.body.as_ref() else {
        return false;
    };

    if !override_body.is_object() {
        log::warn!("[LocalProxyOverrides] Ignoring body override because it is not an object");
        return false;
    }

    merge_json_override(body, override_body)
}

fn merge_json_override(target: &mut Value, patch: &Value) -> bool {
    merge_json_override_inner(target, patch, true)
}

fn merge_json_override_inner(target: &mut Value, patch: &Value, is_top_level: bool) -> bool {
    match (target, patch) {
        (Value::Object(target_map), Value::Object(patch_map)) => {
            let mut changed = false;
            for (key, patch_value) in patch_map {
                if is_top_level && key == "stream" {
                    log::warn!(
                        "[LocalProxyOverrides] Ignoring body override for protected field: stream"
                    );
                    continue;
                }
                match target_map.get_mut(key) {
                    Some(target_value) => {
                        changed |= merge_json_override_inner(target_value, patch_value, false);
                    }
                    None => {
                        target_map.insert(key.clone(), patch_value.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        (target_value, patch_value) => {
            if target_value == patch_value {
                false
            } else {
                *target_value = patch_value.clone();
                true
            }
        }
    }
}

fn apply_local_proxy_header_overrides(
    headers: &mut http::HeaderMap,
    overrides: Option<&LocalProxyRequestOverrides>,
    is_copilot: bool,
) {
    if is_copilot {
        return;
    }

    let Some(header_overrides) = overrides.map(|overrides| &overrides.headers) else {
        return;
    };

    for (raw_name, raw_value) in header_overrides {
        let header_name = raw_name.trim().to_ascii_lowercase();
        if header_name.is_empty() {
            log::warn!("[LocalProxyOverrides] Ignoring header override with empty name");
            continue;
        }

        let Ok(name) = http::HeaderName::from_bytes(header_name.as_bytes()) else {
            log::warn!("[LocalProxyOverrides] Ignoring invalid header override name: {raw_name}");
            continue;
        };

        if is_protected_local_proxy_override_header(&name) {
            log::debug!(
                "[LocalProxyOverrides] Ignoring protected header override: {}",
                name.as_str()
            );
            continue;
        }

        let Ok(value) = http::HeaderValue::from_str(raw_value) else {
            log::warn!(
                "[LocalProxyOverrides] Ignoring invalid header override value for {}",
                name.as_str()
            );
            continue;
        };

        headers.insert(name, value);
    }
}

fn is_protected_local_proxy_override_header(name: &http::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "upgrade"
            | "accept-encoding"
            | "content-type"
            | "authorization"
            | "x-api-key"
            | "x-goog-api-key"
            | "chatgpt-account-id"
            | "session_id"
            | "x-client-request-id"
            | "x-codex-window-id"
            | "x-forwarded-host"
            | "x-forwarded-port"
            | "x-forwarded-proto"
            | "forwarded"
            | "cf-connecting-ip"
            | "cf-ipcountry"
            | "cf-ray"
            | "cf-visitor"
            | "true-client-ip"
            | "fastly-client-ip"
            | "x-azure-clientip"
            | "x-azure-fdid"
            | "x-azure-ref"
            | "akamai-origin-hop"
            | "x-akamai-config-log-detail"
            | "x-request-id"
            | "x-correlation-id"
            | "x-trace-id"
            | "x-amzn-trace-id"
            | "x-b3-traceid"
            | "x-b3-spanid"
            | "x-b3-parentspanid"
            | "x-b3-sampled"
            | "traceparent"
            | "tracestate"
    )
}

fn prepare_upstream_request_body(request_body: Value) -> Value {
    canonicalize_value(filter_private_params_with_whitelist(request_body, &[]))
}

fn should_zstd_compress_codex_official_upstream(
    app_type: &AppType,
    method: &http::Method,
    upstream_url: &str,
    official_auth: bool,
    transformed: bool,
) -> bool {
    matches!(app_type, AppType::Codex)
        && method == http::Method::POST
        && official_auth
        && !transformed
        && is_chatgpt_codex_responses_upstream_url(upstream_url)
}

/// Encode the final JSON entity exactly as the official Codex transport does.
///
/// The local handler must decode the incoming entity before it can normalize or
/// route the JSON. Official Responses requests are compressed again only after
/// those transformations are complete, so retries can clone one immutable wire
/// body and `content-encoding` always describes the bytes actually sent.
fn encode_codex_official_upstream_body(
    headers: &mut http::HeaderMap,
    body: Vec<u8>,
    enabled: bool,
) -> Result<Vec<u8>, ProxyError> {
    if !enabled || body.is_empty() {
        return Ok(body);
    }

    let encoded = zstd::stream::encode_all(std::io::Cursor::new(body), 0).map_err(|error| {
        ProxyError::Internal(format!("Failed to compress request body: {error}"))
    })?;
    headers.insert(
        http::header::CONTENT_ENCODING,
        http::HeaderValue::from_static("zstd"),
    );
    headers.remove(http::header::CONTENT_LENGTH);
    headers.remove(http::header::TRANSFER_ENCODING);
    Ok(encoded)
}

/// 生成 Codex Responses->Chat 出站请求的脱敏形态摘要。
///
/// 该摘要只记录顶层字段名、对象/数组形态和工具计数，不记录消息正文、工具参数、
/// API Key 或任意用户 prompt。上游返回空 400 时可用它定位严格 Chat 接口拒绝的字段组合。
fn summarize_codex_chat_request_shape(body: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(object) = body.as_object() {
        let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        parts.push(format!("top_keys=[{}]", keys.join(",")));
    } else {
        parts.push(format!("body={}", value_for_log(body)));
    }

    parts.push(format!(
        "messages={}",
        body.get("messages")
            .and_then(Value::as_array)
            .map(|values| values.len().to_string())
            .unwrap_or_else(|| "absent".to_string())
    ));

    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let mut types = tools
            .iter()
            .filter_map(|tool| tool.get("type").and_then(Value::as_str))
            .collect::<Vec<_>>();
        types.sort_unstable();
        types.dedup();
        parts.push(format!("tools={}", tools.len()));
        parts.push(format!("tool_types=[{}]", types.join(",")));
    } else {
        parts.push("tools=absent".to_string());
    }

    for key in [
        "tool_choice",
        "parallel_tool_calls",
        "metadata",
        "service_tier",
        "stream_options",
        "response_format",
        "max_tokens",
        "max_completion_tokens",
        "max_output_tokens",
        "reasoning_effort",
        "enable_thinking",
        "reasoning",
    ] {
        parts.push(format!(
            "{key}={}",
            body.get(key)
                .map(value_for_shape_log)
                .unwrap_or_else(|| "absent".to_string())
        ));
    }

    parts.push(format!(
        "thinking={}",
        body.get("thinking")
            .map(value_for_shape_log)
            .unwrap_or_else(|| "absent".to_string())
    ));

    parts.join(";")
}

/// 把 JSON 值压缩成不含正文内容的形态描述。
fn value_for_shape_log(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
            keys.sort_unstable();
            format!("object(keys=[{}])", keys.join(","))
        }
        Value::Array(values) => format!("array(len={})", values.len()),
        Value::Bool(_) => "bool".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Null => "null".to_string(),
    }
}

/// 构建本次转发要尝试的 provider 列表。
///
/// 每个 Codex MultiRouter 在这里仅展开成当前模型实际命中的 route provider；外层
/// `providers` 中显式配置的其它 provider 仍保留正常故障转移。route provider 携带父
/// router 的 id/name，`forward()` 仍能保持日志、状态页和用量归因的外层身份。
fn build_forward_attempt_providers_preserving_codex_router_context(
    app_type: &AppType,
    providers: &[Provider],
    body: &Value,
) -> Vec<Provider> {
    if !matches!(app_type, AppType::Codex) {
        return providers.to_vec();
    }

    let mut expanded = Vec::new();
    for provider in providers {
        if codex_provider_has_routing_config(provider)
            && provider
                .settings_config
                .get("codexResolvedRouteId")
                .is_none()
        {
            let routes = super::providers::resolve_codex_model_routed_providers(provider, body);
            if routes.is_empty() {
                expanded.push(provider.clone());
            } else {
                expanded.extend(routes);
            }
        } else {
            expanded.push(provider.clone());
        }
    }
    expanded
}

/// Replace a compaction request's previous model with the thread's current model.
///
/// Codex may run pre-turn compaction against the model used before the switch;
/// when the state DB knows the new model, use it as the routing truth so the
/// compaction follows the model the user actually switched to.
fn apply_codex_compaction_current_model(body: &mut Value, current_model: Option<&str>) -> bool {
    let Some(current_model) = current_model.filter(|model| !model.is_empty()) else {
        return false;
    };
    let request_model = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if request_model.is_none_or(|model| !model.eq_ignore_ascii_case(current_model)) {
        body["model"] = Value::String(current_model.to_string());
        true
    } else {
        false
    }
}

fn log_prompt_cache_trace(
    app_type: &AppType,
    provider: &Provider,
    endpoint: &str,
    api_format: Option<&str>,
    body: &Value,
    session_client_provided: bool,
) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }

    let prompt_cache_key = body
        .get("prompt_cache_key")
        .and_then(|value| value.as_str())
        .map(|key| format!("present(len={})", key.len()))
        .unwrap_or_else(|| "absent".to_string());
    let store = body
        .get("store")
        .map(value_for_log)
        .unwrap_or_else(|| "absent".to_string());
    let stream = body
        .get("stream")
        .map(value_for_log)
        .unwrap_or_else(|| "absent".to_string());
    let cache_controls = cache_control_summary(body);

    log::debug!(
        "[CacheTrace] app={}, provider={}, endpoint={}, api_format={}, session_client_provided={}, prompt_cache_key={}, store={}, stream={}, instructions_hash={}, system_hash={}, tools_hash={}, input_hash={}, messages_hash={}, include_hash={}, cache_controls={}, body_hash={}",
        app_type.as_str(),
        provider.id,
        // Gemini 的 endpoint 带 ?key=<API_KEY>；脱敏剥掉 query 再落盘。
        crate::redact_url_for_log(endpoint),
        api_format.unwrap_or("native"),
        session_client_provided,
        prompt_cache_key,
        store,
        stream,
        short_value_hash(body.get("instructions")),
        short_value_hash(body.get("system")),
        short_value_hash(body.get("tools")),
        short_value_hash(body.get("input")),
        short_value_hash(body.get("messages")),
        short_value_hash(body.get("include")),
        cache_controls,
        short_value_hash(Some(body)),
    );
}

fn cache_control_summary(value: &Value) -> String {
    fn walk(value: &Value, count: &mut usize, ttls: &mut std::collections::BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                if let Some(cache_control) = object.get("cache_control") {
                    *count += 1;
                    let ttl = cache_control
                        .get("ttl")
                        .and_then(Value::as_str)
                        .unwrap_or("default");
                    ttls.insert(ttl.to_string());
                }
                for child in object.values() {
                    walk(child, count, ttls);
                }
            }
            Value::Array(items) => {
                for child in items {
                    walk(child, count, ttls);
                }
            }
            _ => {}
        }
    }

    let mut count = 0;
    let mut ttls = std::collections::BTreeSet::new();
    walk(value, &mut count, &mut ttls);
    format!(
        "count={count},ttls={}",
        if ttls.is_empty() {
            "none".to_string()
        } else {
            ttls.into_iter().collect::<Vec<_>>().join("|")
        }
    )
}

fn value_for_log(value: &Value) -> String {
    match value {
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Null => "null".to_string(),
        Value::Array(values) => format!("array(len={})", values.len()),
        Value::Object(values) => format!("object(len={})", values.len()),
    }
}

/// 判断 Codex provider 是否声明了本地模型路由。
///
/// 这个标记用于区分普通 Codex provider 与“外层 bucket router”。只有 router
/// provider 发生 route miss 时，才需要额外防止回退到自身本地代理地址。
fn codex_provider_has_routing_config(provider: &Provider) -> bool {
    provider.settings_config.get("codexRouting").is_some()
        || provider.settings_config.get("codexModelRoutes").is_some()
        || provider.settings_config.get("modelRoutes").is_some()
}

fn should_make_codex_v2_agents_plaintext(
    app_type: &AppType,
    router_provider: &Provider,
    official_oauth_request: bool,
) -> bool {
    matches!(app_type, AppType::Codex)
        && official_oauth_request
        && super::providers::codex_multirouter_needs_plaintext_v2_collaboration(router_provider)
}

fn should_project_codex_agent_messages_for_provider(
    app_type: &AppType,
    provider: &Provider,
    endpoint: &str,
) -> bool {
    // Managed Codex OAuth routes are OpenAI-owned even when their request-local
    // route id/category does not match the built-in `codex-official` seed.
    matches!(app_type, AppType::Codex)
        && super::providers::is_codex_responses_endpoint(endpoint)
        && !provider.is_codex_oauth()
        && !super::providers::is_codex_official_provider(provider)
}

/// 判断 effective base_url 是否精确指向当前 CC Switch 代理入口。
///
/// 只匹配当前监听端口，不能把其它 loopback 服务一概拒绝：用户可能合法地把
/// route 指向本机 vLLM。端口由 server 启动时写入 http_client 的共享状态。
fn codex_base_url_points_to_local_proxy(base_url: &str) -> bool {
    super::http_client::proxy_points_to_loopback(base_url)
}

/// 在任何网络发送之前拒绝 Codex 有效上游回到当前代理监听端口。
///
/// 这个边界不依赖 route 是否命中；命中但未物化 target provider 正是本次事故的
/// 触发方式。返回 `InvalidRequest` 后 retry 层按非重试异常记录 failed_requests，
/// 不会得到可被误计为成功的递归 HTTP 响应壳。
fn reject_codex_effective_local_proxy_upstream(
    app_type: &AppType,
    base_url: &str,
    request_context: &str,
) -> Result<(), ProxyError> {
    if matches!(app_type, AppType::Codex) && codex_base_url_points_to_local_proxy(base_url) {
        return Err(ProxyError::InvalidRequest(format!(
            "Codex effective upstream for {request_context} points to the running local proxy ({base_url}). Refusing to forward recursively; repair the route target provider or switch away from the router provider."
        )));
    }
    Ok(())
}

/// 为未知 `/v1/*` raw passthrough 解析 Codex MultiRouter route。
///
/// 规则和常规 Responses 转换路径不同：显式模型命中的 route 仍然优先；除此之外
/// 一律选择 official/Codex OAuth route，把未知 endpoint 当作 GPT App 的原生官方请求。
/// 这里故意不使用 defaultRouteId，避免图片、音频、文件等 OpenAI 原生 endpoint 被
/// defaultRouteId 指向的 DeepSeek/Qwen 文本 provider 吞掉。
pub(crate) fn resolve_codex_raw_passthrough_route_provider(
    provider: &Provider,
    route_body: &Value,
) -> Option<Provider> {
    let model_routed = route_body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .and_then(|_| super::providers::resolve_codex_model_routed_provider(provider, route_body));

    if model_routed
        .as_ref()
        .is_some_and(codex_raw_route_provider_matched_request_model)
    {
        return model_routed;
    }

    resolve_codex_raw_official_route_by_identity(provider)
}

/// 判断 route provider 是否由请求模型显式命中，而不是 defaultRouteId 兜底。
fn codex_raw_route_provider_matched_request_model(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("codexResolvedRouteMatched")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// 按 route 身份扫描 official/Codex OAuth route，用作未知 OpenAI endpoint 的原生兜底。
fn resolve_codex_raw_official_route_by_identity(provider: &Provider) -> Option<Provider> {
    codex_raw_passthrough_routes(provider)
        .into_iter()
        .filter(|route| codex_raw_route_is_enabled(route))
        .find(|route| codex_raw_route_is_official(route))
        .map(|route| build_codex_raw_endpoint_fallback_provider(provider, route))
}

/// 构造 endpoint 级兜底 provider，并标记它不是请求模型真实匹配。
fn build_codex_raw_endpoint_fallback_provider(provider: &Provider, route: &Value) -> Provider {
    let mut route_provider =
        super::providers::build_codex_route_probe_provider(provider, route, None);
    if let Some(settings) = route_provider.settings_config.as_object_mut() {
        settings.insert("codexResolvedRouteMatched".to_string(), Value::Bool(false));
    }
    route_provider
}

/// 提取新旧 schema 下可参与 raw passthrough 的 route 列表。
fn codex_raw_passthrough_routes(provider: &Provider) -> Vec<&Value> {
    if let Some(routing) = provider.settings_config.get("codexRouting") {
        if let Some(routes) = routing.as_array() {
            return routes.iter().collect();
        }
        if routing
            .get("enabled")
            .and_then(Value::as_bool)
            .is_some_and(|enabled| !enabled)
        {
            return Vec::new();
        }
        return routing
            .get("routes")
            .and_then(Value::as_array)
            .map(|routes| routes.iter().collect())
            .unwrap_or_default();
    }

    provider
        .settings_config
        .get("codexModelRoutes")
        .or_else(|| provider.settings_config.get("modelRoutes"))
        .and_then(Value::as_array)
        .map(|routes| routes.iter().collect())
        .unwrap_or_default()
}

/// 判断 route 是否启用；旧配置缺省按启用处理。
fn codex_raw_route_is_enabled(route: &Value) -> bool {
    route
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// 识别 official/Codex OAuth route 身份，而不是依赖模型名。
fn codex_raw_route_is_official(route: &Value) -> bool {
    let target_provider = codex_raw_route_string(
        route,
        &[
            "targetProviderId",
            "target_provider_id",
            "providerId",
            "provider_id",
            "provider",
        ],
    )
    .map(|value| value.to_ascii_lowercase());
    if target_provider.as_deref().is_some_and(|id| {
        matches!(
            id,
            "codex-official" | "openai" | "openai-official" | "openai official"
        )
    }) {
        return true;
    }

    if codex_raw_route_auth_source(route).is_some_and(|source| {
        matches!(
            source,
            "managed_codex_oauth" | "managed_account" | "chatgpt"
        )
    }) {
        return true;
    }

    if codex_raw_route_string(route, &["providerType", "provider_type"])
        .is_some_and(|value| value.eq_ignore_ascii_case("codex_oauth"))
    {
        return true;
    }

    if codex_raw_route_string(route, &["auth_mode", "authMode"])
        .is_some_and(|value| value.eq_ignore_ascii_case("chatgpt"))
    {
        return true;
    }

    codex_raw_route_string(route, &["baseUrl", "baseURL", "base_url"]).is_some_and(|value| {
        value
            .to_ascii_lowercase()
            .contains("chatgpt.com/backend-api/codex")
    })
}

/// 从 route/upstream 两层读取字符串字段。
fn codex_raw_route_string<'a>(route: &'a Value, keys: &[&str]) -> Option<&'a str> {
    let upstream = route.get("upstream").unwrap_or(route);
    keys.iter()
        .find_map(|key| upstream.get(*key).or_else(|| route.get(*key)))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// 读取 route 的 auth.source，并兼容 managed account 的 authProvider。
fn codex_raw_route_auth_source(route: &Value) -> Option<&str> {
    let upstream = route.get("upstream").unwrap_or(route);
    let auth = upstream.get("auth").or_else(|| route.get("auth"))?;
    if auth
        .get("authProvider")
        .or_else(|| auth.get("auth_provider"))
        .and_then(Value::as_str)
        .is_some_and(|provider| provider.eq_ignore_ascii_case("codex_oauth"))
    {
        return Some("managed_codex_oauth");
    }
    auth.get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|source| !source.is_empty())
}

/// 重建 raw passthrough 的上游请求头。
///
/// 客户端原始 body 可以透传，但认证、Host、Content-Length 和转发链路头必须由
/// CCSwitchMulti 重建，避免把本地 external API key、旧 host 或代理 hop-by-hop 头发到上游。
fn build_raw_passthrough_headers(
    source_headers: &http::HeaderMap,
    auth_headers: &[(http::HeaderName, http::HeaderValue)],
    upstream_host: Option<&str>,
    custom_user_agent: Option<&http::HeaderValue>,
) -> http::HeaderMap {
    let mut headers = http::HeaderMap::new();
    let mut saw_user_agent = false;

    for (name, value) in source_headers.iter() {
        if raw_passthrough_header_should_skip(name) {
            continue;
        }
        if *name == http::header::USER_AGENT {
            saw_user_agent = true;
            if custom_user_agent.is_some() {
                continue;
            }
        }
        headers.append(name.clone(), value.clone());
    }

    for (name, value) in auth_headers {
        headers.append(name.clone(), value.clone());
    }

    if custom_user_agent.is_some() || !saw_user_agent {
        if let Some(user_agent) = custom_user_agent {
            headers.insert(http::header::USER_AGENT, user_agent.clone());
        }
    }

    if let Some(host) = upstream_host {
        if let Ok(host) = http::HeaderValue::from_str(host) {
            headers.insert(http::header::HOST, host);
        }
    }

    headers
}

/// 判断 raw passthrough 是否需要丢弃客户端入站头。
fn raw_passthrough_header_should_skip(name: &http::HeaderName) -> bool {
    if outbound_header_is_local_only(name) {
        return true;
    }
    let lower = name.as_str().to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "host"
            | "content-length"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "trailers"
            | "upgrade"
            | "authorization"
            | "x-api-key"
            | "api-key"
            | "x-goog-api-key"
            | "chatgpt-account-id"
            | "x-cc-switch-external-openai-api"
            | "x-forwarded-host"
            | "x-forwarded-port"
            | "x-forwarded-proto"
            | "forwarded"
            | "cf-connecting-ip"
            | "cf-ipcountry"
            | "cf-ray"
            | "cf-visitor"
            | "true-client-ip"
            | "fastly-client-ip"
            | "x-azure-clientip"
            | "x-azure-fdid"
            | "x-azure-ref"
            | "akamai-origin-hop"
            | "x-akamai-config-log-detail"
            | "x-request-id"
            | "x-correlation-id"
            | "x-trace-id"
            | "x-amzn-trace-id"
            | "x-b3-traceid"
            | "x-b3-spanid"
            | "x-b3-parentspanid"
            | "x-b3-sampled"
            | "traceparent"
            | "tracestate"
    ) || lower.starts_with("x-forwarded-")
        || lower.starts_with("cf-")
}

/// CCSM 自己的控制头只允许存在于客户端到本地代理这一跳。
fn outbound_header_is_local_only(name: &http::HeaderName) -> bool {
    name.as_str()
        .to_ascii_lowercase()
        .starts_with("x-cc-switch-")
}

/// 判断 raw passthrough 是否应按流式响应处理。
fn raw_passthrough_request_is_streaming(route_body: &Value, headers: &http::HeaderMap) -> bool {
    route_body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || headers
            .get(http::header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .map(|accept| accept.to_ascii_lowercase().contains("text/event-stream"))
            .unwrap_or(false)
}

/// 归一化 Codex GPT-Live 本地入口，返回 `/v1/live` 或 `/v1/live/{call_id}`。
fn codex_realtime_live_path(endpoint: &str) -> Option<String> {
    let (path, _query) = split_endpoint_and_query(endpoint);
    let path = path.trim_end_matches('/');
    let canonical = if matches!(
        path,
        "/live" | "/v1/live" | "/v1/v1/live" | "/codex/v1/live"
    ) {
        Some("/v1/live".to_string())
    } else if let Some(rest) = path.strip_prefix("/live/") {
        Some(format!("/v1/live/{rest}"))
    } else if let Some(rest) = path.strip_prefix("/v1/v1/live/") {
        Some(format!("/v1/live/{rest}"))
    } else if let Some(rest) = path.strip_prefix("/codex/v1/live/") {
        Some(format!("/v1/live/{rest}"))
    } else if path.starts_with("/v1/live/") {
        Some(path.to_string())
    } else {
        None
    };
    canonical.filter(|path| path == "/v1/live" || path.starts_with("/v1/live/"))
}

/// 只有 GPT-Live call-create 的精确入口需要转换 HTTP 请求形态。
fn codex_realtime_live_call_path(path: &str) -> bool {
    codex_realtime_live_path(path).as_deref() == Some("/v1/live")
}

/// 从 Codex 本地 `/v1/live` multipart 请求中提取 `sdp` 与 `session`。
///
/// Codex 以 `api.openai.com/v1` 形态向本地代理发起 call-create 时，body 是
/// `codex-realtime-call-boundary` multipart；官方 ChatGPT Codex backend 形态要求
/// JSON `{ sdp, session }`，因此这里只在进入 backend 上游前做转换。
fn codex_realtime_multipart_payload(
    body: &[u8],
    content_type: &str,
) -> Result<(String, Value), ProxyError> {
    let boundary = content_type
        .split(';')
        .find_map(|part| {
            let part = part.trim();
            part.strip_prefix("boundary=")
        })
        .map(|boundary| boundary.trim_matches('"').to_string())
        .filter(|boundary| !boundary.is_empty())
        .ok_or_else(|| {
            ProxyError::InvalidRequest(
                "Codex realtime call requires multipart/form-data with a boundary".to_string(),
            )
        })?;
    let text = std::str::from_utf8(body).map_err(|_| {
        ProxyError::InvalidRequest(
            "Codex realtime call body must be UTF-8 multipart data".to_string(),
        )
    })?;
    let delimiter = format!("--{boundary}");
    let mut sdp = None;
    let mut session = None;
    for raw_part in text.split(&delimiter).skip(1) {
        let part = raw_part
            .strip_prefix("\r\n")
            .or_else(|| raw_part.strip_prefix('\n'))
            .unwrap_or(raw_part);
        let part = part.strip_suffix("--").unwrap_or(part);
        let Some((header_block, value)) = part.split_once("\r\n\r\n") else {
            continue;
        };
        let value = value.strip_suffix("\r\n").unwrap_or(value);
        match codex_realtime_multipart_field_name(header_block).as_deref() {
            Some("sdp") => sdp = Some(value.to_string()),
            Some("session") => {
                session = Some(serde_json::from_str(value).map_err(|err| {
                    ProxyError::InvalidRequest(format!(
                        "Codex realtime session part is not valid JSON: {err}"
                    ))
                })?);
            }
            _ => {}
        }
    }
    let sdp = sdp.ok_or_else(|| {
        ProxyError::InvalidRequest("Codex realtime call is missing the sdp part".to_string())
    })?;
    let session = session.ok_or_else(|| {
        ProxyError::InvalidRequest("Codex realtime call is missing the session part".to_string())
    })?;
    Ok((sdp, session))
}

fn codex_realtime_multipart_field_name(headers: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        if !line
            .to_ascii_lowercase()
            .starts_with("content-disposition:")
        {
            return None;
        }
        let needle = "name=\"";
        let start = line.to_ascii_lowercase().find(needle)? + needle.len();
        let end = line[start..].find('"')? + start;
        Some(line[start..end].to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::LocalProxyRequestOverrides;
    use axum::http::header::{HeaderValue, ACCEPT};
    use axum::http::HeaderMap;
    use bytes::Bytes;
    use futures::SinkExt;
    use http::StatusCode;
    use serde_json::json;

    fn test_provider_with_type(provider_type: Option<&str>) -> Provider {
        Provider {
            id: "provider-1".to_string(),
            name: "Provider 1".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: provider_type.map(|value| crate::provider::ProviderMeta {
                provider_type: Some(value.to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn test_codex_official_provider() -> Provider {
        let mut provider = test_provider_with_type(None);
        provider.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        provider.category = Some("official".to_string());
        provider
    }

    fn test_codex_pool_candidate(account_id: &str, generation: u64) -> Provider {
        let mut provider = test_provider_with_type(None);
        provider.id = format!("codex-router::account::{account_id}");
        provider.category = Some("official".to_string());
        provider.settings_config[CODEX_ACCOUNT_POOL_ENABLED] = Value::Bool(true);
        provider.settings_config["codexPoolAccountId"] = Value::String(account_id.to_string());
        provider.settings_config["codexPoolCredentialGeneration"] =
            Value::Number(generation.into());
        provider
    }

    #[test]
    fn codex_account_pool_requires_an_explicit_router_marker() {
        let native = test_codex_official_provider();
        assert!(!provider_requests_codex_account_pool(&native));

        let mut pooled = test_codex_official_provider();
        pooled.settings_config[CODEX_ACCOUNT_POOL_ENABLED] = Value::Bool(true);
        assert!(provider_requests_codex_account_pool(&pooled));
    }

    #[test]
    fn codex_pool_attempt_classification_maps_proxy_errors() {
        for (error, expected) in [
            (
                ProxyError::AuthError("token unavailable".to_string()),
                CodexPoolAttemptOutcome::Credential { status: None },
            ),
            (
                ProxyError::UpstreamError {
                    status: 401,
                    body: None,
                },
                CodexPoolAttemptOutcome::Credential { status: Some(401) },
            ),
            (
                ProxyError::UpstreamError {
                    status: 403,
                    body: None,
                },
                CodexPoolAttemptOutcome::Credential { status: Some(403) },
            ),
            (
                ProxyError::UpstreamError {
                    status: 402,
                    body: None,
                },
                CodexPoolAttemptOutcome::Quota { status: 402 },
            ),
            (
                ProxyError::UpstreamError {
                    status: 429,
                    body: None,
                },
                CodexPoolAttemptOutcome::Quota { status: 429 },
            ),
            (
                ProxyError::Timeout("upstream".to_string()),
                CodexPoolAttemptOutcome::Transient { status: None },
            ),
            (
                ProxyError::ForwardFailed("connect".to_string()),
                CodexPoolAttemptOutcome::Transient { status: None },
            ),
            (
                ProxyError::ProviderUnhealthy("temporary".to_string()),
                CodexPoolAttemptOutcome::Transient { status: None },
            ),
            (
                ProxyError::StreamIdleTimeout(30),
                CodexPoolAttemptOutcome::Transient { status: None },
            ),
            (
                ProxyError::UpstreamError {
                    status: 500,
                    body: None,
                },
                CodexPoolAttemptOutcome::Transient { status: Some(500) },
            ),
            (
                ProxyError::UpstreamError {
                    status: 502,
                    body: None,
                },
                CodexPoolAttemptOutcome::Transient { status: Some(502) },
            ),
            (
                ProxyError::UpstreamError {
                    status: 503,
                    body: None,
                },
                CodexPoolAttemptOutcome::Transient { status: Some(503) },
            ),
        ] {
            assert_eq!(classify_codex_pool_attempt(&error), expected);
        }

        for status in [400, 405, 406, 413, 414, 415, 422, 501] {
            assert_eq!(
                classify_codex_pool_attempt(&ProxyError::UpstreamError { status, body: None }),
                CodexPoolAttemptOutcome::Neutral,
                "status={status} must not mutate account health"
            );
        }
    }

    #[test]
    fn pool_codex_auth_failures_retry_the_next_account() {
        let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        let pool = test_codex_pool_candidate("acc-a", 7);

        for error in [
            ProxyError::AuthError("re-login".to_string()),
            ProxyError::UpstreamError {
                status: 401,
                body: None,
            },
            ProxyError::UpstreamError {
                status: 403,
                body: None,
            },
        ] {
            assert_eq!(
                forwarder.categorize_proxy_error(&error, &pool),
                ErrorCategory::Retryable
            );
        }
    }

    #[test]
    fn pool_candidate_failures_do_not_affect_persistent_provider_health() {
        let pool = test_codex_pool_candidate("acc-a", 7);
        let direct = test_codex_official_provider();

        for error in [
            ProxyError::AuthError("re-login".to_string()),
            ProxyError::UpstreamError {
                status: 429,
                body: None,
            },
            ProxyError::Timeout("upstream".to_string()),
        ] {
            assert!(!retryable_failure_affects_provider_health(&pool, &error));
            assert!(retryable_failure_affects_provider_health(&direct, &error));
        }

        assert!(retryable_failure_affects_provider_health(
            &pool,
            &ProxyError::ConfigError("route configuration".to_string()),
        ));
    }

    #[test]
    fn native_codex_auth_passthrough_is_limited_to_local_codex_requests() {
        let provider = test_codex_official_provider();
        let local_headers = HeaderMap::new();
        assert!(should_passthrough_codex_official_auth(
            &AppType::Codex,
            &provider,
            &local_headers,
        ));

        let mut external_headers = HeaderMap::new();
        external_headers.insert(
            "x-cc-switch-external-openai-api",
            HeaderValue::from_static("1"),
        );
        assert!(!should_passthrough_codex_official_auth(
            &AppType::Codex,
            &provider,
            &external_headers,
        ));
        assert!(!should_passthrough_codex_official_auth(
            &AppType::Claude,
            &provider,
            &local_headers,
        ));
    }

    #[test]
    fn codex_auth_ownership_pool_managed_candidate_does_not_reuse_desktop_bearer() {
        let headers = HeaderMap::new();
        let mut managed = test_codex_official_provider();
        managed.settings_config[CODEX_ACCOUNT_POOL_ENABLED] = Value::Bool(true);
        managed.settings_config["codexNativeAuthPassthrough"] = Value::Bool(false);
        managed.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            ..Default::default()
        });
        assert!(crate::proxy::providers::is_codex_official_provider(
            &managed
        ));
        assert!(
            !should_passthrough_codex_official_auth(&AppType::Codex, &managed, &headers),
            "managed pool candidates must replace the incoming Desktop bearer"
        );

        let mut native = managed.clone();
        native.settings_config["codexNativeAuthPassthrough"] = Value::Bool(true);
        native.meta = Some(crate::provider::ProviderMeta::default());
        assert!(should_passthrough_codex_official_auth(
            &AppType::Codex,
            &native,
            &headers,
        ));
    }

    #[test]
    fn codex_auth_ownership_proxy_mode_header_is_local_only_for_all_transports() {
        let name = http::HeaderName::from_static("x-cc-switch-proxy-mode");
        assert!(outbound_header_is_local_only(&name));
        assert!(raw_passthrough_header_should_skip(&name));

        let mut source = HeaderMap::new();
        source.insert(name.clone(), HeaderValue::from_static("router"));
        source.insert("x-user-header", HeaderValue::from_static("kept"));
        let rebuilt = build_raw_passthrough_headers(&source, &[], None, None);
        assert!(!rebuilt.contains_key(name));
        assert_eq!(
            rebuilt
                .get("x-user-header")
                .and_then(|value| value.to_str().ok()),
            Some("kept")
        );
    }

    fn test_forwarder(
        non_streaming_timeout: Duration,
        streaming_first_byte_timeout: Duration,
    ) -> RequestForwarder {
        let db = Arc::new(Database::memory().expect("memory db"));

        RequestForwarder {
            router: Arc::new(ProviderRouter::new(db.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            gemini_shadow: Arc::new(GeminiShadowStore::new()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
            app_handle: None,
            current_provider_id_at_start: String::new(),
            session_id: String::new(),
            session_client_provided: false,
            preserve_codex_client_originator: false,
            rectifier_config: RectifierConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            copilot_optimizer_config: CopilotOptimizerConfig::default(),
            codex_responses_lite_fallbacks: Arc::new(RwLock::new(HashMap::new())),
            non_streaming_timeout,
            streaming_first_byte_timeout,
            max_attempts: 1,
        }
    }

    // 验证只有上游明确返回 Responses-Lite 不支持时，才触发剥头重试。
    #[test]
    fn codex_responses_lite_error_triggers_retry_without_header() {
        let header = http::HeaderName::from_static("x-openai-internal-codex-responses-lite");
        let mut headers = http::HeaderMap::new();
        headers.insert(header, http::HeaderValue::from_static("true"));

        assert!(should_retry_without_codex_responses_lite_header(
            &AppType::Codex,
            &headers,
            400,
            Some("This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite.")
        ));
    }

    // 验证普通 400 不触发剥头重试，避免隐藏真实请求错误。
    #[test]
    fn ordinary_upstream_error_does_not_trigger_responses_lite_retry() {
        let header = http::HeaderName::from_static("x-openai-internal-codex-responses-lite");
        let mut headers = http::HeaderMap::new();
        headers.insert(header, http::HeaderValue::from_static("true"));

        assert!(!should_retry_without_codex_responses_lite_header(
            &AppType::Codex,
            &headers,
            400,
            Some("invalid_request_error: missing required field")
        ));
    }

    // 验证非 Codex app 流量不应用 Codex Responses-Lite fallback。
    #[test]
    fn non_codex_app_does_not_trigger_responses_lite_retry() {
        let header = http::HeaderName::from_static("x-openai-internal-codex-responses-lite");
        let mut headers = http::HeaderMap::new();
        headers.insert(header, http::HeaderValue::from_static("true"));

        assert!(!should_retry_without_codex_responses_lite_header(
            &AppType::Claude,
            &headers,
            400,
            Some("This model is not supported when using X-OpenAI-Internal-Codex-Responses-Lite.")
        ));
    }

    // 验证 header 名识别只匹配已知 Codex Responses-Lite 私有头。
    #[test]
    fn codex_responses_lite_header_name_is_detected_precisely() {
        let lite_header = http::HeaderName::from_static("x-openai-internal-codex-responses-lite");
        let custom_header = http::HeaderName::from_static("x-custom-feature");

        assert!(is_codex_responses_lite_header(&lite_header));
        assert!(!is_codex_responses_lite_header(&custom_header));
    }

    // 验证 fallback key 按 provider、上游 path 和模型隔离，避免一个模型失败后误伤其它模型。
    #[test]
    fn codex_responses_lite_fallback_key_scopes_provider_url_and_model() {
        let key_a = codex_responses_lite_fallback_key(
            "provider-a",
            "https://api.example.com/v1/responses?token=secret",
            "gpt-5.5",
        );
        let key_b = codex_responses_lite_fallback_key(
            "provider-a",
            "https://api.example.com/v1/responses?token=other",
            "gpt-5.5",
        );
        let key_other_model = codex_responses_lite_fallback_key(
            "provider-a",
            "https://api.example.com/v1/responses",
            "gpt-5.4",
        );
        let key_other_provider = codex_responses_lite_fallback_key(
            "provider-b",
            "https://api.example.com/v1/responses",
            "gpt-5.5",
        );

        assert_eq!(key_a, key_b);
        assert_ne!(key_a, key_other_model);
        assert_ne!(key_a, key_other_provider);
        assert!(!key_a.contains("secret"));
    }

    #[test]
    fn native_codex_responses_streams_get_reconnect_factory() {
        assert!(should_create_responses_stream_reconnector(
            &AppType::Codex,
            "/v1/responses",
            true,
            None,
        ));
        assert!(should_create_responses_stream_reconnector(
            &AppType::Claude,
            "/v1/messages",
            true,
            Some("openai_responses"),
        ));
        assert!(!should_create_responses_stream_reconnector(
            &AppType::Codex,
            "/v1/responses",
            false,
            None,
        ));
        assert!(!should_create_responses_stream_reconnector(
            &AppType::Codex,
            "/v1/models",
            true,
            None,
        ));
    }

    // 验证短期负缓存命中期间有效，过期后会自动删除并允许下一次重新带头探测。
    #[test]
    fn codex_responses_lite_fallback_cache_expires() {
        let now = Instant::now();
        let key = "provider|https://api.example.com/v1/responses|gpt-5.5".to_string();
        let mut fallbacks = HashMap::new();
        fallbacks.insert(key.clone(), now + CODEX_RESPONSES_LITE_FALLBACK_TTL);

        assert!(codex_responses_lite_fallback_active_at(
            &mut fallbacks,
            &key,
            now + Duration::from_secs(60)
        ));
        assert!(!codex_responses_lite_fallback_active_at(
            &mut fallbacks,
            &key,
            now + CODEX_RESPONSES_LITE_FALLBACK_TTL + Duration::from_secs(1)
        ));
        assert!(!fallbacks.contains_key(&key));
    }

    #[test]
    fn codex_chat_request_shape_omits_prompt_text_and_records_field_shapes() {
        let body = json!({
            "model": "glm-5.2",
            "messages": [
                {"role": "user", "content": "secret prompt should not appear"}
            ],
            "thinking": {"type": "enabled"},
            "reasoning_effort": "max",
            "max_tokens": 32768,
            "stream_options": {"include_usage": true},
            "tools": [{
                "type": "function",
                "function": {
                    "name": "read_secret",
                    "parameters": {"type": "object"}
                }
            }]
        });

        let summary = summarize_codex_chat_request_shape(&body);

        assert!(summary.contains("model"));
        assert!(summary.contains("messages=1"));
        assert!(summary.contains("tools=1"));
        assert!(summary.contains("tool_types=[function]"));
        assert!(summary.contains("thinking=object(keys=[type])"));
        assert!(summary.contains("reasoning_effort=string"));
        assert!(summary.contains("max_tokens=number"));
        assert!(summary.contains("stream_options=object(keys=[include_usage])"));
        assert!(!summary.contains("secret prompt"));
        assert!(!summary.contains("read_secret"));
    }

    #[test]
    fn single_provider_retryable_log_uses_single_provider_code() {
        let error = ProxyError::UpstreamError {
            status: 429,
            body: Some(r#"{"error":{"message":"rate limit exceeded"}}"#.to_string()),
        };

        let (code, message) = build_retryable_failure_log("PackyCode-response", 1, 1, &error);

        assert_eq!(code, log_fwd::SINGLE_PROVIDER_FAILED);
        assert!(message.contains("Provider PackyCode-response 请求失败"));
        assert!(message.contains("上游 HTTP 429"));
        // 上游错误消息保留(截断)，用于诊断失败原因。
        assert!(message.contains("rate limit exceeded"));
        assert!(!message.contains("切换下一个"));
    }

    #[test]
    fn multi_provider_retryable_log_keeps_failover_wording() {
        let error = ProxyError::Timeout("upstream timed out after 30s".to_string());

        let (code, message) = build_retryable_failure_log("primary", 1, 3, &error);

        assert_eq!(code, log_fwd::PROVIDER_FAILED_RETRY);
        assert!(message.contains("继续尝试下一个 (1/3)"));
        assert!(message.contains("请求超时"));
    }

    #[test]
    fn single_provider_has_no_terminal_all_failed_log() {
        assert!(build_terminal_failure_log(1, 1, None).is_none());
    }

    #[test]
    fn multi_provider_terminal_log_contains_last_error_summary() {
        let error = ProxyError::ForwardFailed("connection reset by peer".to_string());

        let (code, message) =
            build_terminal_failure_log(2, 2, Some(&error)).expect("expected terminal log");

        assert_eq!(code, log_fwd::ALL_PROVIDERS_FAILED);
        assert!(message.contains("已尝试 2/2 个 Provider，均失败"));
        assert!(message.contains("connection reset by peer"));
    }

    #[test]
    fn summarize_text_for_log_collapses_whitespace_and_truncates() {
        let summary = summarize_text_for_log("line1\n\n line2   line3", 12);

        assert_eq!(summary, "line1 line2...");
    }

    #[test]
    fn codex_local_router_fallback_detection_covers_known_loopback_urls() {
        assert!(codex_base_url_points_to_local_proxy(
            "http://127.0.0.1:15721/v1"
        ));
        assert!(!codex_base_url_points_to_local_proxy(
            "http://localhost:8000/v1"
        ));
        assert!(!codex_base_url_points_to_local_proxy(
            "https://api.openai.com/v1"
        ));
    }

    #[test]
    fn codex_routing_config_detection_reads_new_and_legacy_fields() {
        let mut provider = test_provider_with_type(None);
        assert!(!codex_provider_has_routing_config(&provider));

        provider.settings_config = json!({ "codexRouting": { "routes": [] } });
        assert!(codex_provider_has_routing_config(&provider));

        provider.settings_config = json!({ "modelRoutes": [] });
        assert!(codex_provider_has_routing_config(&provider));
    }

    #[test]
    fn codex_multirouter_attempts_keep_only_matched_route_and_parent_context() {
        let mut provider = test_provider_with_type(None);
        provider.id = "codex-openai-router".to_string();
        provider.name = "OpenAI Multi-Model Router".to_string();
        provider.settings_config = json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "qwen-local",
                        "name": "Qwen Local vLLM",
                        "enabled": true,
                        "models": ["qwen3.6"],
                        "base_url": "https://example.test/v1",
                        "wireApi": "chat"
                    },
                    {
                        "id": "deepseek",
                        "name": "DeepSeek",
                        "enabled": true,
                        "models": ["deepseek-v4-flash"],
                        "base_url": "https://api.deepseek.com/v1",
                        "wireApi": "chat"
                    }
                ]
            }
        });

        let attempts = build_forward_attempt_providers_preserving_codex_router_context(
            &AppType::Codex,
            &[provider.clone()],
            &json!({ "model": "deepseek-v4-flash" }),
        );

        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].id, "codex-openai-router::route::deepseek");
        assert_eq!(
            attempts[0]
                .settings_config
                .get("codexResolvedRouteId")
                .and_then(serde_json::Value::as_str),
            Some("deepseek")
        );
        for attempt in &attempts {
            assert_eq!(
                attempt
                    .settings_config
                    .get("codexRouterParentProviderId")
                    .and_then(serde_json::Value::as_str),
                Some("codex-openai-router")
            );
            assert_eq!(
                attempt
                    .settings_config
                    .get("codexRouterParentProviderName")
                    .and_then(serde_json::Value::as_str),
                Some("OpenAI Multi-Model Router")
            );
        }
    }

    #[test]
    fn codex_multirouter_keeps_outer_provider_failover_without_cross_route_fallback() {
        let mut router = test_provider_with_type(None);
        router.id = "codex-openai-router".to_string();
        router.settings_config = json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "deepseek",
                        "enabled": true,
                        "models": ["deepseek-v4-flash"],
                        "base_url": "https://api.deepseek.com/v1",
                        "wireApi": "chat"
                    },
                    {
                        "id": "qwen-local",
                        "enabled": true,
                        "models": ["qwen3.6"],
                        "base_url": "https://example.test/v1",
                        "wireApi": "chat"
                    }
                ]
            }
        });
        let mut explicit_fallback = test_provider_with_type(None);
        explicit_fallback.id = "provider-level-fallback".to_string();

        let attempts = build_forward_attempt_providers_preserving_codex_router_context(
            &AppType::Codex,
            &[router, explicit_fallback],
            &json!({ "model": "deepseek-v4-flash" }),
        );

        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].id, "codex-openai-router::route::deepseek");
        assert_eq!(attempts[1].id, "provider-level-fallback");
    }

    #[test]
    fn codex_multirouter_attempt_materializes_referenced_target_before_forwarding() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let target = Provider::with_id(
            "deepseek-target".to_string(),
            "DeepSeek Target".to_string(),
            json!({
                "base_url": "https://api.deepseek.com",
                "api_key": "target-secret"
            }),
            None,
        );
        db.save_provider("codex", &target)
            .expect("save target provider");

        let mut router = test_provider_with_type(None);
        router.id = "codex-multirouter".to_string();
        router.name = "Codex MultiRouter".to_string();
        router.settings_config = json!({
            "base_url": "http://127.0.0.1:15721/v1",
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "deepseek",
                        "label": "DeepSeek",
                        "enabled": true,
                        "targetProviderId": "deepseek-target",
                        "match": { "models": ["deepseek-v4-flash"] },
                        "upstream": { "apiFormat": "openai_chat" }
                    }
                ]
            }
        });

        let route_attempts = build_forward_attempt_providers_preserving_codex_router_context(
            &AppType::Codex,
            &[router],
            &json!({ "model": "deepseek-v4-flash" }),
        );
        let forwarder = RequestForwarder {
            router: Arc::new(ProviderRouter::new(db.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            gemini_shadow: Arc::new(GeminiShadowStore::new()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
            app_handle: None,
            current_provider_id_at_start: String::new(),
            session_id: String::new(),
            session_client_provided: false,
            preserve_codex_client_originator: false,
            rectifier_config: RectifierConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            copilot_optimizer_config: CopilotOptimizerConfig::default(),
            codex_responses_lite_fallbacks: Arc::new(RwLock::new(HashMap::new())),
            non_streaming_timeout: Duration::ZERO,
            streaming_first_byte_timeout: Duration::ZERO,
            max_attempts: 1,
        };

        let effective = forwarder
            .materialize_codex_forward_attempt_provider(&AppType::Codex, &route_attempts[0])
            .expect("materialize target provider");

        assert_eq!(
            effective.settings_config["base_url"],
            "https://api.deepseek.com"
        );
        assert_eq!(effective.settings_config["api_key"], "target-secret");
        assert_eq!(
            effective.settings_config["codexRouterParentProviderId"],
            "codex-multirouter"
        );
        assert_eq!(
            effective.settings_config["codexResolvedTargetProviderId"],
            "deepseek-target"
        );
    }

    #[test]
    fn materialized_official_route_keeps_mixed_router_plaintext_delivery_policy() {
        let db = Arc::new(Database::memory().expect("memory db"));
        let official_target = test_codex_official_provider();
        db.save_provider("codex", &official_target)
            .expect("save official target provider");

        let mut router = test_provider_with_type(None);
        router.id = "codex-multirouter".to_string();
        router.settings_config = json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "official",
                        "enabled": true,
                        "targetProviderId": official_target.id,
                        "match": { "models": ["gpt-5.6-sol"] },
                        "upstream": {
                            "apiFormat": "openai_responses",
                            "auth": { "source": "native_codex_auth" }
                        }
                    },
                    {
                        "id": "qwen",
                        "enabled": true,
                        "match": { "models": ["qwen3.6"] },
                        "upstream": {
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" }
                        }
                    }
                ]
            }
        });

        let route_attempts = build_forward_attempt_providers_preserving_codex_router_context(
            &AppType::Codex,
            &[router],
            &json!({ "model": "gpt-5.6-sol" }),
        );
        let forwarder = RequestForwarder {
            router: Arc::new(ProviderRouter::new(db.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            gemini_shadow: Arc::new(GeminiShadowStore::new()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
            app_handle: None,
            current_provider_id_at_start: String::new(),
            session_id: String::new(),
            session_client_provided: false,
            preserve_codex_client_originator: false,
            rectifier_config: RectifierConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            copilot_optimizer_config: CopilotOptimizerConfig::default(),
            codex_responses_lite_fallbacks: Arc::new(RwLock::new(HashMap::new())),
            non_streaming_timeout: Duration::ZERO,
            streaming_first_byte_timeout: Duration::ZERO,
            max_attempts: 1,
        };

        let effective = forwarder
            .materialize_codex_forward_attempt_provider(&AppType::Codex, &route_attempts[0])
            .expect("materialize official target provider");

        assert_eq!(
            effective
                .settings_config
                .get("codexRouterPlaintextV2Collaboration")
                .and_then(Value::as_bool),
            Some(true),
            "retry-layer route materialization must preserve the mixed-router plaintext policy"
        );
    }

    #[test]
    fn agents_plaintext_rewrite_requires_codex_mixed_router_and_official_parent() {
        let mut mixed = test_provider_with_type(None);
        mixed.settings_config = json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {"enabled": true, "upstream": {"auth": {"source": "managed_codex_oauth"}}},
                    {"enabled": true, "upstream": {"auth": {"source": "provider_config"}}}
                ]
            }
        });
        assert!(should_make_codex_v2_agents_plaintext(
            &AppType::Codex,
            &mixed,
            true
        ));
        assert!(!should_make_codex_v2_agents_plaintext(
            &AppType::Codex,
            &mixed,
            false
        ));
        assert!(!should_make_codex_v2_agents_plaintext(
            &AppType::Claude,
            &mixed,
            true
        ));

        let mut official_only = test_provider_with_type(None);
        official_only.settings_config = json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "enabled": true,
                    "upstream": {"auth": {"source": "native_codex_auth"}}
                }]
            }
        });
        assert!(!should_make_codex_v2_agents_plaintext(
            &AppType::Codex,
            &official_only,
            true
        ));
    }

    #[test]
    fn agent_message_projection_runs_only_for_third_party_codex_responses() {
        let third_party = test_provider_with_type(None);
        assert!(should_project_codex_agent_messages_for_provider(
            &AppType::Codex,
            &third_party,
            "/v1/responses"
        ));
        assert!(!should_project_codex_agent_messages_for_provider(
            &AppType::Claude,
            &third_party,
            "/v1/responses"
        ));
        assert!(!should_project_codex_agent_messages_for_provider(
            &AppType::Codex,
            &third_party,
            "/v1/chat/completions"
        ));

        let official = test_codex_official_provider();
        assert!(!should_project_codex_agent_messages_for_provider(
            &AppType::Codex,
            &official,
            "/v1/responses"
        ));

        let managed_official = test_provider_with_type(Some("codex_oauth"));
        assert!(!should_project_codex_agent_messages_for_provider(
            &AppType::Codex,
            &managed_official,
            "/v1/responses"
        ));
    }

    #[tokio::test]
    async fn codex_resolved_route_local_self_loop_is_failed_not_successful() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let mut resolved = test_provider_with_type(None);
        resolved.id = "codex-multirouter::route::broken".to_string();
        resolved.name = "Broken local route".to_string();
        resolved.settings_config = json!({
            "base_url": "http://127.0.0.1:15721/v1",
            "api_key": "unused",
            "codexResolvedRouteId": "broken",
            "codexResolvedRouteMatched": true,
            "codexRouterParentProviderId": "codex-multirouter",
            "codexRouterParentProviderName": "Codex MultiRouter"
        });

        let error = match forwarder
            .forward_with_retry(
                &AppType::Codex,
                http::Method::POST,
                "/responses",
                json!({
                    "model": "deepseek-v4-flash",
                    "stream": true,
                    "input": []
                }),
                HeaderMap::new(),
                Extensions::new(),
                vec![resolved],
            )
            .await
        {
            Ok(_) => panic!("local self-loop must be rejected"),
            Err(error) => error,
        };

        assert!(matches!(error.error, ProxyError::InvalidRequest(_)));
        let status = forwarder.status.read().await;
        assert_eq!(status.total_requests, 1);
        assert_eq!(status.success_requests, 0);
        assert_eq!(status.failed_requests, 1);
        assert_eq!(status.success_rate, 0.0);
    }

    #[test]
    fn compaction_routes_to_current_session_model_not_arbitrary_fallback() {
        let mut provider = test_provider_with_type(None);
        provider.id = "codex-openai-router".to_string();
        provider.name = "OpenAI Multi-Model Router".to_string();
        provider.settings_config = json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "official",
                        "name": "OpenAI Official Backup",
                        "enabled": true,
                        "models": ["gpt-5.6-sol"],
                        "base_url": "https://chatgpt.com/backend-api/codex",
                        "wireApi": "responses"
                    },
                    {
                        "id": "deepseek",
                        "name": "DeepSeek",
                        "enabled": true,
                        "models": ["deepseek-v4-flash"],
                        "base_url": "https://api.deepseek.com/v1",
                        "wireApi": "chat"
                    },
                    {
                        "id": "qwen-local",
                        "name": "Qwen Local vLLM",
                        "enabled": true,
                        "models": ["qwen3.6"],
                        "base_url": "https://example.test/v1",
                        "wireApi": "chat"
                    }
                ]
            }
        });

        let mut body = json!({ "model": "gpt-5.6-sol" });
        assert!(apply_codex_compaction_current_model(
            &mut body,
            Some("qwen3.6")
        ));
        assert_eq!(body["model"], "qwen3.6");
        let attempts = build_forward_attempt_providers_preserving_codex_router_context(
            &AppType::Codex,
            &[provider.clone()],
            &body,
        );

        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].id, "codex-openai-router::route::qwen-local");
        assert_eq!(
            attempts[0]
                .settings_config
                .get("codexResolvedRouteId")
                .and_then(serde_json::Value::as_str),
            Some("qwen-local")
        );
        assert_eq!(
            attempts[0]
                .settings_config
                .get("model")
                .and_then(serde_json::Value::as_str),
            Some("qwen3.6")
        );
    }

    #[test]
    fn canonical_json_sorts_object_keys_for_cache_trace_hashes() {
        let left = json!({
            "tools": [
                {
                    "parameters": {
                        "properties": {
                            "b": {"type": "string"},
                            "a": {"type": "number"}
                        },
                        "type": "object"
                    },
                    "name": "lookup"
                }
            ]
        });
        let right = json!({
            "tools": [
                {
                    "name": "lookup",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "a": {"type": "number"},
                            "b": {"type": "string"}
                        }
                    }
                }
            ]
        });

        assert_eq!(
            crate::proxy::json_canonical::canonical_json_string(&left),
            crate::proxy::json_canonical::canonical_json_string(&right)
        );
        assert_eq!(
            short_value_hash(Some(&left)),
            short_value_hash(Some(&right))
        );
    }

    #[test]
    fn prepare_upstream_request_body_filters_private_fields_and_canonicalizes_order() {
        let body = json!({
            "z": 1,
            "_internal": "drop",
            "tools": [
                {
                    "name": "lookup",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "_id": {
                                "_private_note": "drop",
                                "type": "string"
                            },
                            "b": {"type": "number"},
                            "a": {"type": "string"}
                        }
                    }
                }
            ],
            "a": 2
        });

        let prepared = prepare_upstream_request_body(body);

        assert!(prepared.get("_internal").is_none());
        assert!(prepared["tools"][0]["parameters"]["properties"]
            .get("_id")
            .is_some());
        assert!(prepared["tools"][0]["parameters"]["properties"]["_id"]
            .get("_private_note")
            .is_none());
        assert_eq!(
            serde_json::to_string(&prepared).unwrap(),
            r#"{"a":2,"tools":[{"name":"lookup","parameters":{"properties":{"_id":{"type":"string"},"a":{"type":"string"},"b":{"type":"number"}},"type":"object"}}],"z":1}"#
        );
    }

    #[test]
    fn prepare_upstream_request_body_preserves_codex_native_multimodal_and_internal_items() {
        let body = json!({
            "model": "gpt-5.6-sol",
            "client_metadata": {
                "x-codex-installation-id": "install-1",
                "x-codex-turn-metadata": "{\"request_kind\":\"turn\"}"
            },
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "inspect" },
                        { "type": "input_image", "image_url": "data:image/png;base64,abc", "detail": "original" },
                        { "type": "input_audio", "audio_url": "data:audio/wav;base64,def" }
                    ],
                    "internal_chat_message_metadata_passthrough": { "kind": "desktop" }
                },
                {
                    "type": "function_call",
                    "name": "lookup",
                    "call_id": "call-1",
                    "arguments": "{}",
                    "encrypted_function_args": ["encrypted-args"]
                }
            ]
        });

        let prepared = prepare_upstream_request_body(body.clone());

        assert_eq!(prepared, body);
    }

    #[test]
    fn official_codex_responses_wire_body_is_zstd_encoded() {
        let original = br#"{"model":"gpt-5.6-luna","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"diagnostic"}]}]}"#
            .repeat(256);
        let mut headers = HeaderMap::new();

        let encoded = encode_codex_official_upstream_body(
            &mut headers,
            original.clone(),
            /* enabled */ true,
        )
        .expect("encode official request");

        assert_eq!(
            headers
                .get(http::header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok()),
            Some("zstd")
        );
        assert_ne!(encoded, original);
        assert_eq!(
            zstd::stream::decode_all(std::io::Cursor::new(encoded))
                .expect("decode official request"),
            original
        );
    }

    #[test]
    fn non_official_upstream_wire_body_remains_uncompressed() {
        let original = br#"{"model":"deepseek-v4-flash","input":[]}"#.to_vec();
        let mut headers = HeaderMap::new();

        let encoded = encode_codex_official_upstream_body(
            &mut headers,
            original.clone(),
            /* enabled */ false,
        )
        .expect("prepare third-party request");

        assert!(!headers.contains_key(http::header::CONTENT_ENCODING));
        assert_eq!(encoded, original);
    }

    #[test]
    fn official_codex_responses_request_selects_zstd() {
        assert!(should_zstd_compress_codex_official_upstream(
            &AppType::Codex,
            &http::Method::POST,
            "https://chatgpt.com/backend-api/codex/responses",
            /* official_auth */ true,
            /* transformed */ false,
        ));
    }

    #[test]
    fn third_party_or_transformed_responses_request_does_not_select_zstd() {
        assert!(!should_zstd_compress_codex_official_upstream(
            &AppType::Codex,
            &http::Method::POST,
            "https://api.deepseek.com/v1/responses",
            /* official_auth */ true,
            /* transformed */ false,
        ));
        assert!(!should_zstd_compress_codex_official_upstream(
            &AppType::Codex,
            &http::Method::POST,
            "https://chatgpt.com/backend-api/codex/responses",
            /* official_auth */ true,
            /* transformed */ true,
        ));
    }

    #[test]
    fn codex_oauth_responses_passthrough_normalizer_is_scoped() {
        let codex_oauth = test_provider_with_type(Some("codex_oauth"));
        let regular = test_provider_with_type(None);
        let official_url = "https://chatgpt.com/backend-api/codex/responses";

        assert!(should_normalize_codex_oauth_responses_passthrough_body(
            &AppType::Codex,
            &codex_oauth,
            official_url,
            false,
            false,
            false
        ));
        assert!(!should_normalize_codex_oauth_responses_passthrough_body(
            &AppType::Codex,
            &regular,
            official_url,
            false,
            false,
            false
        ));
        assert!(!should_normalize_codex_oauth_responses_passthrough_body(
            &AppType::Codex,
            &codex_oauth,
            "https://api.openai.com/v1/responses",
            false,
            false,
            false
        ));
        assert!(!should_normalize_codex_oauth_responses_passthrough_body(
            &AppType::Claude,
            &codex_oauth,
            official_url,
            false,
            false,
            false
        ));
        assert!(!should_normalize_codex_oauth_responses_passthrough_body(
            &AppType::Codex,
            &codex_oauth,
            official_url,
            true,
            false,
            false
        ));
        assert!(!should_normalize_codex_oauth_responses_passthrough_body(
            &AppType::Codex,
            &codex_oauth,
            official_url,
            false,
            true,
            false
        ));
    }

    #[test]
    fn native_openai_official_responses_uses_the_same_replay_normalizer() {
        // Switching the top-level Codex provider back from DeepSeek/Qwen uses
        // the built-in native-auth OpenAI Official provider, not a provider
        // whose meta.provider_type is codex_oauth. It still targets the same
        // ChatGPT Codex Responses backend, so third-party reasoning/content
        // history must cross the same target-transport replay boundary.
        let native_official = test_codex_official_provider();

        assert!(should_normalize_codex_oauth_responses_passthrough_body(
            &AppType::Codex,
            &native_official,
            "https://chatgpt.com/backend-api/codex/responses",
            false,
            false,
            false
        ));
    }

    #[test]
    fn codex_responses_passthrough_control_message_normalizer_is_scoped() {
        let codex_oauth = test_provider_with_type(Some("codex_oauth"));
        let regular = test_provider_with_type(None);

        assert!(
            should_normalize_codex_responses_passthrough_control_messages(
                &AppType::Codex,
                &regular,
                "/v1/responses",
                false,
                false,
                false
            )
        );
        assert!(
            should_normalize_codex_responses_passthrough_control_messages(
                &AppType::Codex,
                &regular,
                "/responses/compact?conversation=1",
                false,
                false,
                false
            )
        );
        assert!(
            !should_normalize_codex_responses_passthrough_control_messages(
                &AppType::Codex,
                &codex_oauth,
                "/v1/responses",
                false,
                false,
                false
            )
        );
        assert!(
            !should_normalize_codex_responses_passthrough_control_messages(
                &AppType::Claude,
                &regular,
                "/v1/responses",
                false,
                false,
                false
            )
        );
        assert!(
            !should_normalize_codex_responses_passthrough_control_messages(
                &AppType::Codex,
                &regular,
                "/v1/chat/completions",
                false,
                false,
                false
            )
        );
        assert!(
            !should_normalize_codex_responses_passthrough_control_messages(
                &AppType::Codex,
                &regular,
                "/v1/responses",
                true,
                false,
                false
            )
        );
        assert!(
            !should_normalize_codex_responses_passthrough_control_messages(
                &AppType::Codex,
                &regular,
                "/v1/responses",
                false,
                true,
                false
            )
        );
    }

    #[test]
    fn lm_studio_native_responses_adds_missing_text_format() {
        let mut lm_studio = test_provider_with_type(Some("lmstudio"));
        lm_studio.id = "lmstudio".to_string();
        lm_studio.name = "LM Studio".to_string();

        let normalized = normalize_lm_studio_responses_request(
            &AppType::Codex,
            &lm_studio,
            "/v1/responses",
            true,
            json!({"model": "local-model", "text": {"verbosity": "low"}}),
        );

        assert_eq!(normalized["text"]["verbosity"], "low");
        assert_eq!(normalized["text"]["format"], json!({"type": "text"}));
    }

    #[test]
    fn lm_studio_responses_preserves_explicit_format_and_scope() {
        let mut lm_studio = test_provider_with_type(Some("lmstudio"));
        lm_studio.name = "LM Studio".to_string();
        let explicit_format = json!({
            "type": "json_schema",
            "name": "answer",
            "schema": {"type": "object"}
        });

        let explicit = normalize_lm_studio_responses_request(
            &AppType::Codex,
            &lm_studio,
            "/responses",
            true,
            json!({"text": {"format": explicit_format.clone()}}),
        );
        assert_eq!(explicit["text"]["format"], explicit_format);

        let unrelated = test_provider_with_type(None);
        let unrelated_body = normalize_lm_studio_responses_request(
            &AppType::Codex,
            &unrelated,
            "/responses",
            true,
            json!({"text": {"verbosity": "low"}}),
        );
        assert!(unrelated_body["text"].get("format").is_none());

        let transformed = normalize_lm_studio_responses_request(
            &AppType::Codex,
            &lm_studio,
            "/responses",
            false,
            json!({"text": {"verbosity": "low"}}),
        );
        assert!(transformed["text"].get("format").is_none());
    }

    #[test]
    fn local_proxy_body_overrides_deep_merge_final_body_without_stream() {
        let mut body = json!({
            "model": "before",
            "stream": false,
            "metadata": {
                "keep": true,
                "temperature": 1
            },
            "messages": [{ "role": "user", "content": "hello" }]
        });
        let overrides = LocalProxyRequestOverrides {
            headers: HashMap::new(),
            body: Some(json!({
                "model": "after",
                "stream": true,
                "metadata": {
                    "temperature": 0.2,
                    "top_p": 0.9
                },
                "messages": []
            })),
        };

        assert!(apply_local_proxy_body_overrides(&mut body, &overrides));

        assert_eq!(body["model"], "after");
        assert_eq!(body["stream"], false);
        assert_eq!(body["metadata"]["keep"], true);
        assert_eq!(body["metadata"]["temperature"], 0.2);
        assert_eq!(body["metadata"]["top_p"], 0.9);
        assert_eq!(body["messages"], json!([]));
    }

    #[test]
    fn local_proxy_header_overrides_replace_allowed_headers_only() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::USER_AGENT,
            http::HeaderValue::from_static("original"),
        );
        headers.insert(
            http::header::AUTHORIZATION,
            http::HeaderValue::from_static("Bearer good"),
        );
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );

        let overrides = LocalProxyRequestOverrides {
            headers: HashMap::from([
                ("User-Agent".to_string(), "custom".to_string()),
                ("X-Test".to_string(), "ok".to_string()),
                ("Authorization".to_string(), "Bearer bad".to_string()),
                ("Content-Type".to_string(), "text/plain".to_string()),
                ("X-Bad".to_string(), "bad\nvalue".to_string()),
            ]),
            body: None,
        };

        apply_local_proxy_header_overrides(&mut headers, Some(&overrides), false);

        assert_eq!(
            headers
                .get(http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some("custom")
        );
        assert_eq!(
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer good")
        );
        assert_eq!(
            headers
                .get(http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            headers.get("x-test").and_then(|value| value.to_str().ok()),
            Some("ok")
        );
        assert!(headers.get("x-bad").is_none());
    }

    #[test]
    fn local_proxy_header_overrides_are_skipped_for_copilot() {
        let mut headers = http::HeaderMap::new();
        headers.insert(
            http::header::USER_AGENT,
            http::HeaderValue::from_static("copilot"),
        );
        let overrides = LocalProxyRequestOverrides {
            headers: HashMap::from([("User-Agent".to_string(), "custom".to_string())]),
            body: None,
        };

        apply_local_proxy_header_overrides(&mut headers, Some(&overrides), true);

        assert_eq!(
            headers
                .get(http::header::USER_AGENT)
                .and_then(|value| value.to_str().ok()),
            Some("copilot")
        );
    }

    #[tokio::test]
    async fn non_streaming_success_is_buffered_before_marking_provider_successful() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"{\"ok\":true}"))
            }),
        );

        let prepared = forwarder
            .prepare_success_response_for_failover(response, false)
            .await
            .expect("response should be buffered");

        assert_eq!(
            prepared
                .bytes_with_limit(MAX_RESPONSE_BODY_BYTES)
                .await
                .unwrap(),
            Bytes::from_static(b"{\"ok\":true}")
        );
    }

    #[tokio::test]
    async fn non_streaming_body_read_error_is_pending_before_success_record() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Err::<Bytes, std::io::Error>(std::io::Error::other("body boom"))
            }),
        );

        let err = match forwarder
            .prepare_success_response_for_failover(response, false)
            .await
        {
            Ok(_) => panic!("body read errors should fail the attempt"),
            Err(err) => err,
        };

        assert!(matches!(err, ProxyError::ResponsePending(_)));
    }

    #[tokio::test]
    async fn streaming_success_primes_first_chunk_and_replays_it() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::iter(vec![
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"first")),
                Ok::<Bytes, std::io::Error>(Bytes::from_static(b"second")),
            ]),
        );

        let prepared = forwarder
            .prepare_success_response_for_failover(response, true)
            .await
            .expect("stream should be primed");

        assert_eq!(
            prepared
                .bytes_with_limit(MAX_RESPONSE_BODY_BYTES)
                .await
                .unwrap(),
            Bytes::from_static(b"firstsecond")
        );
    }

    #[tokio::test]
    async fn streaming_first_chunk_error_is_retryable_before_success_record() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Err::<Bytes, std::io::Error>(std::io::Error::other("first chunk boom"))
            }),
        );

        let err = match forwarder
            .prepare_success_response_for_failover(response, true)
            .await
        {
            Ok(_) => panic!("first chunk errors should fail the attempt"),
            Err(err) => err,
        };

        assert!(matches!(err, ProxyError::ForwardFailed(_)));
    }

    #[tokio::test]
    async fn streaming_first_sse_error_event_is_retryable_before_response_is_returned() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::once(async {
                Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"event: error\ndata: {\"error\":{\"message\":\"We're currently experiencing high demand\",\"type\":\"server_error\"}}\n\n",
                ))
            }),
        );

        let err = match forwarder
            .prepare_success_response_for_failover(response, true)
            .await
        {
            Ok(_) => panic!("first SSE error event should fail the attempt before streaming"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            ProxyError::UpstreamError {
                status: 503,
                body: Some(message),
            } if message.contains("high demand")
        ));
    }

    #[tokio::test]
    async fn streaming_first_normal_sse_event_is_replayed_to_client() {
        let forwarder = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        let response = ProxyResponse::streamed(
            StatusCode::OK,
            HeaderMap::new(),
            futures::stream::iter(vec![
                Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"event: response.created\ndata: {\"type\":\"response.created\"}\n\n",
                )),
                Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n",
                )),
            ]),
        );

        let prepared = forwarder
            .prepare_success_response_for_failover(response, true)
            .await
            .expect("normal first SSE event should be replayed");

        let body = prepared
            .bytes_with_limit(MAX_RESPONSE_BODY_BYTES)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("response.created"));
        assert!(String::from_utf8_lossy(&body).contains("response.completed"));
    }

    #[test]
    fn codex_oauth_session_headers_match_codex_cache_identity() {
        let headers = build_codex_oauth_session_headers("session-123");
        let mut map = HeaderMap::new();
        for (name, value) in headers {
            map.insert(name, value);
        }

        assert_eq!(
            map.get("session-id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("thread-id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("x-client-request-id"),
            Some(&HeaderValue::from_static("session-123"))
        );
        assert_eq!(
            map.get("x-codex-window-id"),
            Some(&HeaderValue::from_static("session-123:0"))
        );
    }

    #[test]
    /// 可信本地 Codex OAuth 请求应保留唯一的官方 first-party 来源。
    fn codex_oauth_originator_preserves_trusted_first_party_identity() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("Codex Desktop"));

        enforce_codex_oauth_originator(&mut headers, true, true);

        let values = headers
            .get_all("originator")
            .iter()
            .map(|value| value.to_str().expect("valid originator"))
            .collect::<Vec<_>>();
        assert_eq!(values, vec!["Codex Desktop"]);
    }

    #[test]
    /// 自定义 OpenAI provider 不继承内建 provider 的 version 头；最终出站前必须从
    /// 同一可信 Codex 客户端的 User-Agent 恢复真实版本，不能继续使用 CCSM 硬编码版本。
    fn codex_oauth_identity_restores_version_from_matching_native_user_agent() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("Codex Desktop"));
        headers.insert(
            http::header::USER_AGENT,
            HeaderValue::from_static(
                "Codex Desktop/0.151.0 (Windows 11; x86_64) codex-terminal/1.0",
            ),
        );

        enforce_codex_oauth_originator(&mut headers, true, true);

        assert_eq!(
            headers.get("version"),
            Some(&HeaderValue::from_static("0.151.0"))
        );
    }

    #[test]
    /// 官方 Codex 的 User-Agent 描述进程身份，originator 允许被线程级来源覆盖；
    /// 两者不相同时仍应恢复进程携带的真实版本。
    fn codex_oauth_identity_restores_version_when_thread_originator_differs_from_process() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("codex_vscode"));
        headers.insert(
            http::header::USER_AGENT,
            HeaderValue::from_static(
                "codex_cli_rs/0.151.0 (Windows 11; x86_64) codex-terminal/1.0",
            ),
        );

        enforce_codex_oauth_originator(&mut headers, true, true);

        assert_eq!(
            headers.get("version"),
            Some(&HeaderValue::from_static("0.151.0"))
        );
        assert_eq!(
            headers.get("originator"),
            Some(&HeaderValue::from_static("codex_vscode"))
        );
    }

    #[test]
    /// 本地 Codex 的线程来源头即使损坏并回退到 CLI，可信 User-Agent 中的真实版本
    /// 仍应保留，避免再次形成只有 originator 没有 version 的半套身份。
    fn codex_oauth_identity_restores_version_before_invalid_originator_fallback() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("unknown-client"));
        headers.insert(
            http::header::USER_AGENT,
            HeaderValue::from_static(
                "codex_cli_rs/0.151.0 (Windows 11; x86_64) codex-terminal/1.0",
            ),
        );

        enforce_codex_oauth_originator(&mut headers, true, true);

        assert_eq!(
            headers.get("version"),
            Some(&HeaderValue::from_static("0.151.0"))
        );
        assert_eq!(
            headers.get("originator"),
            Some(&HeaderValue::from_static("codex_cli_rs"))
        );
    }

    #[test]
    fn codex_oauth_identity_does_not_invent_version_for_mismatched_user_agent() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("Codex Desktop"));
        headers.insert(
            http::header::USER_AGENT,
            HeaderValue::from_static("cc-switch/3.17.0"),
        );

        enforce_codex_oauth_originator(&mut headers, true, true);

        assert!(headers.get("version").is_none());
    }

    #[test]
    fn codex_official_identity_overwrites_stale_version_from_trusted_user_agent() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
        headers.insert(
            http::header::USER_AGENT,
            HeaderValue::from_static("codex_cli_rs/0.151.0 (Windows 11; x86_64) terminal/1.0"),
        );
        headers.insert("version", HeaderValue::from_static("0.150.2"));

        enforce_codex_oauth_originator(&mut headers, true, true);

        assert_eq!(
            headers.get("version"),
            Some(&HeaderValue::from_static("0.151.0"))
        );
    }

    #[test]
    fn codex_official_identity_strips_external_api_version_claim() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("Codex Desktop"));
        headers.insert(
            http::header::USER_AGENT,
            HeaderValue::from_static("external-agent/1.0"),
        );
        headers.insert("version", HeaderValue::from_static("999.0.0"));
        headers.insert(
            "x-oai-attestation",
            HeaderValue::from_static("untrusted-external-attestation"),
        );

        enforce_codex_oauth_originator(&mut headers, true, false);

        assert_eq!(
            headers.get("originator"),
            Some(&HeaderValue::from_static("codex_cli_rs"))
        );
        assert!(headers.get("version").is_none());
        assert!(headers.get("x-oai-attestation").is_none());
    }

    #[test]
    fn codex_native_auth_passthrough_identity_restores_version() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("Codex Desktop"));
        headers.insert(
            http::header::USER_AGENT,
            HeaderValue::from_static("Codex Desktop/0.151.0 (Windows 11; x86_64)"),
        );

        let is_managed_oauth = false;
        let is_native_auth_passthrough = true;
        enforce_codex_oauth_originator(
            &mut headers,
            is_managed_oauth || is_native_auth_passthrough,
            true,
        );

        assert_eq!(
            headers.get("version"),
            Some(&HeaderValue::from_static("0.151.0"))
        );
    }

    #[test]
    fn raw_native_codex_auth_rebuild_preserves_desktop_bearer_and_account() {
        let mut source = HeaderMap::new();
        source.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer desktop-oauth"),
        );
        source.insert("chatgpt-account-id", HeaderValue::from_static("account-1"));
        let mut auth_headers = vec![(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer stale-managed"),
        )];

        replace_with_native_codex_auth_headers(&mut auth_headers, &source);
        let rebuilt = build_raw_passthrough_headers(&source, &auth_headers, None, None);

        assert_eq!(
            rebuilt.get(http::header::AUTHORIZATION),
            Some(&HeaderValue::from_static("Bearer desktop-oauth"))
        );
        assert_eq!(
            rebuilt.get("chatgpt-account-id"),
            Some(&HeaderValue::from_static("account-1"))
        );
    }

    #[test]
    /// 官方 CLI、VS Code、TUI 和 ChatGPT Desktop 身份都属于可保留白名单。
    fn codex_oauth_originator_accepts_official_first_party_values() {
        for originator in [
            "codex_cli_rs",
            "codex_vscode",
            "codex-tui",
            "Codex Desktop",
            "codex_atlas",
            "codex_chatgpt_desktop",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                "originator",
                HeaderValue::from_str(originator).expect("valid originator"),
            );

            enforce_codex_oauth_originator(&mut headers, true, true);

            assert_eq!(
                headers
                    .get("originator")
                    .and_then(|value| value.to_str().ok()),
                Some(originator)
            );
        }
    }

    #[test]
    /// 缺失、未知、重复或不在官方 first-party 分类中的来源必须回退到 CLI 默认值。
    fn codex_oauth_originator_falls_back_for_untrusted_values() {
        let mut cases = vec![HeaderMap::new()];

        for originator in ["cc-switch", "third-party-client", "codex_exec"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                "originator",
                HeaderValue::from_str(originator).expect("valid originator"),
            );
            cases.push(headers);
        }

        let mut duplicate_headers = HeaderMap::new();
        duplicate_headers.append("originator", HeaderValue::from_static("codex_vscode"));
        duplicate_headers.append("originator", HeaderValue::from_static("Codex Desktop"));
        cases.push(duplicate_headers);

        for mut headers in cases {
            enforce_codex_oauth_originator(&mut headers, true, true);

            let values = headers
                .get_all("originator")
                .iter()
                .map(|value| value.to_str().expect("valid originator"))
                .collect::<Vec<_>>();
            assert_eq!(values, vec!["codex_cli_rs"]);
        }
    }

    #[test]
    /// External API 和协议转换请求即使伪造官方来源，也只能使用受控 CLI 回退值。
    fn codex_oauth_originator_does_not_preserve_external_identity() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("Codex Desktop"));

        enforce_codex_oauth_originator(&mut headers, true, false);

        assert_eq!(
            headers.get("originator"),
            Some(&HeaderValue::from_static("codex_cli_rs"))
        );
    }

    #[test]
    /// 非 OAuth 第三方请求的来源头属于调用方，不能被 CCSwitchMulti 改写。
    fn non_codex_oauth_originator_is_not_rewritten() {
        let mut headers = HeaderMap::new();
        headers.insert("originator", HeaderValue::from_static("third-party-client"));

        enforce_codex_oauth_originator(&mut headers, false, true);

        assert_eq!(
            headers.get("originator"),
            Some(&HeaderValue::from_static("third-party-client"))
        );
    }

    #[test]
    fn managed_account_upstream_rejects_proxy_managed_placeholder_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        let err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.githubcopilot.com/chat/completions",
            &headers,
        )
        .expect_err("placeholder should be rejected before upstream");

        assert!(matches!(
            err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));

        let xai_err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.x.ai/v1/responses",
            &headers,
        )
        .expect_err("xAI placeholder should be rejected before upstream");
        assert!(matches!(
            xai_err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));
    }

    #[test]
    fn codex_oauth_upstream_rejects_proxy_managed_placeholder_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        let err = reject_proxy_placeholder_for_managed_account_upstream(
            "https://chatgpt.com/backend-api/codex/responses",
            &headers,
        )
        .expect_err("placeholder should be rejected before upstream");

        assert!(matches!(
            err,
            ProxyError::AuthError(message) if message.contains("PROXY_MANAGED")
        ));
    }

    #[test]
    fn non_managed_upstream_allows_proxy_managed_placeholder_guard() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );

        reject_proxy_placeholder_for_managed_account_upstream(
            "https://api.example.com/v1/messages",
            &headers,
        )
        .expect("guard is scoped to managed-account upstreams");
    }

    #[test]
    fn source_codex_oauth_credentials_reads_request_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer token-from-codex"),
        );
        headers.insert("chatgpt-account-id", HeaderValue::from_static("acct_1"));

        let (token, account_id) =
            source_codex_oauth_credentials(&headers).expect("oauth credentials");

        assert_eq!(token, "token-from-codex");
        assert_eq!(account_id.as_deref(), Some("acct_1"));
    }

    #[test]
    fn streaming_auto_tool_choice_preserves_upstream_stream_instead_of_hosted_loop() {
        let request = serde_json::json!({
            "stream": true,
            "tool_choice": "auto",
            "tools": [
                {"type": "web_search"},
                {"type": "function", "name": "shell", "parameters": {"type": "object"}}
            ]
        });

        assert!(!should_enable_hosted_tool_loop(&request, true));
    }

    #[test]
    fn explicit_hosted_tool_choice_may_use_buffered_hosted_loop() {
        for hosted_type in ["web_search", "image_generation"] {
            let request = serde_json::json!({
                "stream": true,
                "tool_choice": {"type": hosted_type},
                "tools": [{"type": hosted_type}]
            });

            assert!(should_enable_hosted_tool_loop(&request, true));
        }
    }

    #[test]
    fn non_streaming_auto_request_keeps_hosted_tool_loop() {
        let request = serde_json::json!({
            "stream": false,
            "tool_choice": "auto",
            "tools": [{"type": "web_search"}]
        });

        assert!(should_enable_hosted_tool_loop(&request, false));
    }

    /// 验证 hosted web_search loop 会消费第一轮工具调用、回灌 tool output 并返回最终 Chat 响应。
    #[tokio::test]
    async fn hosted_web_search_loop_appends_tool_output_and_marks_response() {
        use crate::proxy::providers::hosted_tools::web_search::HostedWebSearchConfig;

        let initial_response = ProxyResponse::buffered(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::from(
                json!({
                    "id": "chatcmpl_search",
                    "object": "chat.completion",
                    "created": 1,
                    "model": "deepseek-v4-flash",
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "tool_calls": [{
                                "id": "call_search",
                                "type": "function",
                                "function": {
                                    "name": "web_search",
                                    "arguments": "{\"query\":\"\"}"
                                }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }]
                })
                .to_string(),
            ),
        );
        let mut chat_request = json!({
            "model": "deepseek-v4-flash",
            "messages": [{ "role": "user", "content": "Search." }],
            "stream": true,
            "stream_options": { "include_usage": true }
        });
        let sent_bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sent_bodies_for_closure = sent_bodies.clone();

        let final_response = run_hosted_tool_chat_loop(
            initial_response,
            &mut chat_request,
            &HostedToolLoopConfig {
                web_search: Some(HostedWebSearchConfig::default()),
                image_generation: None,
            },
            &Err("test no credentials".to_string()),
            move |body| {
                let sent_bodies = sent_bodies_for_closure.clone();
                let body = body.clone();
                async move {
                    sent_bodies.lock().unwrap().push(body);
                    Ok(ProxyResponse::buffered(
                        StatusCode::OK,
                        HeaderMap::new(),
                        Bytes::from(
                            json!({
                                "id": "chatcmpl_final",
                                "object": "chat.completion",
                                "created": 2,
                                "model": "deepseek-v4-flash",
                                "choices": [{
                                    "message": {
                                        "role": "assistant",
                                        "content": "Search was unavailable."
                                    },
                                    "finish_reason": "stop"
                                }]
                            })
                            .to_string(),
                        ),
                    ))
                }
            },
            None,
            true,
        )
        .await
        .expect("hosted web_search loop should finish");

        assert_eq!(final_response.status(), StatusCode::OK);
        assert!(final_response
            .headers()
            .contains_key(HOSTED_TOOL_LOOP_HEADER));
        let final_body = serde_json::from_slice::<Value>(
            &final_response
                .bytes_with_limit(MAX_RESPONSE_BODY_BYTES)
                .await
                .unwrap(),
        )
        .expect("final chat response json");
        assert_eq!(
            final_body["choices"][0]["message"]["content"],
            "Search was unavailable."
        );

        let sent = sent_bodies.lock().unwrap();
        assert_eq!(sent.len(), 1, "loop should resend Chat request once");
        assert_eq!(sent[0]["stream"], false);
        assert!(sent[0].get("stream_options").is_none());
        let messages = sent[0]["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_search");
        let tool_content: Value = serde_json::from_str(
            messages[2]["content"]
                .as_str()
                .expect("tool content should be JSON string"),
        )
        .expect("tool content json");
        assert!(tool_content.get("error").is_some());
        assert_eq!(tool_content["sources"], json!([]));
    }

    /// 验证 hosted image_generation loop 会消费第一轮工具调用并回灌错误或结果。
    #[tokio::test]
    async fn hosted_image_generation_loop_appends_tool_output_and_marks_response() {
        use crate::proxy::providers::hosted_tools::image_generation::HostedImageGenerationConfig;

        let initial_response = ProxyResponse::buffered(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::from(
                json!({
                    "id": "chatcmpl_image",
                    "object": "chat.completion",
                    "created": 1,
                    "model": "kimi-k3",
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "tool_calls": [{
                                "id": "call_image",
                                "type": "function",
                                "function": {
                                    "name": "generate_image",
                                    "arguments": "{\"prompt\":\"a robot\"}"
                                }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }]
                })
                .to_string(),
            ),
        );
        let mut chat_request = json!({
            "model": "kimi-k3",
            "messages": [{ "role": "user", "content": "Generate." }],
            "stream": true,
            "stream_options": { "include_usage": true }
        });
        let sent_bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sent_bodies_for_closure = sent_bodies.clone();

        let final_response = run_hosted_tool_chat_loop(
            initial_response,
            &mut chat_request,
            &HostedToolLoopConfig {
                web_search: None,
                image_generation: Some(HostedImageGenerationConfig::default()),
            },
            &Err("test no credentials".to_string()),
            move |body| {
                let sent_bodies = sent_bodies_for_closure.clone();
                let body = body.clone();
                async move {
                    sent_bodies.lock().unwrap().push(body);
                    Ok(ProxyResponse::buffered(
                        StatusCode::OK,
                        HeaderMap::new(),
                        Bytes::from(
                            json!({
                                "id": "chatcmpl_image_final",
                                "object": "chat.completion",
                                "created": 2,
                                "model": "kimi-k3",
                                "choices": [{
                                    "message": {
                                        "role": "assistant",
                                        "content": "Image generation was unavailable."
                                    },
                                    "finish_reason": "stop"
                                }]
                            })
                            .to_string(),
                        ),
                    ))
                }
            },
            None,
            true,
        )
        .await
        .expect("hosted image_generation loop should finish");

        assert_eq!(final_response.status(), StatusCode::OK);
        assert!(final_response
            .headers()
            .contains_key(HOSTED_TOOL_LOOP_HEADER));
        let final_body = serde_json::from_slice::<Value>(
            &final_response
                .bytes_with_limit(MAX_RESPONSE_BODY_BYTES)
                .await
                .unwrap(),
        )
        .expect("final chat response json");
        assert_eq!(
            final_body["choices"][0]["message"]["content"],
            "Image generation was unavailable."
        );

        let sent = sent_bodies.lock().unwrap();
        assert_eq!(sent.len(), 1, "loop should resend Chat request once");
        assert_eq!(sent[0]["stream"], false);
        assert!(sent[0].get("stream_options").is_none());
        let messages = sent[0]["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_image");
        let tool_content: Value = serde_json::from_str(
            messages[2]["content"]
                .as_str()
                .expect("tool content should be JSON string"),
        )
        .expect("tool content json");
        assert!(tool_content.get("error").is_some());
        assert_eq!(tool_content["artifact_path"], "");
    }

    #[test]
    fn exact_header_case_preserved_for_native_claude_only() {
        let provider = test_provider_with_type(None);

        assert!(should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some("anthropic"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Claude",
            &provider,
            Some("openai_responses"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Codex", &provider, None, false
        ));
        assert!(!should_preserve_exact_header_case(
            "Gemini", &provider, None, false
        ));
    }

    #[test]
    fn exact_header_case_skipped_for_codex_oauth_and_copilot() {
        let codex_oauth = test_provider_with_type(Some("codex_oauth"));
        let copilot = test_provider_with_type(Some("github_copilot"));

        assert!(!should_preserve_exact_header_case(
            "Claude",
            &codex_oauth,
            Some("openai_responses"),
            false
        ));
        assert!(!should_preserve_exact_header_case(
            "Claude",
            &copilot,
            Some("openai_chat"),
            true
        ));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_strips_beta_for_chat_completions() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&foo=bar",
            "openai_chat",
            false,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_strips_beta_for_responses() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/claude/v1/messages?beta=true&x-id=1",
            "openai_responses",
            false,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/responses?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_codex_responses_endpoint_to_chat_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_chat("/v1/responses?foo=bar");

        assert_eq!(endpoint, "/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn prepend_claude_code_system_prompt_from_string() {
        let mut body = json!({ "system": "You are a Codex agent." });
        prepend_claude_code_system_prompt(&mut body);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
        assert_eq!(system[1]["text"], "You are a Codex agent.");
    }

    #[test]
    fn prepend_claude_code_system_prompt_when_absent() {
        let mut body = json!({});
        prepend_claude_code_system_prompt(&mut body);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
    }

    #[test]
    fn prepend_claude_code_system_prompt_is_idempotent() {
        let mut body = json!({ "system": "orig" });
        prepend_claude_code_system_prompt(&mut body);
        prepend_claude_code_system_prompt(&mut body);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 2);
        assert_eq!(system[0]["text"], CLAUDE_CODE_SYSTEM_IDENTITY);
        assert_eq!(system[1]["text"], "orig");
    }

    #[test]
    fn rewrite_codex_responses_endpoint_to_anthropic_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_anthropic("/responses?x=1");
        assert_eq!(endpoint, "/v1/messages?x=1");
        assert_eq!(passthrough_query.as_deref(), Some("x=1"));

        let (endpoint, _) = rewrite_codex_responses_endpoint_to_anthropic("/v1/responses");
        assert_eq!(endpoint, "/v1/messages");
    }

    #[test]
    fn codex_anthropic_full_endpoint_guard_avoids_double_messages() {
        // On the Codex→Anthropic path a base URL already ending in `/v1/messages` (switch
        // off) must be treated as a full endpoint by the real `base_url_is_full_endpoint`.

        // Without the guard, build_url would concatenate the pasted endpoint with the
        // rewritten `/v1/messages` target, producing a broken double suffix.
        use super::super::providers::ProviderAdapter;
        let doubled = super::super::providers::CodexAdapter::new()
            .build_url("https://host.example/v1/messages", "/v1/messages");
        assert_eq!(doubled, "https://host.example/v1/messages/v1/messages");

        // With the guard, the pasted URL is used verbatim (plus preserved query). Includes
        // query/fragment/whitespace suffixes, which must not hide the endpoint (fix: a base
        // like `.../v1/messages?beta=true` previously evaded the suffix check).
        for base in [
            "https://host.example/v1/messages",
            "https://host.example/v1/messages/",
            "https://host.example/api/v1/messages", // prefixed gateway
            "https://host.example/v1/messages?beta=true",
            "https://host.example/v1/messages/?beta=true",
            "https://host.example/v1/messages#frag",
            "  https://host.example/v1/messages  ",
        ] {
            assert!(
                base_url_is_full_endpoint(base, "/v1/messages"),
                "expected full-endpoint match: {base:?}"
            );
        }
        assert_eq!(
            append_query_to_full_url("https://host.example/v1/messages", Some("x=1")),
            "https://host.example/v1/messages?x=1"
        );
        // A base URL that already carries its own query is preserved verbatim (no double
        // `/v1/messages`, query kept).
        assert_eq!(
            append_query_to_full_url("https://host.example/v1/messages?beta=true", None),
            "https://host.example/v1/messages?beta=true"
        );

        // A non-endpoint base (origin/prefix) must NOT match, so build_url still appends.
        assert!(!base_url_is_full_endpoint(
            "https://host.example",
            "/v1/messages"
        ));
        assert!(!base_url_is_full_endpoint(
            "https://host.example/v1",
            "/v1/messages"
        ));
        // The shared helper also backs the Chat path's `/chat/completions` guard.
        assert!(base_url_is_full_endpoint(
            "https://host.example/v1/chat/completions?api-version=2024",
            "/chat/completions"
        ));
    }

    #[test]
    fn codex_client_fingerprint_headers_are_dropped_for_anthropic_upstreams() {
        // Codex/OpenAI fingerprints a native Claude Code client never sends → must drop.
        for header in [
            "originator",
            "session_id",
            "session-id",
            "thread-id",
            "conversation_id",
            "chatgpt-account-id",
            "x-openai-subagent",
            "x-client-request-id",
            "x-codex-window-id",
            "openai-beta",
            "openai-organization",
            "openai-project",
            "x-stainless-lang",
            "x-stainless-runtime",
            "x-codex-turn-id",
        ] {
            assert!(
                is_codex_client_fingerprint_header(header),
                "expected {header} to be dropped while impersonating Claude Code"
            );
        }

        // Headers a real Claude Code client sends (or that the forwarder rebuilds) must
        // NOT be caught by the denylist.
        for header in [
            "anthropic-version",
            "anthropic-beta",
            "user-agent",
            "accept",
            "content-type",
            "x-app",
        ] {
            assert!(
                !is_codex_client_fingerprint_header(header),
                "{header} must be preserved while impersonating Claude Code"
            );
        }
    }

    #[test]
    fn codex_anthropic_2xx_error_envelope_is_detected_for_failover() {
        let body = br#"{"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#;
        assert_eq!(
            codex_anthropic_error_envelope_message(body).as_deref(),
            Some("overloaded_error: busy")
        );
        assert!(
            codex_anthropic_error_envelope_message(br#"{"type":"message","content":[]}"#).is_none()
        );
    }

    #[test]
    fn responses_2xx_failure_is_detected_for_failover() {
        assert_eq!(
            responses_error_envelope_message(
                br#"{"status":"failed","error":{"type":"server_error","message":"busy"},"output":[]}"#
            )
            .as_deref(),
            Some("server_error: busy")
        );
        assert_eq!(
            responses_error_envelope_message(br#"{"status":"cancelled","output":[]}"#).as_deref(),
            Some("cancelled: response generation was cancelled")
        );
        assert!(responses_error_envelope_message(
            br#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[]}"#
        )
        .is_none());
        assert!(responses_error_envelope_message(
            br#"{"status":"completed","error":null,"output":[]}"#
        )
        .is_none());
    }

    #[test]
    fn responses_stream_start_semantic_failure_is_retryable() {
        let created = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\"}}"
        );
        assert!(inspect_responses_start_event(created).is_none());

        let failed = concat!(
            "event: response.failed\n",
            "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"type\":\"server_error\",\"message\":\"boom\"}}}"
        );
        assert!(matches!(
            inspect_responses_start_event(failed),
            Some(Err(ProxyError::TransformError(message))) if message.contains("boom")
        ));

        let delta = concat!(
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}"
        );
        assert!(matches!(inspect_responses_start_event(delta), Some(Ok(()))));
    }

    #[test]
    fn responses_stream_start_accepts_unlabelled_whole_json() {
        assert!(matches!(
            inspect_responses_json_document(
                r#"{
                    "status": "completed",

                    "output": []
                }"#
            ),
            Some(Ok(()))
        ));
        assert!(inspect_responses_json_document(r#"{"status":"completed""#).is_none());

        let failed = inspect_responses_json_document(
            r#"{"status":"failed","error":{"message":"backend unavailable"}}"#,
        );
        assert!(
            matches!(failed, Some(Err(ProxyError::TransformError(message))) if message.contains("backend unavailable"))
        );
    }

    #[test]
    fn codex_anthropic_cache_is_default_on_but_honors_sub_switch() {
        let default = codex_anthropic_cache_config(&OptimizerConfig::default());
        assert!(default.enabled);
        assert!(default.cache_injection);

        let disabled = codex_anthropic_cache_config(&OptimizerConfig {
            cache_injection: false,
            ..OptimizerConfig::default()
        });
        assert!(disabled.enabled);
        assert!(!disabled.cache_injection);
    }

    #[test]
    fn invalid_client_history_is_not_retryable() {
        let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        let provider = test_provider_with_type(None);
        assert_eq!(
            forwarder.categorize_proxy_error(
                &ProxyError::InvalidRequest("invalid historical tool arguments".to_string()),
                &provider,
            ),
            ErrorCategory::NonRetryable
        );
    }

    #[test]
    fn official_codex_auth_failures_are_not_retryable() {
        let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        let mut provider = test_provider_with_type(None);
        provider.id = "codex-official".to_string();
        provider.category = Some("official".to_string());

        for error in [
            ProxyError::AuthError("restart Codex".to_string()),
            ProxyError::UpstreamError {
                status: 401,
                body: None,
            },
            ProxyError::UpstreamError {
                status: 403,
                body: None,
            },
        ] {
            assert_eq!(
                forwarder.categorize_proxy_error(&error, &provider),
                ErrorCategory::NonRetryable
            );
        }
    }

    #[test]
    fn xai_oauth_token_auth_failures_are_not_retryable() {
        let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        let provider = test_provider_with_type(Some("xai_oauth"));

        // 本地取 token 失败 = 账号级问题（需重新登录），failover 无济于事
        assert_eq!(
            forwarder.categorize_proxy_error(
                &ProxyError::AuthError("xAI OAuth 认证失败".to_string()),
                &provider,
            ),
            ErrorCategory::NonRetryable
        );
        // 上游 401/403 保持 Retryable：换 provider 可能持有可用的 key
        assert_eq!(
            forwarder.categorize_proxy_error(
                &ProxyError::UpstreamError {
                    status: 401,
                    body: None,
                },
                &provider,
            ),
            ErrorCategory::Retryable
        );
    }

    #[test]
    fn response_pending_is_not_retryable() {
        let forwarder = test_forwarder(Duration::ZERO, Duration::ZERO);
        let provider = test_provider_with_type(None);
        assert_eq!(
            forwarder.categorize_proxy_error(
                &ProxyError::ResponsePending(
                    "upstream may still be processing the request".to_string(),
                ),
                &provider,
            ),
            ErrorCategory::NonRetryable
        );
    }

    #[test]
    fn reqwest_connect_timeout_is_forward_failed_not_response_pending() {
        let err = map_reqwest_send_error_class(
            true,
            true,
            false,
            true,
            "error_sending_request".to_string(),
        );

        assert!(matches!(
            err,
            ProxyError::ForwardFailed(message) if message.contains("上游连接失败")
        ));
    }

    #[test]
    fn reqwest_in_flight_timeout_stays_response_pending() {
        let err = map_reqwest_send_error_class(false, true, false, true, "late result".to_string());

        assert!(matches!(
            err,
            ProxyError::ResponsePending(message) if message.contains("上游响应等待超时")
        ));
    }

    #[test]
    fn reqwest_send_request_disconnect_is_not_treated_as_pre_send_build_failure() {
        let err = map_reqwest_send_error_class(
            false,
            false,
            false,
            true,
            "client_error (SendRequest): connection_closed_before_message_completed".to_string(),
        );

        assert!(matches!(
            err,
            ProxyError::ResponsePending(message)
                if message.contains("connection_closed_before_message_completed")
        ));
    }

    #[test]
    fn official_codex_rejects_stale_proxy_placeholder_with_restart_hint() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer PROXY_MANAGED"),
        );
        let error = validate_codex_official_authorization(&headers)
            .expect_err("stale placeholder must be rejected");
        assert!(matches!(error, ProxyError::AuthError(message) if message.contains("重启 Codex")));
    }

    #[test]
    fn rewrite_codex_responses_compact_endpoint_to_chat_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_chat("/v1/responses/compact?foo=bar");

        assert_eq!(endpoint, "/chat/completions?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn rewrite_codex_responses_compact_endpoint_to_messages_preserves_query() {
        let (endpoint, passthrough_query) =
            rewrite_codex_responses_endpoint_to_messages("/responses/compact?foo=bar");

        assert_eq!(endpoint, "/v1/messages?foo=bar");
        assert_eq!(passthrough_query.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn codex_metadata_summary_detects_local_compaction_from_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-codex-turn-metadata",
            HeaderValue::from_static(
                r#"{"request_kind":"compaction","compaction":{"trigger":"auto","reason":"token_limit","implementation":"responses","phase":"pre_turn"}}"#,
            ),
        );

        let summary = CodexRequestMetadataSummary::from_request(
            &AppType::Codex,
            "/v1/responses",
            &json!({"model":"gpt-5.5"}),
            &headers,
        );

        assert_eq!(summary.request_kind, "compaction");
        assert_eq!(summary.compaction_trigger.as_deref(), Some("auto"));
        assert_eq!(summary.compaction_reason.as_deref(), Some("token_limit"));
        assert_eq!(
            summary.compaction_implementation.as_deref(),
            Some("responses")
        );
        assert_eq!(summary.compaction_phase.as_deref(), Some("pre_turn"));
    }

    #[test]
    fn codex_metadata_summary_marks_compaction_by_endpoint_without_metadata() {
        let summary = CodexRequestMetadataSummary::from_request(
            &AppType::Codex,
            "/v1/responses/compact?conversation=1",
            &json!({"model":"gpt-5.5"}),
            &HeaderMap::new(),
        );

        assert_eq!(summary.request_kind, "compaction");
    }

    #[test]
    fn codex_request_v2_compaction_only_matches_remote_v2_contract() {
        let mut v2_headers = HeaderMap::new();
        v2_headers.insert(
            "x-codex-turn-metadata",
            HeaderValue::from_static(
                r#"{"request_kind":"compaction","compaction":{"trigger":"auto","implementation":"responses_compaction_v2","phase":"pre_turn"}}"#,
            ),
        );
        assert!(codex_request_is_v2_compaction(
            &AppType::Codex,
            "/v1/responses",
            &json!({"input":[{"type":"compaction_trigger"}]}),
            &v2_headers,
        ));

        assert!(!codex_request_is_v2_compaction(
            &AppType::Codex,
            "/v1/responses/compact",
            &json!({"model":"gpt-5.5"}),
            &HeaderMap::new(),
        ));

        let mut local_headers = HeaderMap::new();
        local_headers.insert(
            "x-codex-turn-metadata",
            HeaderValue::from_static(
                r#"{"request_kind":"compaction","compaction":{"trigger":"auto","implementation":"responses","phase":"pre_turn"}}"#,
            ),
        );
        assert!(!codex_request_is_v2_compaction(
            &AppType::Codex,
            "/v1/responses",
            &json!({"input":[{"type":"compaction_trigger"}]}),
            &local_headers,
        ));

        let mut legacy_v2_headers = HeaderMap::new();
        legacy_v2_headers.insert(
            "x-codex-turn-metadata",
            HeaderValue::from_static(
                r#"{"request_kind":"compaction","compaction":{"trigger":"auto"}}"#,
            ),
        );
        assert!(codex_request_is_v2_compaction(
            &AppType::Codex,
            "/v1/responses",
            &json!({"input":[{"type":"compaction_trigger"}]}),
            &legacy_v2_headers,
        ));
    }

    #[test]
    fn codex_metadata_summary_falls_back_to_body_when_header_is_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-codex-turn-metadata",
            HeaderValue::from_static("not-json"),
        );
        let summary = CodexRequestMetadataSummary::from_request(
            &AppType::Codex,
            "/v1/responses/compact",
            &json!({
                "client_metadata": {
                    "x-codex-turn-metadata": {
                        "request_kind": "compaction",
                        "compaction": {"reason": "model_downshift", "phase": "post_turn"}
                    }
                }
            }),
            &headers,
        );

        assert_eq!(summary.request_kind, "compaction");
        assert_eq!(
            summary.compaction_reason.as_deref(),
            Some("model_downshift")
        );
        assert_eq!(summary.compaction_phase.as_deref(), Some("post_turn"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_uses_copilot_path() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "anthropic",
            true,
            &json!({ "model": "claude-sonnet-4-6" }),
        );

        assert_eq!(endpoint, "/chat/completions?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_uses_copilot_responses_path() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "openai_responses",
            true,
            &json!({ "model": "gpt-5.4" }),
        );

        assert_eq!(endpoint, "/v1/responses?x-id=1");
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    #[test]
    fn rewrite_claude_transform_endpoint_maps_gemini_generate_content() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true&x-id=1",
            "gemini_native",
            false,
            &json!({ "model": "gemini-2.5-pro" }),
        );

        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-2.5-pro:generateContent?x-id=1"
        );
        assert_eq!(passthrough_query.as_deref(), Some("x-id=1"));
    }

    /// Regression: body.model arriving as the resource-name form
    /// `models/gemini-2.5-pro` must not produce a doubled
    /// `/v1beta/models/models/...` path.
    #[test]
    fn rewrite_claude_transform_endpoint_strips_gemini_model_resource_prefix() {
        let (endpoint, _) = rewrite_claude_transform_endpoint(
            "/v1/messages",
            "gemini_native",
            false,
            &json!({ "model": "models/gemini-2.5-pro" }),
        );

        assert_eq!(endpoint, "/v1beta/models/gemini-2.5-pro:generateContent");
    }

    #[test]
    fn rewrite_claude_transform_endpoint_maps_gemini_streaming() {
        let (endpoint, passthrough_query) = rewrite_claude_transform_endpoint(
            "/v1/messages?beta=true",
            "gemini_native",
            false,
            &json!({ "model": "gemini-2.5-flash", "stream": true }),
        );

        assert_eq!(
            endpoint,
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
        assert_eq!(passthrough_query.as_deref(), Some("alt=sse"));
    }

    #[test]
    fn append_query_to_full_url_preserves_existing_query_string() {
        let url = append_query_to_full_url("https://relay.example/api?foo=bar", Some("x-id=1"));

        assert_eq!(url, "https://relay.example/api?foo=bar&x-id=1");
    }

    #[test]
    fn build_gemini_native_url_uses_origin_when_base_ends_with_v1beta() {
        let url = crate::proxy::gemini_url::build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta",
            "/v1beta/models/gemini-2.5-pro:generateContent",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent"
        );
    }

    #[test]
    fn build_gemini_native_url_uses_origin_when_base_already_contains_models_prefix() {
        let url = crate::proxy::gemini_url::build_gemini_native_url(
            "https://generativelanguage.googleapis.com/v1beta/models",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        );

        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn resolve_gemini_native_url_keeps_opaque_full_url_as_is() {
        let url = crate::proxy::gemini_url::resolve_gemini_native_url(
            "https://relay.example/custom/generate-content",
            "/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            true,
        );

        assert_eq!(url, "https://relay.example/custom/generate-content?alt=sse");
    }

    #[test]
    fn force_identity_for_stream_flag_requests() {
        let headers = HeaderMap::new();

        assert!(should_force_identity_encoding(
            "/v1/responses",
            &json!({ "stream": true }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_gemini_stream_endpoints() {
        let headers = HeaderMap::new();

        assert!(should_force_identity_encoding(
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
            &json!({ "model": "gemini-2.5-pro" }),
            &headers
        ));
    }

    #[test]
    fn streaming_request_detects_gemini_sse_without_body_stream_flag() {
        let headers = HeaderMap::new();

        assert!(is_streaming_request(
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
            &json!({ "model": "gemini-2.5-pro" }),
            &headers
        ));
    }

    #[test]
    fn force_identity_for_sse_accept_header() {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));

        assert!(should_force_identity_encoding(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

    #[test]
    fn non_streaming_requests_allow_automatic_compression() {
        let headers = HeaderMap::new();

        assert!(!should_force_identity_encoding(
            "/v1/responses",
            &json!({ "model": "gpt-5" }),
            &headers
        ));
    }

    // ==================== Copilot 动态 endpoint 路由相关测试 ====================

    /// 验证 is_copilot 检测逻辑：通过 provider_type 判断
    #[test]
    fn copilot_detection_via_provider_type() {
        use crate::provider::{Provider, ProviderMeta};

        let provider = Provider {
            id: "test".to_string(),
            name: "Test Copilot".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot");

        assert!(is_copilot, "应该通过 provider_type 检测为 Copilot");
    }

    /// 验证 is_copilot 检测逻辑：通过 base_url 判断
    #[test]
    fn copilot_detection_via_base_url() {
        let base_url = "https://api.githubcopilot.com";
        let is_copilot = base_url.contains("githubcopilot.com");
        assert!(is_copilot, "应该通过 base_url 检测为 Copilot");

        let non_copilot_url = "https://api.anthropic.com";
        let is_not_copilot = non_copilot_url.contains("githubcopilot.com");
        assert!(!is_not_copilot, "非 Copilot URL 不应被检测为 Copilot");
    }

    /// 验证企业版 endpoint（不包含 githubcopilot.com）场景下 is_copilot 仍然正确
    #[test]
    fn copilot_detection_for_enterprise_endpoint() {
        use crate::provider::{Provider, ProviderMeta};

        // 企业版场景：provider_type 是 github_copilot，但 base_url 可能是企业内部域名
        let provider = Provider {
            id: "enterprise".to_string(),
            name: "Enterprise Copilot".to_string(),
            settings_config: serde_json::json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                provider_type: Some("github_copilot".to_string()),
                ..Default::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        let enterprise_base_url = "https://copilot-api.corp.example.com";

        // is_copilot 应该通过 provider_type 检测成功，即使 base_url 不包含 githubcopilot.com
        let is_copilot = provider
            .meta
            .as_ref()
            .and_then(|m| m.provider_type.as_deref())
            == Some("github_copilot")
            || enterprise_base_url.contains("githubcopilot.com");

        assert!(
            is_copilot,
            "企业版 Copilot 应该通过 provider_type 被正确检测"
        );
    }

    /// 验证动态 endpoint 替换条件
    #[test]
    fn dynamic_endpoint_replacement_conditions() {
        // 条件：is_copilot && !is_full_url
        let test_cases = [
            (true, false, true, "Copilot + 非 full_url 应该替换"),
            (true, true, false, "Copilot + full_url 不应替换"),
            (false, false, false, "非 Copilot 不应替换"),
            (false, true, false, "非 Copilot + full_url 不应替换"),
        ];

        for (is_copilot, is_full_url, should_replace, desc) in test_cases {
            let will_replace = is_copilot && !is_full_url;
            assert_eq!(will_replace, should_replace, "{desc}");
        }
    }

    // ===== P3: forwarder 层 media 开关回归测试 =====
    // 验证 gate 在 forwarder 这一层的"接线"，而非 media_sanitizer 纯函数本身。

    fn forwarder_with_rectifier(config: RectifierConfig) -> RequestForwarder {
        let mut fwd = test_forwarder(Duration::from_secs(1), Duration::from_secs(1));
        fwd.rectifier_config = config;
        fwd
    }

    fn provider_with_settings(settings_config: Value) -> Provider {
        let mut p = test_provider_with_type(Some("anthropic"));
        p.settings_config = settings_config;
        p
    }

    fn body_with_image(model: &str) -> Value {
        json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        })
    }

    fn body_with_codex_input_image(model: &str) -> Value {
        json!({
            "model": model,
            "input": [{
                "role": "user",
                "content": [
                    { "type": "input_image", "image_url": "data:image/png;base64,abc" }
                ]
            }]
        })
    }

    #[test]
    fn raw_passthrough_prefers_official_route_over_nonofficial_default_route() {
        let provider = provider_with_settings(json!({
            "codexRouting": {
                "enabled": true,
                "defaultRouteId": "deepseek",
                "routes": [
                    {
                        "id": "official",
                        "label": "OpenAI Official",
                        "match": { "models": ["gpt-5.5"] },
                        "upstream": {
                            "targetProviderId": "codex-official",
                            "auth": { "source": "managed_codex_oauth" }
                        }
                    },
                    {
                        "id": "deepseek",
                        "label": "DeepSeek",
                        "match": { "models": ["deepseek-v4-flash"] },
                        "upstream": {
                            "baseUrl": "https://api.deepseek.com",
                            "apiKey": "sk-deepseek"
                        }
                    }
                ]
            }
        }));

        let resolved = resolve_codex_raw_passthrough_route_provider(
            &provider,
            &json!({ "model": "gpt-image-1" }),
        )
        .expect("raw route should resolve");

        assert_eq!(resolved.settings_config["codexResolvedRouteId"], "official");
        assert_eq!(
            resolved.settings_config["codexResolvedRouteMatched"], false,
            "OpenAI 原生 endpoint 的 fallback 不是模型显式命中"
        );
    }

    #[test]
    fn raw_passthrough_empty_body_uses_official_route_not_default_route() {
        let provider = provider_with_settings(json!({
            "codexRouting": {
                "enabled": true,
                "defaultRouteId": "deepseek",
                "routes": [
                    {
                        "id": "official",
                        "label": "OpenAI Official",
                        "match": { "models": ["gpt-5.5"] },
                        "upstream": {
                            "targetProviderId": "codex-official",
                            "auth": { "source": "managed_codex_oauth" }
                        }
                    },
                    {
                        "id": "deepseek",
                        "label": "DeepSeek",
                        "match": { "models": ["deepseek-v4-flash"] },
                        "upstream": {
                            "baseUrl": "https://api.deepseek.com",
                            "apiKey": "sk-deepseek"
                        }
                    }
                ]
            }
        }));

        let resolved = resolve_codex_raw_passthrough_route_provider(&provider, &json!({}))
            .expect("empty realtime body should still resolve official");

        assert_eq!(resolved.settings_config["codexResolvedRouteId"], "official");
        assert_eq!(
            resolved.settings_config["codexResolvedRouteMatched"], false,
            "空 body 的 GPT-Live 端点不是模型显式命中"
        );
    }

    #[test]
    fn raw_passthrough_keeps_explicit_nonofficial_model_match() {
        let provider = provider_with_settings(json!({
            "codexRouting": {
                "enabled": true,
                "defaultRouteId": "official",
                "routes": [
                    {
                        "id": "official",
                        "label": "OpenAI Official",
                        "match": { "models": ["gpt-5.5"] },
                        "upstream": {
                            "targetProviderId": "codex-official",
                            "auth": { "source": "managed_codex_oauth" }
                        }
                    },
                    {
                        "id": "deepseek",
                        "label": "DeepSeek",
                        "match": { "models": ["deepseek-v4-flash"] },
                        "upstream": {
                            "baseUrl": "https://api.deepseek.com",
                            "apiKey": "sk-deepseek"
                        }
                    }
                ]
            }
        }));

        let resolved = resolve_codex_raw_passthrough_route_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("raw route should resolve");

        assert_eq!(resolved.settings_config["codexResolvedRouteId"], "deepseek");
        assert_eq!(
            resolved.settings_config["codexResolvedRouteMatched"], true,
            "显式匹配第三方模型时不能被 raw official fallback 抢走"
        );
    }

    #[test]
    fn raw_passthrough_unknown_model_does_not_use_nonofficial_default_route() {
        let provider = provider_with_settings(json!({
            "codexRouting": {
                "enabled": true,
                "defaultRouteId": "deepseek",
                "routes": [
                    {
                        "id": "deepseek",
                        "label": "DeepSeek",
                        "match": { "models": ["deepseek-v4-flash"] },
                        "upstream": {
                            "baseUrl": "https://api.deepseek.com",
                            "apiKey": "sk-deepseek"
                        }
                    }
                ]
            }
        }));

        let resolved = resolve_codex_raw_passthrough_route_provider(
            &provider,
            &json!({ "model": "gpt-image-1" }),
        );

        assert!(
            resolved.is_none(),
            "raw passthrough 未知 GPT App 原生请求不能退到非 official defaultRouteId"
        );
    }

    #[test]
    fn codex_realtime_live_path_normalizes_local_aliases_and_call_ids() {
        assert_eq!(
            codex_realtime_live_path("/v1/live").as_deref(),
            Some("/v1/live")
        );
        assert_eq!(
            codex_realtime_live_path("/codex/v1/live").as_deref(),
            Some("/v1/live")
        );
        assert_eq!(
            codex_realtime_live_path("/v1/v1/live/rtc_123").as_deref(),
            Some("/v1/live/rtc_123")
        );
        assert_eq!(
            codex_realtime_live_path("/live/rtc_123").as_deref(),
            Some("/v1/live/rtc_123")
        );
        assert_eq!(codex_realtime_live_path("/v1/responses").as_deref(), None);
        assert!(!codex_realtime_live_call_path("/v1/live/rtc_123"));
        assert!(codex_realtime_live_call_path("/codex/v1/live"));
    }

    #[test]
    fn codex_realtime_multipart_parses_sdp_and_session() {
        let boundary = "codex-realtime-call-boundary";
        let body = format!(
            "--{boundary}\r\n\
             Content-Disposition: form-data; name=\"sdp\"\r\n\
             Content-Type: application/sdp\r\n\
             \r\n\
             v=offer\r\n\
             o=- 1 1 IN IP4 127.0.0.1\r\n\
             s=codex\r\n\
             \r\n\
             --{boundary}\r\n\
             Content-Disposition: form-data; name=\"session\"\r\n\
             Content-Type: application/json\r\n\
             \r\n\
             {{\"model\":\"gpt-live-1-codex\",\"delegation\":{{\"type\":\"client\"}}}}\r\n\
             --{boundary}--\r\n"
        );
        let (sdp, session) = codex_realtime_multipart_payload(
            body.as_bytes(),
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .expect("parse realtime multipart");
        assert!(sdp.starts_with("v=offer"));
        assert_eq!(session["model"], "gpt-live-1-codex");
        assert_eq!(session["delegation"]["type"], "client");
    }

    #[tokio::test]
    async fn codex_realtime_websocket_connects_to_official_backend() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind realtime ws mock");
        let addr = listener.local_addr().expect("realtime ws mock addr");
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept realtime ws mock");
            let mut ws = tokio_tungstenite::accept_async(stream)
                .await
                .expect("accept realtime websocket");
            ws.send(tokio_tungstenite::tungstenite::Message::Text(
                "hello".to_string(),
            ))
            .await
            .expect("send realtime ws message");
        });
        let db = Arc::new(Database::memory().expect("memory db"));
        db.save_provider(
            "codex",
            &Provider::with_id(
                "codex-official".to_string(),
                "OpenAI Official".to_string(),
                json!({
                    "base_url": format!("ws://{addr}/backend-api/codex"),
                    "api_key": "sk-official"
                }),
                None,
            ),
        )
        .expect("save official provider");
        db.save_provider(
            "codex",
            &Provider::with_id(
                "router".to_string(),
                "Codex Router".to_string(),
                json!({
                    "codexRouting": {
                        "enabled": true,
                        "defaultRouteId": "deepseek",
                        "routes": [
                            {
                                "id": "official",
                                "label": "OpenAI Official",
                                "enabled": true,
                                "targetProviderId": "codex-official",
                                "match": { "models": ["gpt-5.6-sol"] },
                                "upstream": { "apiFormat": "openai_responses" }
                            },
                            {
                                "id": "deepseek",
                                "label": "DeepSeek",
                                "enabled": true,
                                "match": { "models": ["deepseek-v4-flash"] },
                                "upstream": {
                                    "baseUrl": "https://api.deepseek.com",
                                    "apiKey": "sk-deepseek"
                                }
                            }
                        ]
                    }
                }),
                None,
            ),
        )
        .expect("save router provider");
        let mut proxy_config = db
            .get_proxy_config_for_app("codex")
            .await
            .expect("read codex proxy config");
        proxy_config.enabled = true;
        proxy_config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(proxy_config)
            .await
            .expect("enable codex failover queue");
        db.add_to_failover_queue("codex", "router")
            .expect("add router to failover queue");
        let forwarder = RequestForwarder {
            router: Arc::new(ProviderRouter::new(db.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            current_providers: Arc::new(RwLock::new(HashMap::new())),
            gemini_shadow: Arc::new(GeminiShadowStore::new()),
            codex_chat_history: Arc::new(CodexChatHistoryStore::default()),
            failover_manager: Arc::new(FailoverSwitchManager::new(db)),
            app_handle: None,
            current_provider_id_at_start: String::new(),
            session_id: String::new(),
            session_client_provided: false,
            preserve_codex_client_originator: false,
            rectifier_config: RectifierConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            copilot_optimizer_config: CopilotOptimizerConfig::default(),
            codex_responses_lite_fallbacks: Arc::new(RwLock::new(HashMap::new())),
            non_streaming_timeout: Duration::ZERO,
            streaming_first_byte_timeout: Duration::ZERO,
            max_attempts: 1,
        };

        let mut stream = forwarder
            .open_codex_realtime_websocket(
                &AppType::Codex,
                "/v1/live",
                &json!({}),
                &HeaderMap::new(),
            )
            .await
            .expect("connect realtime websocket");
        let message = stream
            .0
            .next()
            .await
            .expect("realtime ws message")
            .expect("realtime ws message ok");
        assert_eq!(message.into_text().expect("text message"), "hello");
        server.await.expect("realtime ws mock task");
    }

    fn body_with_codex_tool_output_image(stringified: bool) -> Value {
        let output = json!({
            "content": [{
                "type": "input_image",
                "image_url": "data:image/png;base64,TOOL_OUTPUT_SENTINEL"
            }]
        });
        json!({
            "model": "any-model",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": if stringified {
                    Value::String(output.to_string())
                } else {
                    output
                }
            }]
        })
    }

    fn body_with_stringified_chat_tool_image() -> Value {
        let content = json!({
            "content": [{
                "type": "image",
                "mimeType": "image/png",
                "data": "CHAT_TOOL_SENTINEL"
            }]
        })
        .to_string();
        json!({
            "model": "any-model",
            "messages": [{
                "role": "tool",
                "tool_call_id": "call_1",
                "content": content
            }]
        })
    }

    fn body_with_gemini_image() -> Value {
        json!({
            "contents": [{
                "role": "user",
                "parts": [{
                    "inlineData": {
                        "mimeType": "image/png",
                        "data": "GEMINI_SENTINEL"
                    }
                }]
            }]
        })
    }

    fn image_unsupported_error() -> ProxyError {
        ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"This model does not support image input"}}"#.to_string(),
            ),
        }
    }

    fn minimax_sensitive_image_error() -> ProxyError {
        ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"base_resp":{"status_code":1026,"status_msg":"input new_sensitive, messages[61]'s content[0] image is sensitive, please check your input"}}"#
                    .to_string(),
            ),
        }
    }
    #[test]
    fn prevention_replaces_when_all_switches_on_and_model_in_heuristic_list() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        let replaced = fwd.apply_media_prevention(&mut body, &provider);

        assert_eq!(replaced, 1, "默认全开 + 名单内模型应预替换");
        assert_eq!(body["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn prevention_skipped_when_media_fallback_off() {
        // 关闭 request_media_fallback：即使名单命中也不预替换。
        let fwd = forwarder_with_rectifier(RectifierConfig {
            request_media_fallback: false,
            ..RectifierConfig::default()
        });
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        let replaced = fwd.apply_media_prevention(&mut body, &provider);

        assert_eq!(replaced, 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn prevention_skipped_when_master_switch_off() {
        let fwd = forwarder_with_rectifier(RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        });
        let provider = provider_with_settings(json!({}));
        let mut body = body_with_image("deepseek-v4-pro");

        assert_eq!(fwd.apply_media_prevention(&mut body, &provider), 0);
        assert_eq!(body["messages"][0]["content"][0]["type"], "image");
    }

    #[test]
    fn prevention_heuristic_off_skips_list_but_keeps_explicit_text_only() {
        // 关闭 request_media_heuristic：名单预测失效，但显式声明 text-only 仍预替换。
        let fwd = forwarder_with_rectifier(RectifierConfig {
            request_media_heuristic: false,
            ..RectifierConfig::default()
        });

        // (a) 名单内模型、无显式声明 → 不再预替换
        let bare_provider = provider_with_settings(json!({}));
        let mut list_body = body_with_image("deepseek-v4-pro");
        assert_eq!(
            fwd.apply_media_prevention(&mut list_body, &bare_provider),
            0,
            "heuristic 关闭后名单模型不应被预替换"
        );
        assert_eq!(list_body["messages"][0]["content"][0]["type"], "image");

        // (b) 显式声明 text-only → 仍预替换（声明驱动，不受 heuristic 开关影响）
        let declared_provider = provider_with_settings(json!({
            "models": [ { "id": "some-text-model", "input": ["text"] } ]
        }));
        let mut declared_body = body_with_image("some-text-model");
        assert_eq!(
            fwd.apply_media_prevention(&mut declared_body, &declared_provider),
            1,
            "显式 text-only 即使关闭 heuristic 也应预替换"
        );
        assert_eq!(declared_body["messages"][0]["content"][0]["type"], "text");
    }

    #[test]
    fn reactive_triggers_when_all_switches_on() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let body = body_with_image("any-model");
        assert!(fwd.media_retry_should_trigger("Claude", false, &body, &image_unsupported_error()));
    }

    #[test]
    fn reactive_triggers_for_codex_image_url_deserialize_errors() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let body = body_with_codex_input_image("deepseek-v4-flash");
        let error = ProxyError::UpstreamError {
            status: 400,
            body: Some(
                r#"{"error":{"message":"Failed to deserialize the JSON body into the target type: messages[11]: unknown variant image_url, expected text"}}"#
                    .to_string(),
            ),
        };

        assert!(fwd.media_retry_should_trigger("Codex", false, &body, &error));
    }

    #[test]
    fn reactive_triggers_for_codex_sensitive_image_errors() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let body = body_with_codex_input_image("MiniMax-M3");

        assert!(fwd.media_retry_should_trigger(
            "Codex",
            false,
            &body,
            &minimax_sensitive_image_error()
        ));
    }

    #[test]
    fn reactive_triggers_for_structured_and_stringified_codex_tool_images() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());

        for stringified in [false, true] {
            let body = body_with_codex_tool_output_image(stringified);
            assert!(
                fwd.media_retry_should_trigger("Codex", false, &body, &image_unsupported_error()),
                "tool-output image should trigger retry (stringified={stringified})"
            );
        }
    }

    #[test]
    fn reactive_triggers_for_chat_tool_and_gemini_images() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());

        assert!(fwd.media_retry_should_trigger(
            "Claude",
            false,
            &body_with_stringified_chat_tool_image(),
            &image_unsupported_error()
        ));
        assert!(fwd.media_retry_should_trigger(
            "Claude",
            false,
            &body_with_gemini_image(),
            &image_unsupported_error()
        ));
    }

    #[test]
    fn reactive_sensitive_image_error_still_requires_image_body() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let body = json!({
            "model": "MiniMax-M3",
            "input": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            }]
        });

        assert!(!fwd.media_retry_should_trigger(
            "Codex",
            false,
            &body,
            &minimax_sensitive_image_error()
        ));
    }

    #[test]
    fn reactive_does_not_treat_context_limit_as_image_rejection() {
        let fwd = forwarder_with_rectifier(RectifierConfig::default());
        let body = body_with_codex_tool_output_image(false);
        let context_error = ProxyError::UpstreamError {
            status: 400,
            body: Some(r#"{"error":{"message":"maximum context length exceeded"}}"#.to_string()),
        };

        assert!(!fwd.media_retry_should_trigger("Codex", false, &body, &context_error));
    }

    #[test]
    fn reactive_skipped_when_media_fallback_off() {
        // 关闭 request_media_fallback：上游报图片错误也不触发兜底重试。
        let fwd = forwarder_with_rectifier(RectifierConfig {
            request_media_fallback: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(!fwd.media_retry_should_trigger(
            "Claude",
            false,
            &body,
            &image_unsupported_error()
        ));
    }

    #[test]
    fn reactive_skipped_when_master_switch_off() {
        let fwd = forwarder_with_rectifier(RectifierConfig {
            enabled: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(!fwd.media_retry_should_trigger(
            "Claude",
            false,
            &body,
            &image_unsupported_error()
        ));
    }

    #[test]
    fn reactive_unaffected_by_heuristic_switch() {
        // 关闭 request_media_heuristic 不影响反应式兜底——它是上游实测错误后的恢复，不是预测。
        let fwd = forwarder_with_rectifier(RectifierConfig {
            request_media_heuristic: false,
            ..RectifierConfig::default()
        });
        let body = body_with_image("any-model");
        assert!(fwd.media_retry_should_trigger("Claude", false, &body, &image_unsupported_error()));
    }

    #[tokio::test]
    async fn upstream_transport_retry_recovers_after_connect_failure() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_send = attempts.clone();

        let result = send_upstream_request_with_transport_retry("test", || {
            let attempts = attempts_for_send.clone();
            async move {
                if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Err(ProxyError::ForwardFailed(
                        "上游连接失败: error_sending_request".to_string(),
                    ))
                } else {
                    Ok(ProxyResponse::buffered(
                        http::StatusCode::OK,
                        http::HeaderMap::new(),
                        Bytes::new(),
                    ))
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn upstream_transport_retry_does_not_replay_response_pending() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_send = attempts.clone();

        let result = send_upstream_request_with_transport_retry("test", || {
            let attempts = attempts_for_send.clone();
            async move {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(ProxyError::ResponsePending(
                    "请求可能已在处理中".to_string(),
                ))
            }
        })
        .await;

        assert!(matches!(result, Err(ProxyError::ResponsePending(_))));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn upstream_transport_retry_stops_after_bounded_attempts() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_send = attempts.clone();

        let result = send_upstream_request_with_transport_retry("test", || {
            let attempts = attempts_for_send.clone();
            async move {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(ProxyError::ForwardFailed(
                    "上游请求构造失败: error_sending_request".to_string(),
                ))
            }
        })
        .await;

        assert!(matches!(result, Err(ProxyError::ForwardFailed(_))));
        assert_eq!(
            attempts.load(std::sync::atomic::Ordering::SeqCst),
            UPSTREAM_TRANSPORT_RETRY_LIMIT + 1
        );
    }

    #[tokio::test(start_paused = true)]
    async fn upstream_transport_retry_survives_five_safe_connect_failures() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_send = attempts.clone();

        let result = send_upstream_request_with_transport_retry("test", || {
            let attempts = attempts_for_send.clone();
            async move {
                if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 5 {
                    Err(ProxyError::ForwardFailed(
                        "上游连接失败: unexpected EOF during handshake".to_string(),
                    ))
                } else {
                    Ok(ProxyResponse::buffered(
                        http::StatusCode::OK,
                        http::HeaderMap::new(),
                        Bytes::new(),
                    ))
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 6);
    }

    #[tokio::test(start_paused = true)]
    async fn codex_rate_limit_retry_recovers_same_request_after_transient_429() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_send = attempts.clone();

        let result = send_codex_request_with_rate_limit_retry("test", || {
            let attempts = attempts_for_send.clone();
            async move {
                let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if attempt == 0 {
                    Ok(ProxyResponse::buffered(
                        http::StatusCode::TOO_MANY_REQUESTS,
                        http::HeaderMap::new(),
                        Bytes::from_static(
                            br#"{"error":{"type":"rate_limit_exceeded","message":"slow down"}}"#,
                        ),
                    ))
                } else {
                    Ok(ProxyResponse::buffered(
                        http::StatusCode::OK,
                        http::HeaderMap::new(),
                        Bytes::new(),
                    ))
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn codex_rate_limit_retry_honors_retry_after_seconds() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_send = attempts.clone();
        let started_at = tokio::time::Instant::now();

        let result = send_codex_request_with_rate_limit_retry("test", || {
            let attempts = attempts_for_send.clone();
            async move {
                let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if attempt == 0 {
                    let mut headers = http::HeaderMap::new();
                    headers.insert(http::header::RETRY_AFTER, "17".parse().unwrap());
                    Ok(ProxyResponse::buffered(
                        http::StatusCode::TOO_MANY_REQUESTS,
                        headers,
                        Bytes::from_static(br#"{"error":{"type":"rate_limit_exceeded"}}"#),
                    ))
                } else {
                    Ok(ProxyResponse::buffered(
                        http::StatusCode::OK,
                        http::HeaderMap::new(),
                        Bytes::new(),
                    ))
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(
            tokio::time::Instant::now() - started_at,
            Duration::from_secs(17)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn codex_rate_limit_retry_does_not_spin_on_usage_limit_reached() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_for_send = attempts.clone();

        let result = send_codex_request_with_rate_limit_retry("test", || {
            let attempts = attempts_for_send.clone();
            async move {
                attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(ProxyResponse::buffered(
                    http::StatusCode::TOO_MANY_REQUESTS,
                    http::HeaderMap::new(),
                    Bytes::from_static(
                        br#"{"error":{"type":"usage_limit_reached","message":"The usage limit has been reached"}}"#,
                    ),
                ))
            }
        })
        .await;

        assert_eq!(
            result.unwrap().status(),
            http::StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
