//! Codex (OpenAI) Provider Adapter
//!
//! 仅透传模式，支持直连 OpenAI API
//!
//! ## 客户端检测
//! 支持检测官方 Codex 客户端 (codex_vscode, codex_cli_rs)

use super::{AuthInfo, AuthStrategy, ProviderAdapter};
use crate::provider::{
    AuthBinding, AuthBindingSource, CodexCacheConfig, CodexChatReasoningConfig, Provider,
    ProviderMeta,
};
use crate::proxy::error::ProxyError;
use crate::proxy::providers::codex_oauth_auth::{CodexAccountPoolPolicy, NATIVE_CODEX_ACCOUNT_ID};
use regex::Regex;
use serde_json::{Map, Value as JsonValue};
use std::sync::LazyLock;
use toml::Value as TomlValue;

const CODEX_ROUTER_PARENT_PROVIDER_ID: &str = "codexRouterParentProviderId";
const CODEX_ROUTER_PARENT_PROVIDER_NAME: &str = "codexRouterParentProviderName";
const CODEX_RESOLVED_TARGET_PROVIDER_ID: &str = "codexResolvedTargetProviderId";
const CODEX_RESOLVED_UPSTREAM_MODEL_OVERRIDE: &str = "codexResolvedUpstreamModelOverride";
const CODEX_NATIVE_AUTH_PASSTHROUGH: &str = "codexNativeAuthPassthrough";
pub(crate) const CODEX_ACCOUNT_POOL_ENABLED: &str = "codexAccountPoolEnabled";
const QWEN_VLLM_MIN_OUTPUT_TOKENS: u64 = 2_048;
const RETIRED_QWEN_VLLM_DEFAULT_OUTPUT_TOKENS: u64 = 32_768;

/// Codex Desktop 看到的 MultiRouter 认证门面。
///
/// 该枚举只描述 Codex 到 CCSM 本地代理这一跳如何携带认证，不描述最终上游
/// 使用哪个账号。最终凭据所有权仍由每次请求解析出的 route 决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexMultiRouterAuthFacade {
    /// 至少一条启用 route 可能复用 Codex Desktop 当前登录。
    NativeMixed,
    /// 所有启用 route 的凭据都由 CCSM 或目标 provider 管理。
    ///
    /// live 门面仍保留 `requires_openai_auth = true` 和 `PROXY_MANAGED`，让
    /// Codex Desktop 继续显示账号/用量/退出登录，同时实际出站凭据仍由 CCSM 接管。
    FullyManaged,
    /// 旧配置缺少足够信息，投影层必须保留现有 live 认证门面。
    LegacyPreserved,
}

/// 根据持久化 Router 配置和账号池策略判断本地认证门面。
///
/// route 是运行时认证所有权的最终事实；`officialAuth` 由保存流程物化到 route，
/// 这里只用它区分新配置和缺少 auth source 的旧歧义配置，不用它覆盖 route。
pub fn classify_codex_multirouter_auth_facade(
    provider: &Provider,
    pool_policy: Option<&CodexAccountPoolPolicy>,
) -> CodexMultiRouterAuthFacade {
    let Some(routing) = provider.settings_config.get("codexRouting") else {
        return CodexMultiRouterAuthFacade::LegacyPreserved;
    };
    if routing
        .get("enabled")
        .and_then(JsonValue::as_bool)
        .is_some_and(|enabled| !enabled)
    {
        return CodexMultiRouterAuthFacade::LegacyPreserved;
    }
    let Some(routes) = routing.get("routes").and_then(JsonValue::as_array) else {
        return CodexMultiRouterAuthFacade::LegacyPreserved;
    };

    let mut has_enabled_route = false;
    let mut needs_native_auth = false;
    let mut has_ambiguous_auth = false;

    for route in routes {
        if route
            .get("enabled")
            .and_then(JsonValue::as_bool)
            .is_some_and(|enabled| !enabled)
        {
            continue;
        }
        has_enabled_route = true;
        let source = route
            .pointer("/upstream/auth/source")
            .and_then(JsonValue::as_str);
        match source {
            Some("native_codex_auth") => needs_native_auth = true,
            Some("account_pool") => match pool_policy {
                Some(policy) => {
                    needs_native_auth |= policy.enabled
                        && policy.entries.iter().any(|entry| {
                            entry.enabled && entry.account_id == NATIVE_CODEX_ACCOUNT_ID
                        });
                }
                None => has_ambiguous_auth = true,
            },
            Some("provider_config" | "managed_account" | "managed_codex_oauth") => {}
            Some(_) | None => has_ambiguous_auth = true,
        }
    }

    if needs_native_auth {
        CodexMultiRouterAuthFacade::NativeMixed
    } else if !has_enabled_route || has_ambiguous_auth {
        CodexMultiRouterAuthFacade::LegacyPreserved
    } else {
        CodexMultiRouterAuthFacade::FullyManaged
    }
}

/// 判断 MultiRouter 是否需要让官方 V2 协作消息保持明文。
///
/// 官方 route 的子 Agent 能由 ChatGPT Codex backend 解密；任一启用的第三方或来源
/// 不明 route 都可能成为子 Agent，因此 official parent 的协作工具参数必须改为 V2
/// 明文。禁用 route 不影响当前任务的传输策略。
pub fn codex_multirouter_needs_plaintext_v2_collaboration(provider: &Provider) -> bool {
    let Some(routing) = provider.settings_config.get("codexRouting") else {
        return false;
    };
    if routing
        .get("enabled")
        .and_then(JsonValue::as_bool)
        .is_some_and(|enabled| !enabled)
    {
        return false;
    }
    let Some(routes) = routing
        .get("routes")
        .and_then(JsonValue::as_array)
        .or_else(|| routing.as_array())
    else {
        return false;
    };

    routes.iter().any(|route| {
        route
            .get("enabled")
            .and_then(JsonValue::as_bool)
            .unwrap_or(true)
            && !codex_route_uses_official_agent_backend(route)
    })
}

fn codex_route_uses_official_agent_backend(route: &JsonValue) -> bool {
    let upstream = route.get("upstream").unwrap_or(route);
    let auth = upstream
        .get("auth")
        .or_else(|| route.get("auth"))
        .unwrap_or(upstream);
    auth.get("source")
        .and_then(JsonValue::as_str)
        .is_some_and(|source| {
            matches!(
                source.to_ascii_lowercase().as_str(),
                "native_codex_auth" | "managed_codex_oauth" | "managed_account" | "account_pool"
            )
        })
        || auth
            .get("authProvider")
            .or_else(|| auth.get("auth_provider"))
            .and_then(JsonValue::as_str)
            .is_some_and(|provider| provider.eq_ignore_ascii_case("codex_oauth"))
}

/// 官方 Codex 客户端 User-Agent 正则
#[allow(dead_code)]
static CODEX_CLIENT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(codex_vscode|codex_cli_rs)/[\d.]+").unwrap());

/// Codex 适配器
pub struct CodexAdapter;

/// Codex `/responses` 请求在真实上游侧应使用的协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexResponsesUpstreamProtocol {
    /// 直接透传 OpenAI Responses API。
    Responses,
    /// 在本地把 Responses 转成 OpenAI Chat Completions。
    Chat,
    /// 在本地把 Responses 转成 OpenAI Messages。
    Messages,
    /// 在本地把 Responses 转成 Anthropic Messages。
    Anthropic,
}

impl CodexResponsesUpstreamProtocol {
    /// 输出前后端共用的协议枚举字符串，避免状态页和运行态口径分叉。
    pub fn api_format(self) -> &'static str {
        match self {
            Self::Responses => "openai_responses",
            Self::Chat => "openai_chat",
            Self::Messages => "openai_messages",
            Self::Anthropic => "anthropic",
        }
    }
}

/// 解释 Codex `/responses` 请求为何会命中某种上游协议。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexResponsesUpstreamDecision {
    pub protocol: CodexResponsesUpstreamProtocol,
    pub source: &'static str,
    pub detail: String,
}

impl CodexResponsesUpstreamDecision {
    /// 构造协议决策，统一收口状态页与运行态共享字段。
    fn new(
        protocol: CodexResponsesUpstreamProtocol,
        source: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            protocol,
            source,
            detail: detail.into(),
        }
    }
}

/// 解释当前 provider 的 `/responses` 请求在真实上游会走哪种协议。
///
/// 这是 Codex MultiRouter 关于协议选择的单一真理来源：
/// - forwarder 运行时通过它判断是否做 responses->chat/messages 转换；
/// - 诊断/状态页也通过它解释“为什么这么走”。
pub fn explain_codex_responses_upstream_protocol(
    provider: &Provider,
) -> CodexResponsesUpstreamDecision {
    if provider_is_managed_codex_oauth(provider) {
        return CodexResponsesUpstreamDecision::new(
            CodexResponsesUpstreamProtocol::Responses,
            "managed_codex_oauth",
            "托管 Codex OAuth 固定直连 chatgpt.com/backend-api/codex/responses",
        );
    }

    if let Some(api_format) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
    {
        return CodexResponsesUpstreamDecision::new(
            codex_upstream_protocol_from_api_format(api_format),
            "provider_meta_api_format",
            format!("meta.apiFormat={api_format}"),
        );
    }

    if let Some(api_format) = provider
        .settings_config
        .get("api_format")
        .and_then(|value| value.as_str())
    {
        return CodexResponsesUpstreamDecision::new(
            codex_upstream_protocol_from_api_format(api_format),
            "settings_api_format",
            format!("settings_config.api_format={api_format}"),
        );
    }

    if let Some(api_format) = provider
        .settings_config
        .get("apiFormat")
        .and_then(|value| value.as_str())
    {
        return CodexResponsesUpstreamDecision::new(
            codex_upstream_protocol_from_api_format(api_format),
            "settings_api_format",
            format!("settings_config.apiFormat={api_format}"),
        );
    }

    if let Some(base_url) = provider_codex_base_url(provider) {
        if is_known_chat_completions_only_url(&base_url) {
            return CodexResponsesUpstreamDecision::new(
                CodexResponsesUpstreamProtocol::Chat,
                "known_chat_completions_only_url",
                format!("base_url={base_url} 命中已知 Chat Completions-only 上游"),
            );
        }
    }

    if let Some(wire_api) = provider
        .settings_config
        .get("config")
        .and_then(|value| value.as_str())
        .and_then(extract_codex_wire_api_from_toml)
    {
        return CodexResponsesUpstreamDecision::new(
            codex_upstream_protocol_from_api_format(&wire_api),
            "config_wire_api",
            format!("config.toml wire_api={wire_api}"),
        );
    }

    CodexResponsesUpstreamDecision::new(
        CodexResponsesUpstreamProtocol::Responses,
        "default_responses",
        "未发现 chat/messages 信号，保持原生 Responses 透传",
    )
}

/// Whether this Codex provider's real upstream should be called through
/// OpenAI Chat Completions, even if the local Codex client is talking to CC
/// Switch through the Responses API.
pub fn codex_provider_uses_chat_completions(provider: &Provider) -> bool {
    matches!(
        explain_codex_responses_upstream_protocol(provider).protocol,
        CodexResponsesUpstreamProtocol::Chat
    )
}

pub fn should_convert_codex_responses_to_chat(provider: &Provider, endpoint: &str) -> bool {
    is_codex_responses_endpoint(endpoint)
        && matches!(
            explain_codex_responses_upstream_protocol(provider).protocol,
            CodexResponsesUpstreamProtocol::Chat
        )
}

/// Whether this effective provider can return a native Responses compaction item.
///
/// Only the official ChatGPT backend is known to support the private compaction
/// payload. Third-party routes may opt in through the route's
/// `capabilities.supportsRemoteCompaction` field; absent that declaration they
/// need CCSwitchMulti's compaction response adapter.
pub fn codex_route_supports_responses_compaction(provider: &Provider) -> bool {
    if is_codex_official_provider(provider) {
        return true;
    }

    if provider
        .settings_config
        .get("codexResolvedRouteId")
        .and_then(JsonValue::as_str)
        == Some("router-codex-official")
    {
        return true;
    }

    let Some(capabilities) = provider.settings_config.get("codexResolvedCapabilities") else {
        return false;
    };
    for key in ["supportsRemoteCompaction", "supports_remote_compaction"] {
        if let Some(value) = capabilities.get(key).and_then(JsonValue::as_bool) {
            return value;
        }
    }
    false
}

/// Whether this provider should expose the OpenAI provider name to Codex.
///
/// Codex decides remote vs local compaction from the model provider `name`
/// (`is_openai()`), not from CCSM route capabilities. Third-party providers must
/// therefore stay on a non-OpenAI name by default. Official OAuth-only
/// MultiRouter buckets keep remote compaction, while any third-party or mixed
/// bucket falls back to local unless the user explicitly opts in. The legacy UI
/// toggle records `name = "OpenAI"` in the provider's Codex TOML, so we honor
/// that marker and a structured provider setting too.
pub(crate) fn codex_provider_remote_compaction_enabled(provider: &Provider) -> bool {
    for key in [
        "codexRemoteCompaction",
        "remoteCompaction",
        "remote_compaction",
    ] {
        if provider
            .settings_config
            .get(key)
            .and_then(JsonValue::as_bool)
            == Some(true)
        {
            return true;
        }
    }

    if let Some(config_text) = provider
        .settings_config
        .get("config")
        .and_then(JsonValue::as_str)
    {
        if let Ok(doc) = config_text.parse::<TomlValue>() {
            if let Some(provider_id) = doc.get("model_provider").and_then(TomlValue::as_str) {
                if doc
                    .get("model_providers")
                    .and_then(|providers| providers.get(provider_id))
                    .and_then(|entry| entry.get("name"))
                    .and_then(TomlValue::as_str)
                    .is_some_and(|name| name.trim() == "OpenAI")
                {
                    return true;
                }
            }
        }
    }

    // Keep remote compaction for official OAuth-only MultiRouter buckets by
    // default, but never for third-party or mixed buckets. A route capability
    // can explicitly claim native compaction support without OAuth.
    let Some(routing) = provider.settings_config.get("codexRouting") else {
        return false;
    };
    if routing
        .get("enabled")
        .and_then(JsonValue::as_bool)
        .is_some_and(|enabled| !enabled)
    {
        return false;
    }
    let Some(routes) = routing
        .get("routes")
        .and_then(JsonValue::as_array)
        .or_else(|| routing.as_array())
    else {
        return false;
    };
    let enabled_routes = routes
        .iter()
        .filter(|route| {
            route
                .get("enabled")
                .and_then(JsonValue::as_bool)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    if enabled_routes.is_empty() {
        return false;
    }
    enabled_routes.iter().all(|route| {
        if let Some(supports) = route
            .pointer("/capabilities/supportsRemoteCompaction")
            .or_else(|| route.pointer("/capabilities/supports_remote_compaction"))
            .and_then(JsonValue::as_bool)
        {
            return supports;
        }
        codex_route_uses_official_agent_backend(route)
    })
}

pub fn should_convert_codex_responses_to_messages(provider: &Provider, endpoint: &str) -> bool {
    is_codex_responses_endpoint(endpoint)
        && matches!(
            explain_codex_responses_upstream_protocol(provider).protocol,
            CodexResponsesUpstreamProtocol::Messages
        )
}

/// 根据 Codex 请求体里的 `model` 字段，把复合 provider 解析成本次真实上游 provider。
///
/// 新 schema 使用 `settings_config.codexRouting`；旧的 `codexModelRoutes` / `modelRoutes`
/// 仍然只读兼容，便于本地旧配置在 UI 保存前继续可用。函数不访问数据库，也不改变当前
/// CC Switch provider，避免聊天窗口切模型时反向触发 GUI 当前供应商切换。
pub fn resolve_codex_model_routed_provider(
    provider: &Provider,
    body: &JsonValue,
) -> Option<Provider> {
    resolve_codex_model_routed_providers(provider, body)
        .into_iter()
        .next()
}

/// 解析 Codex router 的唯一命中 route。
///
/// MultiRouter 是按模型分流，不是跨模型的故障转移池：一个请求只可发送给它实际命中的
/// route。特别是 `gpt-*` 官方模型发生上游错误时，绝不能回退到 DeepSeek/Qwen 等路由，
/// 否则会改变用户选择的模型与认证边界。
pub fn resolve_codex_model_routed_providers(
    provider: &Provider,
    body: &JsonValue,
) -> Vec<Provider> {
    let request_model = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty());
    let Some(request_model) = request_model else {
        return Vec::new();
    };

    resolve_codex_route_candidates(provider, request_model)
        .into_iter()
        .next()
        .map(|route| vec![build_codex_routed_provider(provider, route, request_model)])
        .unwrap_or_default()
}

/// 返回 routed Codex provider 对应的真实持久 provider 身份。
pub fn codex_route_persistent_provider(provider: &Provider) -> (&str, &str) {
    let id = provider
        .settings_config
        .get(CODEX_ROUTER_PARENT_PROVIDER_ID)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(provider.id.as_str());
    let name = provider
        .settings_config
        .get(CODEX_ROUTER_PARENT_PROVIDER_NAME)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(provider.name.as_str());
    (id, name)
}

/// 返回 routed Codex provider 引用的真实目标 provider id。
pub fn codex_route_target_provider_id(provider: &Provider) -> Option<&str> {
    provider
        .settings_config
        .get(CODEX_RESOLVED_TARGET_PROVIDER_ID)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// 用 route 命中的真实目标 provider 作为底座，生成本次请求的 effective provider。
///
/// route 引用已有供应商时，base_url、认证、apiFormat、reasoning 等转换配置都应该跟随
/// 该供应商；route 只叠加 request-local 的路由身份、匹配状态、能力声明和显式模型映射。
pub fn materialize_codex_routed_provider_from_target(
    route_provider: &Provider,
    target_provider: &Provider,
) -> Provider {
    let mut materialized = target_provider.clone();
    materialized.id = route_provider.id.clone();
    materialized.name = route_provider.name.clone();

    let mut settings = target_provider
        .settings_config
        .as_object()
        .cloned()
        .unwrap_or_else(Map::new);
    let route_settings = route_provider.settings_config.as_object();

    for key in [
        "codexResolvedRouteId",
        "codexResolvedRouteMatched",
        "codexResolvedCapabilities",
        CODEX_ROUTER_PARENT_PROVIDER_ID,
        CODEX_ROUTER_PARENT_PROVIDER_NAME,
        CODEX_RESOLVED_TARGET_PROVIDER_ID,
        CODEX_ACCOUNT_POOL_ENABLED,
        "apiFormat",
        "api_format",
    ] {
        if let Some(value) = route_settings
            .and_then(|settings| settings.get(key))
            .cloned()
        {
            settings.insert(key.to_string(), value);
        }
    }

    // 保留 route provider 的 modelCatalog，使 apply_codex_request_upstream_model
    // 能通过 catalog 把可见模型名映射回真实上游模型名。MultiRouter 的 modelCatalog
    // 只存在于 parent plan 中，target provider 通常不携带。
    if let Some(catalog) = route_settings
        .and_then(|settings| settings.get("modelCatalog"))
        .cloned()
    {
        settings.insert("modelCatalog".to_string(), catalog);
    }

    if let Some(model_override) = route_settings
        .and_then(|settings| settings.get(CODEX_RESOLVED_UPSTREAM_MODEL_OVERRIDE))
        .cloned()
    {
        settings.insert("model".to_string(), model_override.clone());
        settings.insert(
            CODEX_RESOLVED_UPSTREAM_MODEL_OVERRIDE.to_string(),
            model_override,
        );
    }

    let native_codex_auth = route_provider
        .settings_config
        .get(CODEX_NATIVE_AUTH_PASSTHROUGH)
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    let account_pool = route_provider
        .settings_config
        .get(CODEX_ACCOUNT_POOL_ENABLED)
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    let managed_codex_oauth = !native_codex_auth
        && !account_pool
        && should_treat_target_as_managed_codex_oauth(
            route_provider,
            target_provider,
            &materialized,
        );
    if managed_codex_oauth || native_codex_auth || account_pool {
        sanitize_materialized_managed_codex_oauth_settings(&mut settings);
    }
    if account_pool {
        settings.remove("auth");
        settings.remove("apiKey");
        settings.remove("api_key");
    }
    if native_codex_auth {
        settings.insert(
            CODEX_NATIVE_AUTH_PASSTHROUGH.to_string(),
            JsonValue::Bool(true),
        );
        settings.remove("auth");
    }

    materialized.settings_config = JsonValue::Object(settings);
    if account_pool {
        let meta = materialized.meta.get_or_insert_with(ProviderMeta::default);
        meta.provider_type = None;
        meta.api_format = Some("openai_responses".to_string());
        meta.auth_binding = None;
    } else if managed_codex_oauth {
        let meta = materialized.meta.get_or_insert_with(ProviderMeta::default);
        meta.provider_type = Some("codex_oauth".to_string());
    } else if native_codex_auth {
        let meta = materialized.meta.get_or_insert_with(ProviderMeta::default);
        meta.provider_type = None;
        meta.api_format = Some("openai_responses".to_string());
    } else if let Some(api_format) = route_provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
    {
        // route 是本次请求的显式协议来源，必须覆盖目标 provider 的陈旧元数据。
        let meta = materialized.meta.get_or_insert_with(ProviderMeta::default);
        meta.api_format = Some(api_format.to_string());
    }
    materialized
}

/// 为诊断/状态页构造某条 route 的真实 effective provider。
///
/// 这里复用运行态的 route 构造和 materialize 逻辑，让“配置判定”和真实转发链路
/// 保持同一口径，避免状态页自己猜协议。
pub fn build_codex_route_probe_provider(
    provider: &Provider,
    route: &JsonValue,
    target_provider: Option<&Provider>,
) -> Provider {
    let request_model = first_codex_route_model(route).unwrap_or("route-probe");
    let routed = build_codex_routed_provider(provider, route, request_model);
    if let Some(target_provider) = target_provider {
        materialize_codex_routed_provider_from_target(&routed, target_provider)
    } else {
        routed
    }
}

/// 判断 route 引用的旧版官方 Codex provider 是否实际应走托管 ChatGPT OAuth。
///
/// 早期 `codex-official` 只保存 `auth.auth_mode = "chatgpt"` 和 OAuth tokens，
/// 没有写 `meta.provider_type = "codex_oauth"`；异常恢复后还可能残留第三方
/// `base_url` / API key。MultiRouter 通过 `targetProviderId` 命中官方身份时必须
/// 先按 managed OAuth 物化，避免污染字段把官方 route 拉到第三方中转。只有没有官方
/// 身份证据的 provider 才用真实非本地 `base_url` 阻止 OAuth 兜底。
fn should_treat_target_as_managed_codex_oauth(
    route_provider: &Provider,
    target_provider: &Provider,
    materialized: &Provider,
) -> bool {
    if materialized
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("codex_oauth")
    {
        return true;
    }

    let route_target = codex_route_target_provider_id(route_provider).unwrap_or_default();
    if target_provider_looks_like_managed_codex_oauth(target_provider, route_target) {
        return true;
    }

    if provider_id_or_name_marks_official(target_provider, route_target)
        && provider_has_managed_codex_oauth_auth(route_provider)
    {
        return true;
    }

    if provider_has_non_proxy_codex_base_url(target_provider)
        || provider_has_non_proxy_codex_base_url(materialized)
    {
        return false;
    }

    false
}

/// 移除官方 OAuth 物化 provider 上可能来自旧 DB/接管备份的普通 API 字段。
///
/// 这些字段保留在持久 provider 里不会被改写；这里只清理 request-local effective
/// provider，避免后续诊断或兼容逻辑再次把 `codex-official` 当成第三方中转。
fn sanitize_materialized_managed_codex_oauth_settings(settings: &mut Map<String, JsonValue>) {
    for key in ["base_url", "baseURL", "baseUrl", "apiKey", "api_key"] {
        settings.remove(key);
    }
}

/// 检查 provider 是否有非本地接管代理的真实上游地址。
///
/// official provider 在切换/恢复异常后可能被污染成 `127.0.0.1:15721`；
/// 这种地址不能阻止托管 OAuth 兜底，否则 OpenAI route 会递归打回本地代理。
fn provider_has_non_proxy_codex_base_url(provider: &Provider) -> bool {
    provider_codex_base_url(provider)
        .as_deref()
        .is_some_and(|url| !codex_base_url_points_to_local_proxy(url))
}

fn provider_codex_base_url(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .or_else(|| provider.settings_config.get("baseUrl"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|value| value.as_str())
                .and_then(extract_codex_base_url_from_toml)
        })
}

fn codex_base_url_points_to_local_proxy(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.contains("://127.0.0.1:15721")
        || lower.contains("://localhost:15721")
        || lower.contains("://[::1]:15721")
}

/// 识别旧版官方 Codex OAuth provider，不依赖新版 meta 字段是否已回填。
fn target_provider_looks_like_managed_codex_oauth(
    provider: &Provider,
    route_target_provider_id: &str,
) -> bool {
    if provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("codex_oauth")
    {
        return true;
    }

    if provider_is_empty_codex_official_seed(provider, route_target_provider_id) {
        return true;
    }

    provider_has_managed_codex_oauth_auth(provider)
        && provider_id_or_name_marks_official(provider, route_target_provider_id)
}

/// 判断 provider 是否是旧版空 official seed。
///
/// 旧版备份 provider 可能只作为“使用 CCSwitchMulti 托管 OAuth”的占位记录存在，
/// 真实 refresh/access token 保存在 `CodexOAuthManager`。内置 `codex-official`
/// 由 [`provider_uses_native_codex_auth`] 优先识别为 Desktop 当前登录态；该兼容
/// 推断只在其余调用链中处理旧备份或显式路由物化。
fn provider_is_empty_codex_official_seed(
    provider: &Provider,
    route_target_provider_id: &str,
) -> bool {
    provider.category.as_deref() == Some("official")
        && provider_id_or_name_marks_official(provider, route_target_provider_id)
}

fn provider_has_managed_codex_oauth_auth(provider: &Provider) -> bool {
    let auth = provider.settings_config.get("auth");
    let has_chatgpt_auth_mode = auth
        .and_then(|auth| auth.get("auth_mode"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("chatgpt"));
    let has_oauth_tokens = auth
        .and_then(|auth| auth.get("tokens"))
        .and_then(|tokens| {
            tokens
                .get("access_token")
                .or_else(|| tokens.get("refresh_token"))
                .and_then(|value| value.as_str())
        })
        .map(str::trim)
        .is_some_and(|token| !token.is_empty());

    has_chatgpt_auth_mode || has_oauth_tokens
}

fn provider_id_or_name_marks_official(provider: &Provider, route_target_provider_id: &str) -> bool {
    let id_or_name_marks_official = [
        provider.id.as_str(),
        provider.name.as_str(),
        route_target_provider_id,
    ]
    .into_iter()
    .map(str::to_ascii_lowercase)
    .any(|value| value.contains("codex-official") || value.contains("openai official"));

    id_or_name_marks_official
}

/// 从新旧配置中挑出本次请求的 route 候选；匹配 route 在前，fallback route 在后。
fn resolve_codex_route_candidates<'a>(
    provider: &'a Provider,
    request_model: &str,
) -> Vec<&'a JsonValue> {
    if let Some(routing) = provider.settings_config.get("codexRouting") {
        if let Some(routes) = routing.as_array() {
            return resolve_codex_legacy_route_candidates(routes, request_model);
        }

        if routing
            .get("enabled")
            .and_then(|value| value.as_bool())
            .is_some_and(|enabled| !enabled)
        {
            return Vec::new();
        }

        let Some(routes) = routing.get("routes").and_then(|value| value.as_array()) else {
            return Vec::new();
        };

        let mut selected = Vec::new();
        if let Some(route) = find_codex_route_by_match_priority(routes, request_model) {
            selected.push(route);
        } else if codex_model_is_declared_only_by_disabled_route(routes, request_model) {
            return Vec::new();
        } else if let Some(default_route) = routing
            .get("defaultRouteId")
            .or_else(|| routing.get("default_route_id"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .and_then(|default_route_id| {
                routes.iter().find(|route| {
                    codex_route_is_enabled(route)
                        && route
                            .get("id")
                            .and_then(|value| value.as_str())
                            .is_some_and(|id| id.eq_ignore_ascii_case(default_route_id))
                })
            })
        {
            selected.push(default_route);
        }

        let primary_id = selected
            .first()
            .and_then(|route| route.get("id"))
            .and_then(|value| value.as_str())
            .map(|id| id.to_ascii_lowercase());
        selected.extend(routes.iter().filter(|route| {
            if !codex_route_is_enabled(route) {
                return false;
            }
            let route_id = route
                .get("id")
                .and_then(|value| value.as_str())
                .map(|id| id.to_ascii_lowercase());
            route_id != primary_id
        }));

        return selected;
    }

    resolve_codex_route(provider, request_model)
        .into_iter()
        .collect()
}

/// 判断当前 effective Codex provider 是否声明为 text-only 输入。
///
/// 该信息由 route resolver 写入 `codexResolvedCapabilities`，供 Responses -> Chat 转换
/// 在生成 OpenAI Chat `messages` 时决定是否把图片块降级成文本占位。
pub fn codex_provider_text_only_input(provider: &Provider) -> Option<bool> {
    let capabilities = provider.settings_config.get("codexResolvedCapabilities")?;
    if let Some(text_only) = capabilities
        .get("textOnly")
        .or_else(|| capabilities.get("text_only"))
        .and_then(|value| value.as_bool())
    {
        return Some(text_only);
    }

    capabilities
        .get("inputModalities")
        .or_else(|| capabilities.get("input_modalities"))
        .and_then(|value| value.as_array())
        .map(|modalities| {
            !modalities
                .iter()
                .filter_map(|value| value.as_str())
                .any(|modality| modality.eq_ignore_ascii_case("image"))
        })
}

/// 从新旧配置中挑出本次请求应该使用的 route。
///
/// 新配置允许显式关闭路由，并支持 `defaultRouteId` 兜底；旧配置没有开关语义，只要数组
/// 存在就按旧规则匹配，保证已有本地数据库不会在升级后突然失效。
fn resolve_codex_route<'a>(provider: &'a Provider, request_model: &str) -> Option<&'a JsonValue> {
    if let Some(routing) = provider.settings_config.get("codexRouting") {
        if let Some(routes) = routing.as_array() {
            return find_codex_route_by_match_priority(routes, request_model);
        }

        if routing
            .get("enabled")
            .and_then(|value| value.as_bool())
            .is_some_and(|enabled| !enabled)
        {
            return None;
        }

        let routes = routing.get("routes").and_then(|value| value.as_array())?;
        if let Some(route) = find_codex_route_by_match_priority(routes, request_model) {
            return Some(route);
        }
        if codex_model_is_declared_only_by_disabled_route(routes, request_model) {
            return None;
        }

        return routing
            .get("defaultRouteId")
            .or_else(|| routing.get("default_route_id"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .and_then(|default_route_id| {
                routes.iter().find(|route| {
                    codex_route_is_enabled(route)
                        && route
                            .get("id")
                            .and_then(|value| value.as_str())
                            .is_some_and(|id| id.eq_ignore_ascii_case(default_route_id))
                })
            });
    }

    provider
        .settings_config
        .get("codexModelRoutes")
        .or_else(|| provider.settings_config.get("modelRoutes"))
        .and_then(|value| value.as_array())
        .and_then(|routes| {
            routes
                .iter()
                .find(|route| codex_route_has_exact_model_match(route, request_model))
                .or_else(|| {
                    routes
                        .iter()
                        .find(|route| codex_route_has_prefix_model_match(route, request_model))
                })
        })
}

/// 判断 route 是否启用；字段缺省时按启用处理，减少手写配置的必填项。
fn codex_route_is_enabled(route: &JsonValue) -> bool {
    route
        .get("enabled")
        .and_then(|value| value.as_bool())
        .unwrap_or(true)
}

/// 兼容被旧版/损坏保存路径写成 `codexRouting: []` 的 MultiRouter 配置。
///
/// 这类数据没有 `enabled/defaultRouteId` 外壳，但数组里的 route 本身仍然完整；
/// 请求链路必须直接消费它，否则升级后所有模型都会 `route_missed=true`。
fn resolve_codex_legacy_route_candidates<'a>(
    routes: &'a [JsonValue],
    request_model: &str,
) -> Vec<&'a JsonValue> {
    let mut selected = Vec::new();
    if let Some(route) = find_codex_route_by_match_priority(routes, request_model) {
        selected.push(route);
    } else if codex_model_is_declared_only_by_disabled_route(routes, request_model) {
        return Vec::new();
    }

    let primary_id = selected
        .first()
        .and_then(|route| route.get("id"))
        .and_then(|value| value.as_str())
        .map(|id| id.to_ascii_lowercase());
    selected.extend(routes.iter().filter(|route| {
        if !codex_route_is_enabled(route) {
            return false;
        }
        let route_id = route
            .get("id")
            .and_then(|value| value.as_str())
            .map(|id| id.to_ascii_lowercase());
        route_id != primary_id
    }));
    selected
}

/// 判断请求模型是否只存在于已停用 route 的精确模型列表中。
///
/// Codex Desktop 可能暂存旧 catalog；此时旧 alias 不能被当成未知模型再落到
/// defaultRouteId，否则第三方模型切换会静默回到已耗尽的官方 route。
fn codex_model_is_declared_only_by_disabled_route(
    routes: &[JsonValue],
    request_model: &str,
) -> bool {
    routes.iter().any(|route| {
        !codex_route_is_enabled(route) && codex_route_has_exact_model_match(route, request_model)
    })
}

/// 按全局优先级查找 route：所有精确模型匹配优先于任何前缀匹配。
///
/// 这避免官方 `gpt-` 前缀 route 排在前面时，抢走后面聚合平台显式声明的
/// `gpt-5.5-pro`、`gpt-5.5-relay` 等精确模型。
fn find_codex_route_by_match_priority<'a>(
    routes: &'a [JsonValue],
    request_model: &str,
) -> Option<&'a JsonValue> {
    let exact_matches = routes
        .iter()
        .filter(|route| {
            codex_route_is_enabled(route) && codex_route_has_exact_model_match(route, request_model)
        })
        .collect::<Vec<_>>();
    if exact_matches.len() > 1 {
        let route_ids = exact_matches
            .iter()
            .filter_map(|route| route.get("id").and_then(|value| value.as_str()))
            .collect::<Vec<_>>();
        log::warn!(
            "[Codex MultiRouter] ambiguous exact route match for model `{}`; route_ids={:?}; using the first enabled route by order. Save or refresh the plan to generate unique visible model aliases.",
            request_model,
            route_ids
        );
    }
    exact_matches.into_iter().next().or_else(|| {
        routes.iter().find(|route| {
            codex_route_is_enabled(route)
                && codex_route_has_prefix_model_match(route, request_model)
        })
    })
}

/// 判断单条 Codex route 是否匹配请求模型。
///
/// 新 schema 使用 `match.models` / `match.prefixes`；旧 schema 使用顶层 `models` /
/// `modelPrefixes`。两套字段都按大小写不敏感处理，避免 UI 显示大小写差异导致误路由。
pub(crate) fn codex_route_matches_model(route: &JsonValue, request_model: &str) -> bool {
    codex_route_has_exact_model_match(route, request_model)
        || codex_route_has_prefix_model_match(route, request_model)
}

/// 判断 route 是否精确声明了请求模型。
fn codex_route_has_exact_model_match(route: &JsonValue, request_model: &str) -> bool {
    let match_config = route.get("match").unwrap_or(route);

    match_config
        .get("models")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| model.as_str())
        .any(|model| model.trim().eq_ignore_ascii_case(request_model))
}

/// 判断 route 是否通过模型前缀匹配请求模型。
fn codex_route_has_prefix_model_match(route: &JsonValue, request_model: &str) -> bool {
    let request_model_lower = request_model.to_ascii_lowercase();

    let match_config = route.get("match").unwrap_or(route);

    match_config
        .get("prefixes")
        .or_else(|| match_config.get("modelPrefixes"))
        .or_else(|| match_config.get("model_prefixes"))
        .or_else(|| route.get("modelPrefixes"))
        .or_else(|| route.get("model_prefixes"))
        .and_then(|prefixes| prefixes.as_array())
        .into_iter()
        .flatten()
        .filter_map(|prefix| prefix.as_str())
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty())
        .any(|prefix| request_model_lower.starts_with(&prefix.to_ascii_lowercase()))
}

/// 从 route 配置构造本次请求实际使用的 provider。
///
/// 保留原 provider 的 `modelCatalog` 等 UI 元数据，只覆盖上游连接必需字段。这样 Chat
/// 转换时仍能识别下拉框中的模型，避免把 `deepseek-v4-flash` 覆盖回 provider 默认模型。
fn build_codex_routed_provider(
    provider: &Provider,
    route: &JsonValue,
    request_model: &str,
) -> Provider {
    let mut routed = provider.clone();
    let upstream = route.get("upstream").unwrap_or(route);

    let route_id = route
        .get("id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(request_model);
    routed.id = format!("{}::route::{}", provider.id, route_id);

    if let Some(name) = route
        .get("label")
        .or_else(|| route.get("name"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        routed.name = name.to_string();
    }

    let mut settings = provider
        .settings_config
        .as_object()
        .cloned()
        .unwrap_or_else(Map::new);

    if let Some(base_url) = upstream
        .get("baseUrl")
        .or_else(|| upstream.get("base_url"))
        .or_else(|| route.get("baseUrl"))
        .or_else(|| route.get("baseURL"))
        .or_else(|| route.get("base_url"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        settings.insert(
            "base_url".to_string(),
            JsonValue::String(base_url.to_string()),
        );
    }

    let explicit_upstream_model = explicit_codex_route_model_override(route, request_model);
    let upstream_model = explicit_upstream_model
        .or_else(|| first_codex_route_model(route))
        .unwrap_or(request_model);
    settings.insert(
        "model".to_string(),
        JsonValue::String(upstream_model.to_string()),
    );
    if explicit_upstream_model.is_some() {
        settings.insert(
            CODEX_RESOLVED_UPSTREAM_MODEL_OVERRIDE.to_string(),
            JsonValue::String(upstream_model.to_string()),
        );
    }

    if codex_route_uses_managed_codex_oauth(upstream, route)
        || codex_route_uses_account_pool(upstream, route)
    {
        // 托管 Codex OAuth route 不能继承外层 provider 的 Bearer key，否则会覆盖 managed account 注入链路。
        settings.remove("auth");
        settings.remove("apiKey");
        settings.remove("api_key");
    }
    apply_codex_route_auth(upstream, route, &mut settings);

    if let Some(wire_api) = codex_route_api_format(upstream, route) {
        settings.insert(
            "apiFormat".to_string(),
            JsonValue::String(wire_api.to_string()),
        );
    }
    if let Some(capabilities) = route.get("capabilities").cloned() {
        settings.insert("codexResolvedCapabilities".to_string(), capabilities);
    }
    settings.insert(
        "codexResolvedRouteId".to_string(),
        JsonValue::String(route_id.to_string()),
    );
    if let Some(target_provider_id) = codex_route_target_provider_id_from_route(route) {
        settings.insert(
            CODEX_RESOLVED_TARGET_PROVIDER_ID.to_string(),
            JsonValue::String(target_provider_id.to_string()),
        );
    }
    settings.insert(
        CODEX_ROUTER_PARENT_PROVIDER_ID.to_string(),
        JsonValue::String(provider.id.clone()),
    );
    settings.insert(
        CODEX_ROUTER_PARENT_PROVIDER_NAME.to_string(),
        JsonValue::String(provider.name.clone()),
    );
    settings.insert(
        "codexResolvedRouteMatched".to_string(),
        JsonValue::Bool(codex_route_matches_model(route, request_model)),
    );

    routed.settings_config = JsonValue::Object(settings);

    let mut meta = routed.meta.clone().unwrap_or_default();
    if let Some(wire_api) = codex_route_api_format(upstream, route) {
        meta.api_format = Some(wire_api.to_string());
    }
    if codex_route_uses_managed_codex_oauth(upstream, route) {
        meta.provider_type = Some("codex_oauth".to_string());
    } else if codex_route_uses_account_pool(upstream, route) {
        meta.provider_type = None;
        meta.api_format = Some("openai_responses".to_string());
        meta.auth_binding = None;
    } else if let Some(provider_type) = upstream
        .get("providerType")
        .or_else(|| upstream.get("provider_type"))
        .or_else(|| route.get("providerType"))
        .or_else(|| route.get("provider_type"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|provider_type| !provider_type.is_empty())
    {
        meta.provider_type = Some(provider_type.to_string());
    }
    if let Some(auth_binding) = codex_route_auth_binding(upstream, route) {
        meta.auth_binding = Some(auth_binding);
    } else if let Some(auth_binding) = upstream
        .get("authBinding")
        .or_else(|| upstream.get("auth_binding"))
        .or_else(|| route.get("authBinding"))
    {
        if let Ok(binding) = serde_json::from_value(auth_binding.clone()) {
            meta.auth_binding = Some(binding);
        }
    }
    if let Some(reasoning_config) = codex_route_chat_reasoning_config(upstream, route) {
        meta.codex_chat_reasoning = Some(reasoning_config);
    }
    if let Some(cache_config) = codex_route_cache_config(upstream, route) {
        meta.codex_cache = Some(cache_config);
    }
    routed.meta = Some(meta);

    routed
}

/// 从 route 中读取显式声明的目标 provider id。
fn codex_route_target_provider_id_from_route(route: &JsonValue) -> Option<&str> {
    let upstream = route.get("upstream").unwrap_or(route);
    [
        upstream.get("targetProviderId"),
        upstream.get("target_provider_id"),
        upstream.get("providerId"),
        upstream.get("provider_id"),
        upstream.get("upstreamProviderId"),
        upstream.get("upstream_provider_id"),
        upstream.get("provider"),
        route.get("targetProviderId"),
        route.get("target_provider_id"),
        route.get("providerId"),
        route.get("provider_id"),
        route.get("upstreamProviderId"),
        route.get("upstream_provider_id"),
        route.get("provider"),
    ]
    .into_iter()
    .flatten()
    .filter_map(|value| value.as_str())
    .map(str::trim)
    .find(|value| !value.is_empty())
}

/// 从 route 中读取显式模型覆盖；没有覆盖时应交给目标 provider 自己的 model 配置。
fn explicit_codex_route_model_override<'a>(
    route: &'a JsonValue,
    request_model: &str,
) -> Option<&'a str> {
    let upstream = route.get("upstream").unwrap_or(route);
    upstream
        .get("modelMap")
        .or_else(|| upstream.get("model_map"))
        .and_then(|value| value.as_object())
        .and_then(|map| map.get(request_model))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .or_else(|| {
            upstream
                .get("upstreamModel")
                .or_else(|| upstream.get("upstream_model"))
                .or_else(|| upstream.get("model"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|model| !model.is_empty())
        })
        .or_else(|| {
            route
                .get("upstreamModel")
                .or_else(|| route.get("upstream_model"))
                .or_else(|| route.get("model"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|model| !model.is_empty())
        })
}

/// 读取 route 自己声明的第一个模型，用于跨模型 fallback 时的默认上游模型。
fn first_codex_route_model(route: &JsonValue) -> Option<&str> {
    let match_config = route.get("match").unwrap_or(route);
    match_config
        .get("models")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| model.as_str())
        .map(str::trim)
        .find(|model| !model.is_empty())
}

/// 解析 route 的上游 API 格式，并归一化到 provider meta 使用的枚举字符串。
fn codex_route_api_format<'a>(upstream: &'a JsonValue, route: &'a JsonValue) -> Option<&'a str> {
    upstream
        .get("wire_api")
        .or_else(|| upstream.get("wireApi"))
        .or_else(|| upstream.get("apiFormat"))
        .or_else(|| route.get("wire_api"))
        .or_else(|| route.get("wireApi"))
        .or_else(|| route.get("apiFormat"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|wire_api| !wire_api.is_empty())
        .map(|wire_api| match wire_api {
            "responses" => "openai_responses",
            "chat" => "openai_chat",
            "messages" => "openai_messages",
            other => other,
        })
}

/// 根据 route 的 auth source 写入 effective provider 认证信息。
///
/// `provider_config` 支持 route 自带 API key；`managed_account` / `managed_codex_oauth`
/// 只设置 meta，让现有 Codex OAuth adapter 继续负责 token 注入。
fn apply_codex_route_auth(
    upstream: &JsonValue,
    route: &JsonValue,
    settings: &mut Map<String, JsonValue>,
) {
    let auth_source = upstream
        .get("auth")
        .or_else(|| route.get("auth"))
        .and_then(|auth| auth.get("source"))
        .and_then(|value| value.as_str())
        .map(str::trim);

    if auth_source == Some("native_codex_auth") {
        settings.remove("auth");
        settings.remove("apiKey");
        settings.remove("api_key");
        settings.insert(
            CODEX_NATIVE_AUTH_PASSTHROUGH.to_string(),
            JsonValue::Bool(true),
        );
        return;
    }

    if auth_source == Some("account_pool") {
        settings.remove("auth");
        settings.remove("apiKey");
        settings.remove("api_key");
        settings.insert(
            CODEX_ACCOUNT_POOL_ENABLED.to_string(),
            JsonValue::Bool(true),
        );
        settings.insert(
            CODEX_NATIVE_AUTH_PASSTHROUGH.to_string(),
            JsonValue::Bool(false),
        );
        return;
    }

    if let Some(auth) = upstream.get("auth").or_else(|| route.get("auth")) {
        let mut should_insert_auth = true;
        if let Some(source) = auth_source {
            if matches!(source, "managed_account" | "managed_codex_oauth") {
                return;
            }
            if source == "provider_config" {
                let has_inline_key = auth
                    .get("OPENAI_API_KEY")
                    .or_else(|| auth.get("apiKey"))
                    .or_else(|| auth.get("api_key"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .is_some_and(|key| !key.is_empty());
                if !has_inline_key {
                    // provider_config 是 route 对现有 provider 鉴权的引用声明；没有内联 key 时不能覆盖原 auth。
                    should_insert_auth = false;
                }
            }
        }
        if should_insert_auth {
            settings.insert("auth".to_string(), auth.clone());
        }
    }
    if let Some(env) = upstream.get("env").or_else(|| route.get("env")).cloned() {
        settings.insert("env".to_string(), env);
    }
    if auth_source.is_some_and(|source| source != "provider_config") {
        // 托管账号 route 的鉴权必须由 meta/auth_binding 注入；忽略残留 apiKey，避免 UI 切换 auth source 后误走 Bearer。
        return;
    }
    if let Some(api_key) = upstream
        .get("apiKey")
        .or_else(|| upstream.get("api_key"))
        .or_else(|| route.get("apiKey"))
        .or_else(|| route.get("api_key"))
        .cloned()
    {
        if api_key
            .as_str()
            .map(str::trim)
            .is_some_and(|key| !key.is_empty())
        {
            let mut auth = Map::new();
            auth.insert(
                "OPENAI_API_KEY".to_string(),
                JsonValue::String(api_key.as_str().unwrap_or_default().to_string()),
            );
            settings.insert("auth".to_string(), JsonValue::Object(auth));
        }
        settings.insert("apiKey".to_string(), api_key);
    }
}

/// 判断 route 是否声明使用 CC Switch 托管的 Codex OAuth 账号。
fn codex_route_uses_managed_codex_oauth(upstream: &JsonValue, route: &JsonValue) -> bool {
    upstream
        .get("auth")
        .or_else(|| route.get("auth"))
        .and_then(|auth| auth.get("source"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .is_some_and(|source| matches!(source, "managed_account" | "managed_codex_oauth"))
}

fn codex_route_uses_account_pool(upstream: &JsonValue, route: &JsonValue) -> bool {
    upstream
        .get("auth")
        .or_else(|| route.get("auth"))
        .and_then(|auth| auth.get("source"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        == Some("account_pool")
}

/// 把 route 内联 auth 声明转换成 ProviderMeta 的托管账号绑定。
///
/// `managed_account` 使用标准 `AuthBinding` 字段；`managed_codex_oauth` 是 UI 友好的简写，
/// 自动归一化为 `authProvider = "codex_oauth"`。
fn codex_route_auth_binding(upstream: &JsonValue, route: &JsonValue) -> Option<AuthBinding> {
    let auth = upstream.get("auth").or_else(|| route.get("auth"))?;
    let source = auth
        .get("source")
        .and_then(|value| value.as_str())
        .map(str::trim)?;

    if source == "managed_account" {
        return serde_json::from_value(auth.clone()).ok();
    }

    if source == "managed_codex_oauth" {
        return Some(AuthBinding {
            source: AuthBindingSource::ManagedAccount,
            auth_provider: Some("codex_oauth".to_string()),
            account_id: auth
                .get("accountId")
                .or_else(|| auth.get("account_id"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|account_id| !account_id.is_empty())
                .map(ToString::to_string),
        });
    }

    None
}

/// 从单条 Codex route 中读取 Responses -> Chat reasoning 覆盖配置。
///
/// 用途：复合路由 provider 可能同时包含 OpenAI、DeepSeek、Qwen 等不同上游；
/// 每个上游的 thinking/effort 参数语义不同，必须允许 route 层覆盖全局推断结果。
fn codex_route_chat_reasoning_config(
    upstream: &JsonValue,
    route: &JsonValue,
) -> Option<CodexChatReasoningConfig> {
    upstream
        .get("codexChatReasoning")
        .or_else(|| upstream.get("codex_chat_reasoning"))
        .or_else(|| route.get("codexChatReasoning"))
        .or_else(|| route.get("codex_chat_reasoning"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .map(normalize_codex_chat_reasoning_config)
}

/// 从单条 Codex route 中读取缓存能力覆盖配置。
///
/// MultiRouter 里同一个外层 provider 可能同时路由到 OpenAI、DeepSeek、Qwen 等不同
/// 上游；缓存机制必须跟 route 走，不能只看外层 provider 名称。
fn codex_route_cache_config(upstream: &JsonValue, route: &JsonValue) -> Option<CodexCacheConfig> {
    upstream
        .get("codexCache")
        .or_else(|| upstream.get("codex_cache"))
        .or_else(|| route.get("codexCache"))
        .or_else(|| route.get("codex_cache"))
        .or_else(|| {
            route
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("codexCache"))
        })
        .or_else(|| {
            route
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("codex_cache"))
        })
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .map(normalize_codex_cache_config)
}

/// Whether a converted Codex Responses request may send `prompt_cache_key` to
/// its Chat Completions upstream. Unknown OpenAI-compatible gateways default to
/// false because many reject unsupported request fields with HTTP 400.
pub fn should_send_codex_chat_prompt_cache_key(provider: &Provider) -> bool {
    match provider
        .meta
        .as_ref()
        .and_then(|meta| meta.prompt_cache_routing.as_deref())
        .unwrap_or("auto")
    {
        "enabled" => return true,
        "disabled" => return false,
        _ => {}
    }

    let base_url = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|value| value.as_str())
                .and_then(extract_codex_base_url_from_toml)
        });

    let Some(base_url) = base_url else {
        return false;
    };
    let Ok(url) = url::Url::parse(&base_url) else {
        return false;
    };

    match url.host_str() {
        Some("api.openai.com") => true,
        Some("api.kimi.com") => {
            let path = url.path().trim_end_matches('/');
            path == "/coding" || path.starts_with("/coding/")
        }
        _ => false,
    }
}

/// Add a stable cache-routing key after Responses -> Chat conversion. An
/// explicit client key wins; otherwise only a real client-provided session ID
/// is eligible. Generated per-request UUIDs must never be used here.
pub fn inject_codex_chat_prompt_cache_key(
    provider: &Provider,
    chat_body: &mut JsonValue,
    explicit_key: Option<&str>,
    client_session_id: Option<&str>,
) -> bool {
    if !should_send_codex_chat_prompt_cache_key(provider) {
        return false;
    }

    let key = explicit_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .or_else(|| {
            client_session_id
                .map(str::trim)
                .filter(|session_id| !session_id.is_empty())
        });
    let Some(key) = key else {
        return false;
    };

    chat_body["prompt_cache_key"] = JsonValue::String(key.to_string());
    true
}

/// Whether this Codex provider's real upstream speaks the native Anthropic
/// Messages protocol (`/v1/messages`). The local Codex client always talks to CC
/// Switch through the Responses API, so CC Switch bridges Responses ⇄ Anthropic.
///
/// Determined solely from explicit config (apiFormat / wire_api); no base_url
/// guessing — Anthropic gateway addresses vary widely and guessing easily misfires.
pub fn codex_provider_uses_anthropic(provider: &Provider) -> bool {
    if let Some(api_format) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            provider
                .settings_config
                .get("apiFormat")
                .and_then(|v| v.as_str())
        })
    {
        return is_anthropic_wire_api(api_format);
    }

    provider
        .settings_config
        .get("config")
        .and_then(|v| v.as_str())
        .and_then(extract_codex_wire_api_from_toml)
        .map(|wire_api| is_anthropic_wire_api(&wire_api))
        .unwrap_or(false)
}

pub fn should_convert_codex_responses_to_anthropic(provider: &Provider, endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path);

    matches!(
        path,
        "/responses" | "/v1/responses" | "/responses/compact" | "/v1/responses/compact"
    ) && codex_provider_uses_anthropic(provider)
}

/// Whether a native-Responses Codex upstream needs Codex `namespace`/plugin
/// tool declarations flattened before forwarding.
///
/// Codex 0.142+ emits ChatGPT-backend-private `{"type":"namespace",…}` tool
/// shapes that strict third-party Responses gateways reject with
/// `422 unknown variant "namespace"`. Only providers whose upstream is such a
/// strict native gateway need the flatten+restore pass; the Chat/Anthropic
/// transform paths already unwrap namespaces on their own. Currently that is the
/// managed xAI (Grok) OAuth provider — the first strict gateway cc-switch hit.
pub fn provider_needs_responses_namespace_flatten(provider: &Provider) -> bool {
    provider.is_xai_oauth()
}

/// The single built-in official Codex provider.  Unlike managed Codex OAuth
/// providers used by Claude, this route receives authentication from the
/// calling Codex client (`requires_openai_auth = true`).
pub fn is_codex_official_provider(provider: &Provider) -> bool {
    provider
        .settings_config
        .get(CODEX_ACCOUNT_POOL_ENABLED)
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
        || (provider.category.as_deref() == Some("official")
            && (provider.id == crate::database::CODEX_OFFICIAL_PROVIDER_ID
                || provider
                    .settings_config
                    .get(CODEX_NATIVE_AUTH_PASSTHROUGH)
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false)))
}

/// 判断 effective provider 是否明确由来向 Codex Desktop 登录态提供凭据。
///
/// `is_codex_official_provider` 还承担协议/目录识别，账号池 managed candidate 也会
/// 被它识别为 official Responses，因此不能用它单独决定 Authorization 透传。
pub(crate) fn provider_uses_native_codex_auth(provider: &Provider) -> bool {
    if let Some(native) = provider
        .settings_config
        .get(CODEX_NATIVE_AUTH_PASSTHROUGH)
        .and_then(JsonValue::as_bool)
    {
        return native;
    }
    if provider
        .settings_config
        .get(CODEX_ACCOUNT_POOL_ENABLED)
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
    {
        return false;
    }

    if provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("codex_oauth")
    {
        return false;
    }

    if provider.category.as_deref() == Some("official")
        && provider.id == crate::database::CODEX_OFFICIAL_PROVIDER_ID
    {
        return true;
    }

    false
}

/// Resolve the model-catalog tool profile for a Codex provider using the SAME
/// Anthropic detection as the proxy router ([`codex_provider_uses_anthropic`]), so the
/// generated catalog never disagrees with the routed transform. A provider whose
/// Anthropic upstream is declared only via settings `apiFormat` or TOML `wire_api`
/// (not `meta.api_format`) would otherwise get a `ProxyChat` catalog and emit the
/// freeform `apply_patch` tool that the Anthropic transform then silently drops.
/// Non-Anthropic providers keep the existing `meta.api_format` classification.
pub fn resolve_codex_catalog_tool_profile(
    provider: &Provider,
) -> crate::codex_config::CodexCatalogToolProfile {
    use crate::codex_config::CodexCatalogToolProfile;
    if is_codex_official_provider(provider) {
        return CodexCatalogToolProfile::NativeResponses;
    }
    // xAI OAuth pins the native Responses profile regardless of editable
    // api_format, mirroring the Claude-side managed-provider invariant.
    if provider.is_xai_oauth() {
        return CodexCatalogToolProfile::NativeResponses;
    }
    if codex_provider_uses_anthropic(provider) {
        return CodexCatalogToolProfile::Anthropic;
    }
    CodexCatalogToolProfile::from_api_format(
        provider.meta.as_ref().and_then(|m| m.api_format.as_deref()),
    )
}

/// Extract the real upstream model configured for a Codex provider.
pub fn codex_provider_upstream_model(provider: &Provider) -> Option<String> {
    provider
        .settings_config
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .and_then(|config| {
                    crate::grok_config::extract_model_config(config)
                        .map(|model| model.model)
                        .or_else(|| extract_codex_model_from_toml(config))
                })
        })
}

/// 按 catalog 可见模型名查找真实上游模型；没有显式别名时回退为可见名本身。
fn codex_provider_catalog_upstream_model_for_request(
    provider: &Provider,
    request_model: &str,
) -> Option<String> {
    provider
        .settings_config
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(|models| models.as_array())
        .and_then(|models| {
            models.iter().find_map(|model| {
                let visible_model = model
                    .get("model")
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())?;
                if visible_model != request_model {
                    return None;
                }
                let upstream_model = model
                    .get("upstreamModel")
                    .or_else(|| model.get("upstream_model"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .unwrap_or(visible_model);
                Some(upstream_model.to_string())
            })
        })
}

/// 将 Codex 请求体里的可见模型名改回真实上游模型名；route 显式覆盖优先于 catalog 默认映射。
pub fn apply_codex_request_upstream_model(
    provider: &Provider,
    body: &mut JsonValue,
) -> Option<String> {
    if let Some(route_override) = provider
        .settings_config
        .get(CODEX_RESOLVED_UPSTREAM_MODEL_OVERRIDE)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        body["model"] = JsonValue::String(route_override.to_string());
        return Some(route_override.to_string());
    }

    let request_model = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())?;
    let upstream_model =
        codex_provider_catalog_upstream_model_for_request(provider, request_model)?;
    body["model"] = JsonValue::String(upstream_model.clone());
    Some(upstream_model)
}

/// For Codex Chat providers, ensure the request uses the configured upstream
/// model before converting the request to Chat Completions.
pub fn apply_codex_chat_upstream_model(
    provider: &Provider,
    body: &mut JsonValue,
) -> Option<String> {
    if !codex_provider_uses_chat_completions(provider) {
        return None;
    }
    apply_codex_upstream_model(provider, body)
}

/// Apply route or catalog model mapping for every converted Codex protocol.
pub fn apply_codex_upstream_model(provider: &Provider, body: &mut JsonValue) -> Option<String> {
    if provider
        .settings_config
        .get("codexResolvedRouteMatched")
        .and_then(|value| value.as_bool())
        == Some(false)
    {
        let upstream_model = codex_provider_upstream_model(provider)?;
        body["model"] = JsonValue::String(upstream_model.clone());
        return Some(upstream_model);
    }

    if let Some(upstream_model) = apply_codex_request_upstream_model(provider, body) {
        return Some(upstream_model);
    }

    let upstream_model = codex_provider_upstream_model(provider)?;
    body["model"] = JsonValue::String(upstream_model.clone());
    Some(upstream_model)
}

pub fn resolve_codex_chat_reasoning_config(
    provider: &Provider,
    body: &JsonValue,
) -> Option<CodexChatReasoningConfig> {
    let inferred = infer_codex_chat_reasoning_config(provider, body);
    if let Some(config) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.codex_chat_reasoning.clone())
    {
        let config = normalize_codex_chat_reasoning_config(config);
        if let Some(inferred) = inferred {
            return Some(merge_qwen_vllm_reasoning_defaults(config, inferred));
        }
        return Some(config);
    }

    inferred
}

/// 解析 Codex provider 当前请求应采用的缓存能力。
///
/// 读取顺序为显式 meta、route 物化后的 capabilities、最后按 provider/model 做保守推断；
/// 推断只用于解释和安全透传，绝不为未知第三方注入 OpenAI 私有缓存参数。
pub fn resolve_codex_cache_config(provider: &Provider, body: &JsonValue) -> CodexCacheConfig {
    if let Some(config) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.codex_cache.clone())
    {
        return normalize_codex_cache_config(config);
    }

    if let Some(config) = provider
        .settings_config
        .get("codexResolvedCapabilities")
        .and_then(|capabilities| capabilities.get("codexCache"))
        .or_else(|| {
            provider
                .settings_config
                .get("codexResolvedCapabilities")
                .and_then(|capabilities| capabilities.get("codex_cache"))
        })
        .and_then(|value| serde_json::from_value(value.clone()).ok())
    {
        return normalize_codex_cache_config(config);
    }

    infer_codex_cache_config(provider, body)
}

/// 归一化缓存能力配置，兼容只写 cacheMode 的简化 route。
fn normalize_codex_cache_config(mut config: CodexCacheConfig) -> CodexCacheConfig {
    let mode = config
        .cache_mode
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mode == "openai_prompt_cache" {
        config.supports_prompt_cache_key = Some(config.supports_prompt_cache_key.unwrap_or(true));
        config.supports_prompt_cache_retention =
            Some(config.supports_prompt_cache_retention.unwrap_or(true));
        if config.usage_fields.is_empty() {
            config.usage_fields = vec![
                "usage.input_tokens_details.cached_tokens".to_string(),
                "usage.prompt_tokens_details.cached_tokens".to_string(),
            ];
        }
    }
    if mode == "deepseek_context_cache" && config.usage_fields.is_empty() {
        config.usage_fields = vec![
            "usage.prompt_cache_hit_tokens".to_string(),
            "usage.prompt_cache_miss_tokens".to_string(),
        ];
    }
    if matches!(
        mode.as_str(),
        "auto_prefix_cache" | "zai_context_cache" | "glm_context_cache"
    ) && config.usage_fields.is_empty()
    {
        config.usage_fields = vec!["usage.prompt_tokens_details.cached_tokens".to_string()];
    }
    config
}

/// 按 provider 家族做保守缓存能力推断。
///
/// 这里只有“不会破坏请求”的默认值：DeepSeek/GLM/Qwen 标成自动缓存但不启用
/// OpenAI cache 参数；只有官方 OpenAI/Codex OAuth 或明确 OpenAI provider 才启用
/// prompt_cache_key / prompt_cache_retention。
fn infer_codex_cache_config(provider: &Provider, body: &JsonValue) -> CodexCacheConfig {
    let model = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(str::to_ascii_lowercase)
        .or_else(|| codex_provider_upstream_model(provider).map(|value| value.to_ascii_lowercase()))
        .unwrap_or_default();
    let provider_type = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        .unwrap_or("");
    let provider_text =
        format!("{} {} {}", provider.id, provider.name, provider_type).to_ascii_lowercase();

    let mut config = if provider_text.contains("deepseek") || model.contains("deepseek") {
        CodexCacheConfig {
            cache_mode: Some("deepseek_context_cache".to_string()),
            usage_fields: vec![
                "usage.prompt_cache_hit_tokens".to_string(),
                "usage.prompt_cache_miss_tokens".to_string(),
            ],
            ..CodexCacheConfig::default()
        }
    } else if provider_text.contains("z.ai")
        || provider_text.contains("zai")
        || provider_text.contains("glm")
        || model.contains("glm")
    {
        CodexCacheConfig {
            cache_mode: Some("glm_context_cache".to_string()),
            usage_fields: vec!["usage.prompt_tokens_details.cached_tokens".to_string()],
            ..CodexCacheConfig::default()
        }
    } else if provider_text.contains("dashscope")
        || provider_text.contains("qwen")
        || model.contains("qwen")
    {
        CodexCacheConfig {
            cache_mode: Some("qwen_context_cache".to_string()),
            usage_fields: vec![
                "usage.input_tokens_details.cached_tokens".to_string(),
                "usage.prompt_tokens_details.cached_tokens".to_string(),
                "usage.prompt_tokens_details.cache_creation_input_tokens".to_string(),
            ],
            ..CodexCacheConfig::default()
        }
    } else if provider_text.contains("codex_oauth")
        || provider_text.contains("openai official")
        || provider_text.trim() == "openai openai"
        || model.starts_with("gpt-")
        || model.starts_with('o')
    {
        CodexCacheConfig {
            cache_mode: Some("openai_prompt_cache".to_string()),
            supports_prompt_cache_key: Some(true),
            supports_prompt_cache_retention: Some(true),
            usage_fields: vec![
                "usage.input_tokens_details.cached_tokens".to_string(),
                "usage.prompt_tokens_details.cached_tokens".to_string(),
            ],
            ..CodexCacheConfig::default()
        }
    } else {
        CodexCacheConfig {
            cache_mode: Some("unknown".to_string()),
            ..CodexCacheConfig::default()
        }
    };

    if let Some(meta) = provider.meta.as_ref() {
        if config.prompt_cache_key.is_none() {
            config.prompt_cache_key = meta.prompt_cache_key.clone();
        }
        if config.prompt_cache_retention.is_none() {
            config.prompt_cache_retention = meta.prompt_cache_retention.clone();
        }
    }
    normalize_codex_cache_config(config)
}

fn normalize_codex_chat_reasoning_config(
    mut config: CodexChatReasoningConfig,
) -> CodexChatReasoningConfig {
    if config.supports_effort.unwrap_or(false) && config.supports_thinking.is_none() {
        config.supports_thinking = Some(true);
    }
    config
}

/// 合并 Qwen/vLLM 的运行时兼容默认值。
///
/// 历史 provider 可能已经持久化了 `thinkingParam=thinking` 且没有
/// `minOutputTokens` 的显式 meta；这会阻断 Qwen/vLLM 推断分支。只有当推断结果
/// 明确识别为 Qwen/vLLM 时，才纠正过时字段和过小显式预算，避免影响 DeepSeek、
/// OpenRouter 等需要完整显式覆盖的平台。
fn merge_qwen_vllm_reasoning_defaults(
    mut explicit: CodexChatReasoningConfig,
    inferred: CodexChatReasoningConfig,
) -> CodexChatReasoningConfig {
    if !is_qwen_vllm_reasoning_defaults(&inferred) {
        return explicit;
    }

    if explicit.supports_thinking.is_none() {
        explicit.supports_thinking = inferred.supports_thinking;
    }
    if explicit.supports_effort.is_none() {
        explicit.supports_effort = inferred.supports_effort;
    }

    let thinking_param = explicit
        .thinking_param
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if thinking_param.is_empty() || thinking_param == "thinking" {
        explicit.thinking_param = inferred.thinking_param;
    }
    if explicit.effort_param.is_none() {
        explicit.effort_param = inferred.effort_param;
    }
    if explicit.effort_value_mode.is_none() {
        explicit.effort_value_mode = inferred.effort_value_mode;
    }
    if explicit.output_format.is_none() {
        explicit.output_format = inferred.output_format;
    }
    if let Some(inferred_min_output_tokens) = inferred.min_output_tokens {
        if explicit
            .min_output_tokens
            .map(|current| current < inferred_min_output_tokens)
            .unwrap_or(true)
        {
            explicit.min_output_tokens = Some(inferred_min_output_tokens);
        }
    }
    if let Some(inferred_default_output_tokens) = inferred.default_output_tokens {
        if explicit
            .default_output_tokens
            .map(|current| current < inferred_default_output_tokens)
            .unwrap_or(true)
        {
            explicit.default_output_tokens = Some(inferred_default_output_tokens);
        }
    }
    if explicit.default_output_tokens == Some(RETIRED_QWEN_VLLM_DEFAULT_OUTPUT_TOKENS)
        && inferred.default_output_tokens.is_none()
    {
        explicit.default_output_tokens = None;
    }

    normalize_codex_chat_reasoning_config(explicit)
}

/// 判断推断结果是否是 Qwen/vLLM 专用默认值。
fn is_qwen_vllm_reasoning_defaults(config: &CodexChatReasoningConfig) -> bool {
    config.thinking_param.as_deref() == Some("enable_thinking")
        && config.effort_param.as_deref() == Some("none")
        && config.min_output_tokens == Some(QWEN_VLLM_MIN_OUTPUT_TOKENS)
        && config.output_format.as_deref() == Some("reasoning_content")
}

fn infer_codex_chat_reasoning_config(
    provider: &Provider,
    body: &JsonValue,
) -> Option<CodexChatReasoningConfig> {
    let model = body
        .get("model")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(|| codex_provider_upstream_model(provider))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let base_url = provider
        .settings_config
        .get("base_url")
        .or_else(|| provider.settings_config.get("baseURL"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            provider
                .settings_config
                .get("config")
                .and_then(|v| v.as_str())
                .and_then(extract_codex_base_url_from_toml)
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = provider.name.to_ascii_lowercase();

    // 平台优先：聚合 / 托管平台的 reasoning 接口由平台的推理框架决定，而非模型官方实现，
    // 因此先按平台标识（仅 name + base_url，不含 model 名）判定并覆盖模型规则。
    if let Some(config) = infer_aggregator_platform_config(&name, &base_url) {
        return Some(config);
    }

    let haystack = format!("{name} {base_url} {model}");

    if haystack.contains("deepseek") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("deepseek".to_string()),
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    // StepFun：仅 step-3.5-flash-2603 这一版支持 reasoning effort（low/high 两档），
    // 其余 step 模型不暴露 effort，故 supports_effort 仅对含 "2603" 的模型置真。
    // 第二个 OR 分支覆盖「经中转/聚合跑该模型、但平台 name/base_url 不含 stepfun」的情况。
    if haystack.contains("stepfun") || haystack.contains("step-3.5-flash-2603") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(model.contains("2603")),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("low_high".to_string()),
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("reasoning".to_string()),
        });
    }

    if haystack.contains("kimi") || haystack.contains("moonshot") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    if haystack.contains("glm") || haystack.contains("zhipu") || haystack.contains("z.ai") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(true),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("reasoning_effort".to_string()),
            effort_value_mode: Some("deepseek".to_string()),
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    // 本地 / vLLM 托管的 Qwen 兼容端点会先输出 reasoning；Codex 小
    // `max_output_tokens` 请求容易被思考内容吃满，因此只声明显式预算的最小下限。
    // Codex 完全缺省时应继续交给 vLLM 自身默认策略，不能在路由层强行截断输出长度。
    if haystack.contains("qwen")
        && (haystack.contains("vllm") || haystack.contains("matrixminecraft"))
    {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("enable_thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            min_output_tokens: Some(QWEN_VLLM_MIN_OUTPUT_TOKENS),
            default_output_tokens: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    if haystack.contains("qwen") || haystack.contains("dashscope") || haystack.contains("bailian") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("enable_thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    if haystack.contains("minimax") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("reasoning_split".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("reasoning_details".to_string()),
        });
    }

    if haystack.contains("mimo") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    None
}

/// 聚合 / 托管平台的 reasoning 接口由平台决定：同一个模型在不同平台参数可能完全不同
/// （DeepSeek 官方用 `thinking:{type}`、SiliconFlow 用 `enable_thinking`、
/// OpenRouter 用原生 `reasoning:{effort}` 对象）。仅以平台标识（name / base_url）判定，
/// 绝不掺入 model 名——model 名属于模型厂商，会把托管平台误判成模型官方接口。
fn infer_aggregator_platform_config(
    name: &str,
    base_url: &str,
) -> Option<CodexChatReasoningConfig> {
    let platform = format!("{name} {base_url}");

    // OpenRouter：用原生归一化对象 `reasoning: { effort }`（由 OpenRouter 翻译成各底层
    // 模型的正确推理参数，比顶层 OpenAI 别名 reasoning_effort 覆盖面更全）。effort 走
    // "openrouter" 值映射：枚举为 xhigh|high|medium|low|minimal，无 max——max 会触发
    // `400 reasoning_effort: Invalid option`（见 openclaw#77350），故钳到 xhigh。
    // 安全降级：不发 `thinking:{type}`（OpenRouter 不认该字段），避免误配导致请求被拒。
    if platform.contains("openrouter") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(false),
            supports_effort: Some(true),
            thinking_param: Some("none".to_string()),
            effort_param: Some("reasoning.effort".to_string()),
            effort_value_mode: Some("openrouter".to_string()),
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("auto".to_string()),
        });
    }

    // SiliconFlow：平台级统一 `enable_thinking`，思维回传 reasoning_content。
    // 安全降级：不按 reasoning_effort 发 effort（平台用 thinking_budget 控制深度，
    // 发 reasoning_effort 反而可能不被接受）。
    if platform.contains("siliconflow") {
        return Some(CodexChatReasoningConfig {
            supports_thinking: Some(true),
            supports_effort: Some(false),
            thinking_param: Some("enable_thinking".to_string()),
            effort_param: Some("none".to_string()),
            effort_value_mode: None,
            min_output_tokens: None,
            default_output_tokens: None,
            output_format: Some("reasoning_content".to_string()),
        });
    }

    None
}

fn is_chat_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "chat"
            | "chat_completions"
            | "chat-completions"
            | "openai_chat"
            | "openai-chat"
            | "openai_chat_completions"
    )
}

/// 把各种历史/兼容写法归一化成 Codex 上游协议枚举。
fn codex_upstream_protocol_from_api_format(value: &str) -> CodexResponsesUpstreamProtocol {
    if is_anthropic_wire_api(value) {
        return CodexResponsesUpstreamProtocol::Anthropic;
    }
    if is_openai_messages_wire_api(value) {
        return CodexResponsesUpstreamProtocol::Messages;
    }
    if is_chat_wire_api(value) {
        return CodexResponsesUpstreamProtocol::Chat;
    }
    CodexResponsesUpstreamProtocol::Responses
}

/// 判断是否为 OpenAI 的 Messages 风格 API：
/// `messages`/`openai_messages` 需要把 Responses 转换为 Chat 请求中的 `messages`。
fn is_openai_messages_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "openai_messages" | "openai-messages"
    )
}

fn is_anthropic_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "anthropic" | "anthropic_messages" | "anthropic-messages" | "claude" | "messages"
    )
}

fn is_chat_completions_url(value: &str) -> bool {
    value
        .trim_end_matches('/')
        .to_ascii_lowercase()
        .ends_with("/chat/completions")
}

/// 统一判断当前入口是否是 Codex Responses 路径。
///
/// 参数:
/// - `endpoint`: 本地代理收到或改写后的 endpoint，可带 query。
///   返回:
/// - `true` 表示该请求是 Codex `/responses` 或 `/responses/compact`。
///   副作用:
/// - 无。
pub(crate) fn is_codex_responses_endpoint(endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path);
    matches!(
        path,
        "/responses" | "/v1/responses" | "/responses/compact" | "/v1/responses/compact"
    )
}

pub(crate) fn is_codex_remote_compact_endpoint(endpoint: &str) -> bool {
    let path = endpoint
        .split_once('?')
        .map_or(endpoint, |(path, _query)| path);
    matches!(path, "/responses/compact" | "/v1/responses/compact")
}

/// 判断是否为已知的 OpenAI Chat Completions-only 兼容上游。
///
/// 用于兼容旧数据：一些 provider 曾经把 `wire_api` 误写成 `responses`，
/// 但真实服务端只提供 `/chat/completions`。
fn is_known_chat_completions_only_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    is_chat_completions_url(&lower)
        || [
            "api.deepseek.com",
            "api.moonshot.cn",
            "dashscope.aliyuncs.com",
            "open.bigmodel.cn",
            "api.siliconflow.cn",
            "sensenova.cn",
            "openrouter.ai",
            "vllm",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
}

/// `scheme://host` 之后没有路径段的纯 origin 形式。`build_url` 在这种情况下
/// 会自动补 `/v1`；Stream Check 等同步生产路径的代码也需要同一判定。
pub fn is_origin_only_url(value: &str) -> bool {
    let trimmed = value.trim_end_matches('/');
    match trimmed.split_once("://") {
        Some((_scheme, rest)) => !rest.contains('/'),
        None => !trimmed.contains('/'),
    }
}

fn extract_codex_wire_api_from_toml(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<TomlValue>().ok()?;

    if let Some(active_provider) = doc.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(wire_api) = doc
            .get("model_providers")
            .and_then(|providers| providers.get(active_provider))
            .and_then(|provider| provider.get("wire_api"))
            .and_then(|v| v.as_str())
        {
            return Some(wire_api.to_string());
        }
    }

    doc.get("wire_api")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn extract_codex_model_from_toml(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<TomlValue>().ok()?;

    doc.get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
}

fn extract_codex_base_url_from_toml(config_text: &str) -> Option<String> {
    // Canonical parser lives in codex_config; keep this thin alias so the
    // proxy hot path and the usage-credential resolver share one implementation.
    crate::codex_config::extract_codex_base_url(config_text)
}

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 检测是否为官方 Codex 客户端
    ///
    /// 匹配 User-Agent 模式: `^(codex_vscode|codex_cli_rs)/[\d.]+`
    #[allow(dead_code)]
    pub fn is_official_client(user_agent: &str) -> bool {
        CODEX_CLIENT_REGEX.is_match(user_agent)
    }

    /// 从 Provider 配置中提取 API Key
    fn extract_key(&self, provider: &Provider) -> Option<String> {
        // 1. 尝试从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(key) = env
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|key| !key.is_empty())
            {
                return Some(key.to_string());
            }
        }

        // 2. 尝试从 auth 中获取 (Codex CLI 格式)
        if let Some(auth) = provider.settings_config.get("auth") {
            if let Some(key) = crate::codex_config::extract_codex_auth_api_key(auth) {
                return Some(key.to_string());
            }
        }

        // 3. 尝试直接获取
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            return Some(key.to_string());
        }

        // 4. 尝试从 config 对象中获取
        if let Some(config) = provider.settings_config.get("config") {
            if let Some(key) = config
                .get("api_key")
                .or_else(|| config.get("apiKey"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|key| !key.is_empty())
            {
                return Some(key.to_string());
            }

            if let Some(config_str) = config.as_str() {
                if let Some((_, key)) = crate::grok_config::extract_credentials(config_str) {
                    return Some(key);
                }
                if let Some(key) =
                    crate::codex_config::extract_codex_experimental_bearer_token(config_str)
                {
                    return Some(key);
                }
            }
        }

        None
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "Codex"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // Codex v2 路由到 ChatGPT OAuth 时仍然固定使用 CodexAdapter；
        // 这里补齐托管账号 provider 的 base_url 语义，避免走普通 OpenAI 兼容配置解析。
        if provider_is_managed_codex_oauth(provider) || is_codex_official_provider(provider) {
            return Ok(super::CHATGPT_CODEX_BASE_URL.to_string());
        }

        // xAI OAuth: ignore editable provider base URLs and always use the xAI
        // API origin associated with the managed token.
        if provider.is_xai_oauth() {
            return Ok(super::XAI_API_BASE_URL.to_string());
        }

        // 1. 尝试直接获取 base_url 字段
        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 2. 尝试 baseURL
        if let Some(url) = provider
            .settings_config
            .get("baseURL")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        // 3. 尝试从 config 对象中获取
        if let Some(config) = provider.settings_config.get("config") {
            if let Some(url) = config.get("base_url").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }

            // 尝试解析 TOML 字符串格式
            if let Some(config_str) = config.as_str() {
                if let Some(url) = extract_codex_base_url_from_toml(config_str) {
                    return Ok(url.trim_end_matches('/').to_string());
                }
                if let Some(url) = crate::grok_config::extract_base_url(config_str) {
                    return Ok(url.trim_end_matches('/').to_string());
                }
            }
        }

        Err(ProxyError::ConfigError(
            "Codex Provider 缺少 base_url 配置".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        // Native Desktop routes receive the caller's Authorization header.
        // Check this before legacy empty-official-seed inference, otherwise the
        // built-in codex-official row is mistaken for CCSM-managed OAuth.
        if provider_uses_native_codex_auth(provider) {
            return None;
        }

        // ChatGPT Codex OAuth 的真实 access_token 由 forwarder 动态换取；
        // adapter 这里只返回策略占位，保持和 ClaudeAdapter 的托管账号语义一致。
        if provider_is_managed_codex_oauth(provider) {
            return Some(AuthInfo::new(
                "codex_oauth_placeholder".to_string(),
                AuthStrategy::CodexOAuth,
            ));
        }

        // xAI OAuth (Grok subscription): placeholder credentials only; the real
        // access_token is resolved per-request by the forwarder via XaiOAuthManager.
        if provider.is_xai_oauth() {
            return Some(AuthInfo::new(
                "xai_oauth_placeholder".to_string(),
                AuthStrategy::XaiOAuth,
            ));
        }

        // Anthropic upstream: the auth field is chosen by the user in the UI (meta.apiKeyField).
        //   ANTHROPIC_API_KEY    → x-api-key (AuthStrategy::Anthropic)
        //   ANTHROPIC_AUTH_TOKEN → Authorization: Bearer (default, AuthStrategy::Bearer)
        // The two are mutually exclusive to avoid a 401 from the gateway receiving
        // both auth headers at once. All other Codex upstreams stay pure Bearer.
        let strategy = if codex_provider_uses_anthropic(provider) {
            let uses_x_api_key = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.api_key_field.as_deref())
                .map(|field| field.eq_ignore_ascii_case("ANTHROPIC_API_KEY"))
                .unwrap_or(false);
            if uses_x_api_key {
                AuthStrategy::Anthropic
            } else {
                AuthStrategy::Bearer
            }
        } else {
            AuthStrategy::Bearer
        };
        self.extract_key(provider)
            .map(|key| AuthInfo::new(key, strategy))
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        let base_trimmed = base_url.trim_end_matches('/');
        let endpoint_trimmed = endpoint.trim_start_matches('/');

        // ChatGPT Codex 后端不是标准 OpenAI `/v1` 服务。Codex 客户端命中
        // 本地代理时会请求 `/v1/responses`，但上游真实路径必须是
        // `/backend-api/codex/responses`。这里在 CodexAdapter 层做归一化，
        // 避免多模型路由到托管 OAuth 时拼成不可用的
        // `/backend-api/codex/v1/responses`。
        if base_trimmed == "https://chatgpt.com/backend-api/codex" {
            let normalized_endpoint = endpoint_trimmed
                .strip_prefix("v1/")
                .unwrap_or(endpoint_trimmed);
            return format!("{base_trimmed}/{normalized_endpoint}");
        }

        // OpenAI/Codex 的 base_url 可能是：
        // - 纯 origin: https://api.openai.com  (需要自动补 /v1)
        // - 已含 /v1: https://api.openai.com/v1 (直接拼接)
        // - 自定义前缀: https://xxx/openai (不添加 /v1，直接拼接)

        // 检查 base_url 是否已经包含 /v1
        let already_has_v1 = base_trimmed.ends_with("/v1");
        let origin_only = is_origin_only_url(base_trimmed);

        let mut url = if already_has_v1 {
            // 已经有 /v1，直接拼接
            format!("{base_trimmed}/{endpoint_trimmed}")
        } else if origin_only {
            // 纯 origin，添加 /v1
            format!("{base_trimmed}/v1/{endpoint_trimmed}")
        } else {
            // 自定义前缀，不添加 /v1，直接拼接
            format!("{base_trimmed}/{endpoint_trimmed}")
        };

        // 去除重复的 /v1/v1（可能由 base_url 与 endpoint 都带版本导致）
        while url.contains("/v1/v1") {
            url = url.replace("/v1/v1", "/v1");
        }

        url
    }

    fn get_auth_headers(
        &self,
        auth: &AuthInfo,
    ) -> Result<Vec<(http::HeaderName, http::HeaderValue)>, ProxyError> {
        use super::adapter::auth_header_value;
        let bearer = format!("Bearer {}", auth.api_key);
        // OAuth 的 originator 必须在完整 header 合并后由 forwarder 统一覆盖，避免重复值。
        // Anthropic gateway: send only x-api-key (anthropic-version is filled in by
        // the forwarder). Mutually exclusive with Bearer to avoid a 401 from the
        // gateway receiving both auth headers at once.
        if auth.strategy == AuthStrategy::Anthropic {
            return Ok(vec![(
                http::HeaderName::from_static("x-api-key"),
                auth_header_value(&auth.api_key)?,
            )]);
        }
        Ok(vec![(
            http::HeaderName::from_static("authorization"),
            auth_header_value(&bearer)?,
        )])
    }
}

/// 判断任意 Codex provider 是否应使用托管 ChatGPT/Codex OAuth。
///
/// 新数据直接读取 `meta.providerType = "codex_oauth"`；旧版 official provider 可能只
/// 有 `auth.auth_mode = "chatgpt"` 和 OAuth tokens，且没有 base_url。第三方
/// OpenAI-compatible API profile 直接指向这类 provider 时不会经过 MultiRouter 物化，
/// 因此 adapter 本身也必须能识别它。
fn provider_is_managed_codex_oauth(provider: &Provider) -> bool {
    if provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        == Some("codex_oauth")
    {
        return true;
    }

    target_provider_looks_like_managed_codex_oauth(provider, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::codex_oauth_auth::{
        CodexAccountPoolEntry, CodexAccountPoolPolicy, NATIVE_CODEX_ACCOUNT_ID,
    };
    use serde_json::json;

    fn create_provider(config: serde_json::Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Codex".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("codex".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn multirouter_with_routes(
        routes: serde_json::Value,
        official_auth: Option<serde_json::Value>,
    ) -> Provider {
        let mut routing = json!({
            "enabled": true,
            "routes": routes,
        });
        if let Some(official_auth) = official_auth {
            routing["officialAuth"] = official_auth;
        }
        create_provider(json!({ "codexRouting": routing }))
    }

    #[test]
    fn grok_build_toml_exposes_upstream_credentials_and_model() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "config": r#"
[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "upstream-grok-model"
base_url = "https://relay.example.com/v1/"
name = "Example Relay"
api_key = "grok-secret"
api_backend = "responses"
context_window = 500000
"#
        }));

        assert_eq!(
            adapter.extract_base_url(&provider).unwrap(),
            "https://relay.example.com/v1"
        );
        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "grok-secret");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
        assert_eq!(
            codex_provider_upstream_model(&provider).as_deref(),
            Some("upstream-grok-model")
        );
    }

    #[test]
    fn official_provider_uses_fixed_chatgpt_backend_without_stored_key() {
        let mut provider = create_provider(json!({ "auth": {}, "config": "" }));
        provider.id = "codex-official".to_string();
        provider.category = Some("official".to_string());
        let adapter = CodexAdapter::new();

        assert!(is_codex_official_provider(&provider));
        assert_eq!(
            adapter
                .extract_base_url(&provider)
                .expect("official base url"),
            "https://chatgpt.com/backend-api/codex"
        );
        assert!(adapter.extract_auth(&provider).is_none());
        assert_eq!(
            adapter.build_url(
                "https://chatgpt.com/backend-api/codex",
                "/responses/compact"
            ),
            "https://chatgpt.com/backend-api/codex/responses/compact"
        );
    }

    fn pool_policy(enabled: bool, native_enabled: bool) -> CodexAccountPoolPolicy {
        CodexAccountPoolPolicy {
            enabled,
            entries: vec![
                CodexAccountPoolEntry {
                    account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                    enabled: native_enabled,
                    reserve_percent: 5.0,
                },
                CodexAccountPoolEntry {
                    account_id: "managed-account".to_string(),
                    enabled: true,
                    reserve_percent: 5.0,
                },
            ],
            desktop_account_id: None,
        }
    }

    #[test]
    fn codex_multirouter_auth_facade_classifies_explicit_route_sources() {
        let native = multirouter_with_routes(
            json!([{
                "id": "official",
                "enabled": true,
                "upstream": { "auth": { "source": "native_codex_auth" } }
            }]),
            Some(json!({ "mode": "desktop_current_login" })),
        );
        assert_eq!(
            classify_codex_multirouter_auth_facade(&native, None),
            CodexMultiRouterAuthFacade::NativeMixed
        );

        let managed = multirouter_with_routes(
            json!([{
                "id": "official",
                "enabled": true,
                "upstream": { "auth": { "source": "managed_codex_oauth", "accountId": "managed-account" } }
            }]),
            Some(json!({ "mode": "managed_oauth", "accountId": "managed-account" })),
        );
        assert_eq!(
            classify_codex_multirouter_auth_facade(&managed, None),
            CodexMultiRouterAuthFacade::FullyManaged
        );

        let provider_config = multirouter_with_routes(
            json!([{
                "id": "third-party",
                "enabled": true,
                "upstream": { "auth": { "source": "provider_config" } }
            }]),
            None,
        );
        assert_eq!(
            classify_codex_multirouter_auth_facade(&provider_config, None),
            CodexMultiRouterAuthFacade::FullyManaged
        );
    }

    #[test]
    fn codex_multirouter_auth_facade_uses_enabled_account_pool_entries() {
        let router = multirouter_with_routes(
            json!([{
                "id": "official-pool",
                "enabled": true,
                "upstream": { "auth": { "source": "account_pool" } }
            }]),
            Some(json!({ "mode": "account_pool" })),
        );

        assert_eq!(
            classify_codex_multirouter_auth_facade(&router, Some(&pool_policy(true, true))),
            CodexMultiRouterAuthFacade::NativeMixed
        );
        assert_eq!(
            classify_codex_multirouter_auth_facade(&router, Some(&pool_policy(true, false))),
            CodexMultiRouterAuthFacade::FullyManaged
        );
        assert_eq!(
            classify_codex_multirouter_auth_facade(&router, Some(&pool_policy(false, true))),
            CodexMultiRouterAuthFacade::FullyManaged,
            "a globally disabled pool cannot currently select Desktop auth"
        );
        assert_eq!(
            classify_codex_multirouter_auth_facade(&router, None),
            CodexMultiRouterAuthFacade::LegacyPreserved,
            "without a pool snapshot the backend must not guess credential ownership"
        );
    }

    #[test]
    fn codex_multirouter_auth_facade_ignores_disabled_routes_and_preserves_ambiguity() {
        let router = multirouter_with_routes(
            json!([
                {
                    "id": "disabled-native",
                    "enabled": false,
                    "upstream": { "auth": { "source": "native_codex_auth" } }
                },
                {
                    "id": "managed",
                    "enabled": true,
                    "upstream": { "auth": { "source": "managed_account" } }
                }
            ]),
            None,
        );
        assert_eq!(
            classify_codex_multirouter_auth_facade(&router, None),
            CodexMultiRouterAuthFacade::FullyManaged
        );

        let ambiguous = multirouter_with_routes(
            json!([{
                "id": "legacy",
                "enabled": true,
                "upstream": { "apiFormat": "openai_responses" }
            }]),
            None,
        );
        assert_eq!(
            classify_codex_multirouter_auth_facade(&ambiguous, None),
            CodexMultiRouterAuthFacade::LegacyPreserved
        );
    }

    #[test]
    fn codex_multirouter_plaintext_v2_delivery_depends_on_enabled_route_ownership() {
        let mixed = multirouter_with_routes(
            json!([
                {
                    "id": "official",
                    "enabled": true,
                    "upstream": { "auth": { "source": "managed_codex_oauth" } }
                },
                {
                    "id": "third-party",
                    "enabled": true,
                    "upstream": { "auth": { "source": "provider_config" } }
                }
            ]),
            None,
        );
        assert!(codex_multirouter_needs_plaintext_v2_collaboration(&mixed));

        let official_only = multirouter_with_routes(
            json!([{
                "id": "official",
                "enabled": true,
                "upstream": { "auth": { "source": "account_pool" } }
            }]),
            None,
        );
        assert!(!codex_multirouter_needs_plaintext_v2_collaboration(
            &official_only
        ));

        let disabled_third_party = multirouter_with_routes(
            json!([
                {
                    "id": "official",
                    "enabled": true,
                    "upstream": { "auth": { "source": "native_codex_auth" } }
                },
                {
                    "id": "third-party",
                    "enabled": false,
                    "upstream": { "auth": { "source": "provider_config" } }
                }
            ]),
            None,
        );
        assert!(!codex_multirouter_needs_plaintext_v2_collaboration(
            &disabled_third_party
        ));

        let ambiguous = multirouter_with_routes(
            json!([{
                "id": "legacy",
                "enabled": true,
                "upstream": { "apiFormat": "openai_responses" }
            }]),
            None,
        );
        assert!(codex_multirouter_needs_plaintext_v2_collaboration(
            &ambiguous
        ));
    }

    #[test]
    /// Codex OAuth adapter 只提供认证头，来源身份由 forwarder 在最终出站阶段统一写入。
    fn test_codex_oauth_auth_headers_defer_originator_to_forwarder() {
        let adapter = CodexAdapter::new();
        let auth = AuthInfo::new("oauth-token".to_string(), AuthStrategy::CodexOAuth);

        let headers = adapter.get_auth_headers(&auth).expect("OAuth headers");

        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0.as_str(), "authorization");
        assert_eq!(headers[0].1.to_str().unwrap(), "Bearer oauth-token");
    }

    #[test]
    fn test_codex_responses_provider_does_not_convert_to_chat() {
        let mut provider = create_provider(json!({
            "config": r#"model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
base_url = "https://www.matrixminecraft.cn:24443/ccswitch/v1"
wire_api = "responses"
experimental_bearer_token = "ccsw-test"
"#,
        }));
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });

        assert!(!codex_provider_uses_chat_completions(&provider));
        assert!(!should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_codex_responses_provider_ignores_stale_top_level_proxy_url() {
        let mut provider = create_provider(json!({
            "config": r#"base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
model = "gpt-5.5"
model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
base_url = "https://www.matrixminecraft.cn:24443/ccswitch/v1"
wire_api = "responses"
experimental_bearer_token = "ccsw-test"

[model_providers.codex_model_router_v2]
name = "OpenAI Multi-Model Router"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
requires_openai_auth = true
experimental_bearer_token = "PROXY_MANAGED"
"#,
        }));
        provider.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });

        assert_eq!(
            extract_codex_base_url_from_toml(
                provider
                    .settings_config
                    .get("config")
                    .and_then(|value| value.as_str())
                    .expect("config toml")
            )
            .as_deref(),
            Some("https://www.matrixminecraft.cn:24443/ccswitch/v1")
        );
        assert!(!should_convert_codex_responses_to_chat(
            &provider,
            "/responses"
        ));
    }

    #[test]
    fn test_codex_model_route_resolves_deepseek_chat_provider() {
        let provider = create_provider(json!({
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-flash" },
                    { "model": "gpt-5.5" }
                ]
            },
            "codexModelRoutes": [
                {
                    "id": "deepseek",
                    "name": "DeepSeek",
                    "models": ["deepseek-v4-flash", "deepseek-v4-pro"],
                    "base_url": "https://api.deepseek.com",
                    "wire_api": "chat",
                    "auth": { "OPENAI_API_KEY": "sk-deepseek" }
                }
            ]
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("deepseek route");

        assert_eq!(routed.name, "DeepSeek");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://api.deepseek.com"
        );
        assert_eq!(
            codex_route_persistent_provider(&routed),
            ("test", "Test Codex")
        );
        assert_eq!(
            routed
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_chat")
        );
        assert!(should_convert_codex_responses_to_chat(
            &routed,
            "/responses"
        ));
    }

    #[test]
    fn test_codex_route_target_provider_reuses_provider_conversion_config() {
        let router = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "deepseek",
                    "label": "DeepSeek Route",
                    "targetProviderId": "codex-deepseek",
                    "match": { "models": ["deepseek-v4-flash"] }
                }]
            }
        }));
        let mut target = Provider::with_id(
            "codex-deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({
                "base_url": "https://api.deepseek.com",
                "auth": { "OPENAI_API_KEY": "sk-target" },
                "model": "deepseek-chat"
            }),
            None,
        );
        target.meta = Some(ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });

        let routed =
            resolve_codex_model_routed_provider(&router, &json!({ "model": "deepseek-v4-flash" }))
                .expect("deepseek route");
        assert_eq!(
            codex_route_target_provider_id(&routed),
            Some("codex-deepseek")
        );

        let materialized = materialize_codex_routed_provider_from_target(&routed, &target);

        assert_eq!(materialized.id, "test::route::deepseek");
        assert_eq!(
            materialized.settings_config["base_url"],
            "https://api.deepseek.com"
        );
        assert_eq!(materialized.settings_config["model"], "deepseek-chat");
        assert_eq!(
            codex_route_persistent_provider(&materialized),
            ("test", "Test Codex")
        );
        assert!(should_convert_codex_responses_to_chat(
            &materialized,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_codex_route_target_provider_infers_legacy_official_oauth_base_url() {
        let adapter = CodexAdapter::new();
        let router = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "official",
                    "label": "OpenAI Official",
                    "targetProviderId": "codex-official",
                    "match": { "models": ["gpt-5.5"] },
                    "upstream": {
                        "apiFormat": "openai_responses",
                        "auth": { "source": "provider_config" }
                    }
                }]
            }
        }));
        let target = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official Backup".to_string(),
            json!({
                "auth": {
                    "auth_mode": "chatgpt",
                    "tokens": {
                        "access_token": "managed-access-token"
                    }
                },
                "config": "model_reasoning_effort = \"medium\"\n"
            }),
            None,
        );

        let routed = resolve_codex_model_routed_provider(&router, &json!({ "model": "gpt-5.5" }))
            .expect("official route");
        let materialized = materialize_codex_routed_provider_from_target(&routed, &target);

        assert_eq!(
            materialized
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("codex_oauth")
        );
        assert_eq!(
            adapter.extract_base_url(&materialized).unwrap(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            adapter.extract_auth(&materialized).unwrap().strategy,
            AuthStrategy::CodexOAuth
        );
        assert_eq!(
            adapter.build_url(
                &adapter.extract_base_url(&materialized).unwrap(),
                "/v1/responses"
            ),
            "https://chatgpt.com/backend-api/codex/responses"
        );
    }

    #[test]
    fn test_codex_route_target_provider_infers_official_oauth_from_router_auth() {
        let adapter = CodexAdapter::new();
        let router = create_provider(json!({
            "auth": {
                "auth_mode": "chatgpt",
                "tokens": {
                    "access_token": "router-managed-access-token"
                }
            },
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "router-codex-official",
                    "label": "OpenAI Official",
                    "targetProviderId": "codex-official",
                    "match": { "models": ["gpt-5.5"] },
                    "upstream": {
                        "apiFormat": "openai_chat",
                        "auth": { "source": "provider_config" }
                    }
                }],
                "defaultRouteId": "router-codex-official"
            }
        }));
        let target = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "auth": {},
                "config": ""
            }),
            None,
        );

        let routed = resolve_codex_model_routed_provider(&router, &json!({ "model": "gpt-5.5" }))
            .expect("official route");
        let materialized = materialize_codex_routed_provider_from_target(&routed, &target);

        assert_eq!(
            materialized
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("codex_oauth")
        );
        assert_eq!(
            adapter.extract_base_url(&materialized).unwrap(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            adapter.extract_auth(&materialized).unwrap().strategy,
            AuthStrategy::CodexOAuth
        );
    }

    #[test]
    fn test_codex_route_target_provider_infers_empty_official_seed_as_managed_oauth() {
        let adapter = CodexAdapter::new();
        let router = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "router-codex-official",
                    "label": "OpenAI Official",
                    "targetProviderId": "codex-official",
                    "match": { "models": ["gpt-5.5"] },
                    "upstream": {
                        "apiFormat": "openai_responses",
                        "auth": { "source": "provider_config" }
                    }
                }],
                "defaultRouteId": "router-codex-official"
            }
        }));
        let mut target = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official Backup".to_string(),
            json!({
                "auth": {},
                "config": ""
            }),
            None,
        );
        target.category = Some("official".to_string());

        let routed = resolve_codex_model_routed_provider(&router, &json!({ "model": "gpt-5.5" }))
            .expect("official route");
        let materialized = materialize_codex_routed_provider_from_target(&routed, &target);

        assert_eq!(
            materialized
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("codex_oauth")
        );
        assert_eq!(
            adapter.extract_base_url(&materialized).unwrap(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            adapter.extract_auth(&materialized).unwrap().strategy,
            AuthStrategy::CodexOAuth
        );
    }

    #[test]
    fn test_codex_route_target_provider_treats_local_proxy_official_as_managed_oauth() {
        let adapter = CodexAdapter::new();
        let router = create_provider(json!({
            "auth": {
                "auth_mode": "chatgpt",
                "tokens": {
                    "access_token": "router-managed-access-token"
                }
            },
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "router-codex-official",
                    "label": "OpenAI Official",
                    "targetProviderId": "codex-official",
                    "match": { "models": ["gpt-5.5"] },
                    "upstream": {
                        "apiFormat": "openai_responses",
                        "auth": { "source": "managed_codex_oauth" }
                    }
                }]
            }
        }));
        let target = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            json!({
                "auth": {
                    "auth_mode": "chatgpt",
                    "tokens": {
                        "access_token": "stale-access-token"
                    }
                },
                "base_url": "http://127.0.0.1:15721/v1",
                "config": "model_provider = \"codex_model_router_v2\"\n[model_providers.codex_model_router_v2]\nbase_url = \"http://127.0.0.1:15721/v1\"\nwire_api = \"responses\"\n"
            }),
            None,
        );

        let routed = resolve_codex_model_routed_provider(&router, &json!({ "model": "gpt-5.5" }))
            .expect("official route");
        let materialized = materialize_codex_routed_provider_from_target(&routed, &target);

        assert_eq!(
            materialized
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("codex_oauth")
        );
        assert_eq!(
            adapter.extract_base_url(&materialized).unwrap(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            adapter.extract_auth(&materialized).unwrap().strategy,
            AuthStrategy::CodexOAuth
        );
    }

    #[test]
    fn test_codex_route_target_provider_treats_polluted_official_as_managed_oauth() {
        let adapter = CodexAdapter::new();
        let router = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "router-codex-official",
                    "label": "OpenAI Official",
                    "targetProviderId": "codex-official",
                    "match": { "models": ["gpt-5.5"] },
                    "upstream": {
                        "apiFormat": "openai_responses",
                        "auth": { "source": "managed_codex_oauth" }
                    }
                }]
            }
        }));
        let mut target = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official Backup".to_string(),
            json!({
                "base_url": "https://relay.example.com/v1",
                "apiKey": "sk-third-party",
                "auth": {
                    "OPENAI_API_KEY": "sk-third-party"
                },
                "model": "gpt-5.5"
            }),
            None,
        );
        target.category = Some("official".to_string());

        let routed = resolve_codex_model_routed_provider(&router, &json!({ "model": "gpt-5.5" }))
            .expect("official route");
        let materialized = materialize_codex_routed_provider_from_target(&routed, &target);

        assert_eq!(
            materialized
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("codex_oauth")
        );
        assert!(materialized.settings_config.get("base_url").is_none());
        assert!(materialized.settings_config.get("apiKey").is_none());
        assert_eq!(
            adapter.extract_base_url(&materialized).unwrap(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            adapter.extract_auth(&materialized).unwrap().strategy,
            AuthStrategy::CodexOAuth
        );
        assert!(!should_convert_codex_responses_to_chat(
            &materialized,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_codex_model_route_supports_prefix_matching() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {
                    "id": "qwen",
                    "name": "Qwen",
                    "modelPrefixes": ["qwen3."],
                    "base_url": "https://www.matrixminecraft.cn:24443/vllm/v1",
                    "wireApi": "chat",
                    "auth": { "OPENAI_API_KEY": "vllm-local" }
                }
            ]
        }));

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "qwen3.6" }))
            .expect("qwen route");

        assert_eq!(routed.name, "Qwen");
        assert_eq!(routed.settings_config["model"], "qwen3.6");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://www.matrixminecraft.cn:24443/vllm/v1"
        );
    }

    #[test]
    fn test_codex_model_route_overrides_chat_reasoning_config() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {
                    "id": "qwen",
                    "name": "Qwen",
                    "models": ["qwen3.6"],
                    "base_url": "https://www.matrixminecraft.cn:24443/vllm/v1",
                    "wire_api": "chat",
                    "codexChatReasoning": {
                        "supportsThinking": true,
                        "supportsEffort": false,
                        "thinkingParam": "enable_thinking",
                        "effortParam": "none",
                        "minOutputTokens": 2048,
                        "outputFormat": "reasoning_content"
                    }
                }
            ]
        }));

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "qwen3.6" }))
            .expect("qwen route");
        let config = resolve_codex_chat_reasoning_config(&routed, &json!({ "model": "qwen3.6" }))
            .expect("route reasoning config");

        assert_eq!(config.supports_thinking, Some(true));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.effort_param.as_deref(), Some("none"));
        assert_eq!(config.min_output_tokens, Some(QWEN_VLLM_MIN_OUTPUT_TOKENS));
        assert_eq!(config.default_output_tokens, None);
    }

    #[test]
    fn test_codex_model_route_overrides_cache_config() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [{
                    "id": "routing-deepseek",
                    "match": { "models": ["deepseek-chat"] },
                    "upstream": {
                        "apiFormat": "openai_chat",
                        "auth": { "source": "provider_config" }
                    },
                    "capabilities": {
                        "codexCache": {
                            "cacheMode": "deepseek_context_cache",
                            "usageFields": [
                                "usage.prompt_cache_hit_tokens",
                                "usage.prompt_cache_miss_tokens"
                            ]
                        }
                    }
                }],
                "enabled": true
            }
        }));

        let routed =
            resolve_codex_model_routed_provider(&provider, &json!({ "model": "deepseek-chat" }))
                .expect("deepseek route");
        let config = resolve_codex_cache_config(&routed, &json!({ "model": "deepseek-chat" }));

        assert_eq!(config.cache_mode.as_deref(), Some("deepseek_context_cache"));
        assert_ne!(config.supports_prompt_cache_key, Some(true));
        assert_eq!(
            config.usage_fields,
            vec![
                "usage.prompt_cache_hit_tokens".to_string(),
                "usage.prompt_cache_miss_tokens".to_string()
            ]
        );
    }

    #[test]
    fn test_qwen_vllm_route_infers_thinking_without_default_output_budget() {
        let provider = create_provider(json!({
            "modelRoutes": [
                {
                    "id": "qwen",
                    "name": "Qwen",
                    "models": ["qwen3.6"],
                    "base_url": "https://www.matrixminecraft.cn:24443/vllm/v1",
                    "wire_api": "chat"
                }
            ]
        }));

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "qwen3.6" }))
            .expect("qwen route");
        let config = resolve_codex_chat_reasoning_config(&routed, &json!({ "model": "qwen3.6" }))
            .expect("inferred qwen vllm reasoning config");

        assert_eq!(config.supports_thinking, Some(true));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.effort_param.as_deref(), Some("none"));
        assert_eq!(config.min_output_tokens, Some(QWEN_VLLM_MIN_OUTPUT_TOKENS));
        assert_eq!(config.default_output_tokens, None);
    }

    #[test]
    fn test_qwen_vllm_explicit_stale_reasoning_keeps_inferred_defaults() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "qwen_local"
model = "qwen3.6"

[model_providers.qwen_local]
name = "Qwen Local"
base_url = "https://www.matrixminecraft.cn:24443/vllm/v1"
wire_api = "chat"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            codex_chat_reasoning: Some(CodexChatReasoningConfig {
                supports_thinking: Some(true),
                supports_effort: Some(false),
                thinking_param: Some("thinking".to_string()),
                effort_param: Some("none".to_string()),
                effort_value_mode: None,
                min_output_tokens: None,
                default_output_tokens: None,
                output_format: Some("reasoning_content".to_string()),
            }),
            ..Default::default()
        });

        let config = resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "qwen3.6" }))
            .expect("qwen vllm reasoning config");

        assert_eq!(config.supports_thinking, Some(true));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.effort_param.as_deref(), Some("none"));
        assert_eq!(config.min_output_tokens, Some(QWEN_VLLM_MIN_OUTPUT_TOKENS));
        assert_eq!(config.default_output_tokens, None);
    }

    #[test]
    fn test_qwen_vllm_retired_auto_default_budget_is_cleared() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "qwen_local"
model = "qwen3.6"

[model_providers.qwen_local]
name = "Qwen Local"
base_url = "https://www.matrixminecraft.cn:24443/vllm/v1"
wire_api = "chat"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            codex_chat_reasoning: Some(CodexChatReasoningConfig {
                supports_thinking: Some(true),
                supports_effort: Some(false),
                thinking_param: Some("thinking".to_string()),
                effort_param: Some("none".to_string()),
                effort_value_mode: None,
                min_output_tokens: Some(QWEN_VLLM_MIN_OUTPUT_TOKENS),
                default_output_tokens: Some(RETIRED_QWEN_VLLM_DEFAULT_OUTPUT_TOKENS),
                output_format: Some("reasoning_content".to_string()),
            }),
            ..Default::default()
        });

        let config = resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "qwen3.6" }))
            .expect("qwen vllm reasoning config");

        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.min_output_tokens, Some(QWEN_VLLM_MIN_OUTPUT_TOKENS));
        assert_eq!(config.default_output_tokens, None);
    }

    #[test]
    fn test_qwen_vllm_explicit_larger_budget_is_preserved() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "qwen_local"
model = "qwen3.6"

[model_providers.qwen_local]
name = "Qwen Local"
base_url = "https://www.matrixminecraft.cn:24443/vllm/v1"
wire_api = "chat"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            codex_chat_reasoning: Some(CodexChatReasoningConfig {
                supports_thinking: Some(true),
                supports_effort: Some(false),
                thinking_param: Some("enable_thinking".to_string()),
                effort_param: Some("none".to_string()),
                effort_value_mode: None,
                min_output_tokens: Some(4096),
                default_output_tokens: Some(65_536),
                output_format: Some("reasoning_content".to_string()),
            }),
            ..Default::default()
        });

        let config = resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "qwen3.6" }))
            .expect("qwen vllm reasoning config");

        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.min_output_tokens, Some(4096));
        assert_eq!(config.default_output_tokens, Some(65_536));
    }

    #[test]
    fn test_codex_model_route_uses_codex_routing_first() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [{
                    "id": "routing-deepseek",
                    "match": {
                        "models": ["deepseek-v4-flash"]
                    },
                    "label": "DeepSeek Routing",
                    "baseUrl": "https://routing.deepseek.example",
                    "apiFormat": "chat",
                    "upstream": {
                        "modelMap": {
                            "deepseek-v4-flash": "deepseek-upstream-v4-flash"
                        }
                    },
                    "capabilities": {
                        "textOnly": true,
                        "image": {
                            "supported": false
                        }
                    }
                }],
                "enabled": true
            },
            "codexModelRoutes": [{
                "id": "legacy",
                "name": "Legacy DeepSeek",
                "models": ["deepseek-v4-flash"],
                "base_url": "https://legacy.deepseek.example",
                "wire_api": "chat"
            }]
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("routing should resolve");

        assert_eq!(routed.name, "DeepSeek Routing");
        assert_eq!(routed.id, "test::route::routing-deepseek");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://routing.deepseek.example"
        );
        assert_eq!(
            routed.settings_config["model"],
            "deepseek-upstream-v4-flash"
        );
        assert_eq!(routed.settings_config["apiFormat"], "openai_chat");
        assert_eq!(
            codex_provider_text_only_input(&routed),
            Some(true),
            "route-level textOnly should be preserved in routed provider settings"
        );
        assert_eq!(
            routed
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_chat")
        );
    }

    #[test]
    fn test_codex_route_default_route_is_used_when_no_match() {
        let provider = create_provider(json!({
            "codexRouting": {
                "defaultRouteId": "fallback",
                "routes": [
                    {
                        "id": "fallback",
                        "enabled": true,
                        "match": { "prefixes": ["qwen"] },
                        "label": "Qwen Fallback",
                        "base_url": "https://fallback.example"
                    },
                    {
                        "id": "disabled",
                        "enabled": false,
                        "match": { "models": ["does-not-match"] },
                        "base_url": "https://disabled.example"
                    }
                ],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("default fallback route");

        assert_eq!(routed.id, "test::route::fallback");
        assert_eq!(routed.name, "Qwen Fallback");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://fallback.example"
        );
    }

    #[test]
    fn test_codex_model_route_accepts_legacy_array_codex_routing() {
        let provider = create_provider(json!({
            "codexRouting": [
                {
                    "id": "router-codex-official",
                    "label": "OpenAI Official",
                    "providerId": "codex-official",
                    "models": ["gpt-5.5"],
                    "upstream": {
                        "apiFormat": "openai_responses",
                        "auth": { "source": "managed_codex_oauth" }
                    }
                },
                {
                    "id": "router-deepseek",
                    "label": "DeepSeek",
                    "providerId": "codex-deepseek",
                    "modelPrefixes": ["deepseek-"],
                    "upstream": {
                        "apiFormat": "openai_chat",
                        "auth": { "source": "provider_config" }
                    }
                }
            ]
        }));

        let gpt_route =
            resolve_codex_model_routed_provider(&provider, &json!({ "model": "gpt-5.5" }))
                .expect("legacy array gpt route");
        let deepseek_route = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("legacy array deepseek route");

        assert_eq!(gpt_route.id, "test::route::router-codex-official");
        assert_eq!(
            codex_route_target_provider_id(&gpt_route),
            Some("codex-official")
        );
        assert_eq!(deepseek_route.id, "test::route::router-deepseek");
        assert_eq!(
            codex_route_target_provider_id(&deepseek_route),
            Some("codex-deepseek")
        );
    }

    #[test]
    fn test_codex_router_returns_only_the_matched_route() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [
                    {
                        "id": "official",
                        "label": "Official",
                        "match": { "models": ["gpt-5.5"], "prefixes": ["gpt-"] },
                        "upstream": {
                            "baseUrl": "https://chatgpt.com/backend-api/codex",
                            "apiFormat": "openai_responses",
                            "auth": { "source": "managed_codex_oauth" },
                            "modelMap": { "gpt-5.5": "gpt-5.5" }
                        }
                    },
                    {
                        "id": "deepseek",
                        "label": "DeepSeek",
                        "match": { "models": ["deepseek-v4-flash"], "prefixes": ["deepseek-"] },
                        "upstream": {
                            "baseUrl": "https://api.deepseek.com",
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" },
                            "modelMap": { "deepseek-v4-flash": "deepseek-v4-flash" }
                        }
                    }
                ],
                "enabled": true
            }
        }));

        let routed =
            resolve_codex_model_routed_providers(&provider, &json!({ "model": "gpt-5.5" }));

        assert_eq!(routed.len(), 1);
        assert_eq!(routed[0].id, "test::route::official");
        assert_eq!(routed[0].settings_config["model"], "gpt-5.5");
        assert_eq!(routed[0].settings_config["codexResolvedRouteMatched"], true);
    }

    #[test]
    fn test_codex_router_duplicate_exact_routes_remain_order_dependent() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [
                    {
                        "id": "relay",
                        "label": "Relay GPT",
                        "targetProviderId": "relay-provider",
                        "match": { "models": ["gpt-5.5"] },
                        "upstream": {
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" }
                        }
                    },
                    {
                        "id": "official",
                        "label": "OpenAI Official",
                        "targetProviderId": "codex-official",
                        "match": { "models": ["gpt-5.5"] },
                        "upstream": {
                            "apiFormat": "openai_responses",
                            "auth": { "source": "managed_codex_oauth" }
                        }
                    }
                ],
                "enabled": true
            }
        }));

        let routed =
            resolve_codex_model_routed_providers(&provider, &json!({ "model": "gpt-5.5" }));

        assert_eq!(routed.len(), 1);
        assert_eq!(
            routed[0].id, "test::route::relay",
            "相同可见模型名没有额外选择信息，只能按 route 顺序命中第一条；前端保存/同步必须生成唯一别名"
        );
        assert_eq!(routed[0].settings_config["codexResolvedRouteMatched"], true);
    }

    #[test]
    fn test_codex_router_prefers_exact_route_over_earlier_prefix_route() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [
                    {
                        "id": "official",
                        "label": "OpenAI Official",
                        "match": { "models": ["gpt-5.5"], "prefixes": ["gpt-"] },
                        "upstream": {
                            "baseUrl": "https://chatgpt.com/backend-api/codex",
                            "apiFormat": "openai_responses",
                            "auth": { "source": "managed_codex_oauth" }
                        }
                    },
                    {
                        "id": "aggregate",
                        "label": "Aggregate Relay",
                        "match": { "models": ["gpt-5.5-pro"], "prefixes": ["gpt-5.5-pro"] },
                        "upstream": {
                            "baseUrl": "https://relay.example/v1",
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" },
                            "modelMap": { "gpt-5.5-pro": "gpt-5.5-pro" }
                        }
                    }
                ],
                "enabled": true
            }
        }));

        let routed =
            resolve_codex_model_routed_providers(&provider, &json!({ "model": "gpt-5.5-pro" }));

        assert_eq!(routed.len(), 1);
        assert_eq!(routed[0].id, "test::route::aggregate");
        assert_eq!(routed[0].settings_config["codexResolvedRouteMatched"], true);
    }

    #[test]
    fn test_codex_route_resolver_prefers_exact_route_over_earlier_prefix_route() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [
                    {
                        "id": "official",
                        "match": { "models": ["gpt-5.5"], "prefixes": ["gpt-"] },
                        "base_url": "https://chatgpt.com/backend-api/codex"
                    },
                    {
                        "id": "aggregate",
                        "match": { "models": ["gpt-5.5-pro"] },
                        "base_url": "https://relay.example/v1"
                    }
                ],
                "enabled": true
            }
        }));

        let route = resolve_codex_route(&provider, "gpt-5.5-pro").expect("aggregate exact route");

        assert_eq!(
            route.get("id").and_then(|value| value.as_str()),
            Some("aggregate")
        );
    }

    #[test]
    fn test_codex_legacy_router_returns_only_best_route() {
        let provider = create_provider(json!({
            "codexRouting": [
                {
                    "id": "official",
                    "label": "OpenAI Official",
                    "models": ["gpt-5.5"],
                    "modelPrefixes": ["gpt-"],
                    "upstream": {
                        "apiFormat": "openai_responses",
                        "auth": { "source": "managed_codex_oauth" }
                    }
                },
                {
                    "id": "aggregate",
                    "label": "Aggregate Relay",
                    "models": ["gpt-5.5-pro"],
                    "upstream": {
                        "baseUrl": "https://relay.example/v1",
                        "apiFormat": "openai_chat",
                        "auth": { "source": "provider_config" }
                    }
                }
            ]
        }));

        let routed =
            resolve_codex_model_routed_providers(&provider, &json!({ "model": "gpt-5.5-pro" }));

        assert_eq!(routed.len(), 1);
        assert_eq!(routed[0].id, "test::route::aggregate");
    }

    #[test]
    fn test_codex_route_skips_disabled_matches() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [
                    {
                        "id": "disabled",
                        "enabled": false,
                        "match": { "models": ["deepseek-v4-flash"] },
                        "base_url": "https://disabled.example"
                    },
                    {
                        "id": "enabled",
                        "match": { "models": ["deepseek-v4-flash"] },
                        "base_url": "https://enabled.example"
                    }
                ],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("fallback to enabled route");

        assert_eq!(routed.id, "test::route::enabled");
        assert_eq!(
            routed.settings_config["base_url"],
            "https://enabled.example"
        );
    }

    /// 验证旧 catalog 中已停用 route 的 alias 不会静默回退到官方默认 route。
    #[test]
    fn test_codex_disabled_stale_alias_does_not_fall_back_to_official_default() {
        let provider = create_provider(json!({
            "codexRouting": {
                "defaultRouteId": "official",
                "routes": [
                    {
                        "id": "official",
                        "enabled": true,
                        "match": { "models": ["gpt-5.5"] },
                        "upstream": {
                            "apiFormat": "openai_responses",
                            "auth": { "source": "managed_codex_oauth" }
                        }
                    },
                    {
                        "id": "relay",
                        "enabled": false,
                        "match": { "models": ["deepseek-v4-flash-relay"] },
                        "upstream": {
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" }
                        }
                    }
                ],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash-relay" }),
        );

        assert!(
            routed.is_none(),
            "stale alias from a disabled route must fail closed instead of using official quota"
        );
    }

    /// 验证同一 router 在连续请求中始终按当前 body.model 重新选路。
    #[test]
    fn test_codex_same_session_model_switch_re_resolves_route_per_request() {
        let provider = create_provider(json!({
            "codexRouting": {
                "routes": [
                    {
                        "id": "official",
                        "targetProviderId": "codex-official",
                        "match": { "models": ["gpt-5.5"] },
                        "upstream": {
                            "apiFormat": "openai_responses",
                            "auth": { "source": "managed_codex_oauth" }
                        }
                    },
                    {
                        "id": "relay",
                        "targetProviderId": "relay-provider",
                        "match": { "models": ["deepseek-v4-flash-relay"] },
                        "upstream": {
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" },
                            "modelMap": {
                                "deepseek-v4-flash-relay": "deepseek-v4-flash"
                            }
                        }
                    }
                ],
                "enabled": true
            }
        }));

        let official =
            resolve_codex_model_routed_provider(&provider, &json!({ "model": "gpt-5.5" }))
                .expect("official request should resolve");
        let relay = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash-relay" }),
        )
        .expect("relay request should resolve");

        assert_eq!(
            codex_route_target_provider_id(&official),
            Some("codex-official")
        );
        assert_eq!(
            codex_route_target_provider_id(&relay),
            Some("relay-provider")
        );
        assert_eq!(
            relay.settings_config["codexResolvedUpstreamModelOverride"],
            "deepseek-v4-flash"
        );
    }

    #[test]
    fn test_codex_route_managed_codex_oauth_keeps_auth_in_meta() {
        let mut provider = create_provider(json!({
            "codexRouting": {
                "routes": [{
                    "id": "codex_oauth",
                    "label": "ChatGPT OAuth Route",
                    "match": { "models": ["gpt-5.5"] },
                    "auth": {
                        "source": "managed_codex_oauth",
                        "account_id": "acct_123"
                    },
                    "base_url": "https://chatgpt.com/backend-api/codex"
                }],
                "enabled": true
            }
        }));
        provider.meta = Some(ProviderMeta::default());

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "gpt-5.5" }))
            .expect("managed route");

        let meta = routed.meta.as_ref().expect("meta");
        assert_eq!(meta.provider_type.as_deref(), Some("codex_oauth"));
        assert_eq!(
            meta.auth_binding
                .as_ref()
                .and_then(|binding| binding.auth_provider.as_deref()),
            Some("codex_oauth")
        );
        assert!(routed
            .meta
            .as_ref()
            .and_then(|m| m.auth_binding.as_ref())
            .is_some());
        assert!(
            routed.settings_config.get("auth").is_none(),
            "managed auth route should not inline raw auth into settings"
        );
    }

    #[test]
    fn test_codex_route_managed_auth_ignores_stale_api_key() {
        let adapter = CodexAdapter::new();
        let mut provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-provider-key"
            },
            "codexRouting": {
                "routes": [{
                    "id": "codex_oauth",
                    "match": { "models": ["gpt-5.5"] },
                    "upstream": {
                        "baseUrl": "https://chatgpt.com/backend-api/codex",
                        "apiFormat": "responses",
                        "auth": {
                            "source": "managed_codex_oauth",
                            "accountId": "acct_123"
                        },
                        "apiKey": "sk-stale-route-key"
                    }
                }],
                "enabled": true
            }
        }));
        provider.meta = Some(ProviderMeta::default());

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "gpt-5.5" }))
            .expect("managed route");
        let auth = adapter
            .extract_auth(&routed)
            .expect("managed route should use Codex OAuth auth strategy");

        assert_eq!(auth.strategy, AuthStrategy::CodexOAuth);
        assert_ne!(auth.api_key, "sk-stale-route-key");
        assert_eq!(routed.settings_config.get("apiKey"), None);
        assert_eq!(routed.settings_config.get("auth"), None);
    }

    #[test]
    fn test_codex_route_provider_config_auth_preserves_provider_key() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-provider-key"
            },
            "codexRouting": {
                "routes": [{
                    "id": "deepseek",
                    "match": { "models": ["deepseek-v4-flash"] },
                    "upstream": {
                        "baseUrl": "https://api.deepseek.example",
                        "apiFormat": "chat",
                        "auth": { "source": "provider_config" }
                    }
                }],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("provider_config route");
        let auth = adapter
            .extract_auth(&routed)
            .expect("provider auth should remain usable");

        assert_eq!(auth.api_key, "sk-provider-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
        assert_eq!(
            routed.settings_config.get("auth"),
            provider.settings_config.get("auth")
        );
    }

    #[test]
    fn test_codex_route_provider_config_api_key_overrides_provider_key() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-provider-key"
            },
            "codexRouting": {
                "routes": [{
                    "id": "deepseek",
                    "match": { "models": ["deepseek-v4-flash"] },
                    "upstream": {
                        "baseUrl": "https://api.deepseek.example",
                        "apiFormat": "chat",
                        "auth": { "source": "provider_config" },
                        "apiKey": "sk-route-key"
                    }
                }],
                "enabled": true
            }
        }));

        let routed = resolve_codex_model_routed_provider(
            &provider,
            &json!({ "model": "deepseek-v4-flash" }),
        )
        .expect("provider_config route");
        let auth = adapter
            .extract_auth(&routed)
            .expect("route api key should be usable");

        assert_eq!(auth.api_key, "sk-route-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_codex_adapter_supports_routed_codex_oauth_provider() {
        let adapter = CodexAdapter::new();
        let mut provider = create_provider(json!({
            "codexModelRoutes": [
                {
                    "id": "openai",
                    "models": ["gpt-5.5"],
                    "wire_api": "openai_responses",
                    "providerType": "codex_oauth"
                }
            ]
        }));
        provider.meta = Some(ProviderMeta::default());

        let routed = resolve_codex_model_routed_provider(&provider, &json!({ "model": "gpt-5.5" }))
            .expect("openai route");
        let auth = adapter.extract_auth(&routed).expect("codex oauth auth");

        assert_eq!(
            adapter.extract_base_url(&routed).unwrap(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            adapter.build_url(&adapter.extract_base_url(&routed).unwrap(), "/v1/responses"),
            "https://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(auth.strategy, AuthStrategy::CodexOAuth);
        assert!(!should_convert_codex_responses_to_chat(
            &routed,
            "/responses"
        ));
    }

    #[test]
    fn test_codex_adapter_treats_legacy_empty_official_backup_as_managed_oauth() {
        let adapter = CodexAdapter::new();
        let mut provider = Provider::with_id(
            "legacy-official-backup".to_string(),
            "OpenAI Official Backup".to_string(),
            json!({
                "auth": {},
                "config": null
            }),
            None,
        );
        provider.category = Some("official".to_string());

        assert_eq!(
            adapter.extract_base_url(&provider).unwrap(),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            adapter.extract_auth(&provider).unwrap().strategy,
            AuthStrategy::CodexOAuth
        );
    }

    #[test]
    fn test_extract_base_url_direct() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "base_url": "https://api.openai.com/v1"
        }));

        let url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_extract_auth_from_auth_field() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": "sk-test-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-test-key-12345678");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_extract_auth_falls_back_to_config_bearer_when_auth_key_empty() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "auth": {
                "OPENAI_API_KEY": ""
            },
            "config": r#"model_provider = "custom"

[model_providers.custom]
experimental_bearer_token = "sk-config-key"
"#
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-config-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_extract_auth_from_env() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "OPENAI_API_KEY": "sk-env-key-12345678"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-env-key-12345678");
    }

    #[test]
    fn test_extract_base_url_uses_active_model_provider_only() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "config": r#"
model_provider = "openai"

[model_providers.router]
name = "Inactive Router"
base_url = "http://127.0.0.1:15721/v1"

[mcp_servers.local]
base_url = "http://localhost:15722"

[model_providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
wire_api = "responses"
"#
        }));

        let base_url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_extract_base_url_uses_openai_base_url_for_builtin_openai() {
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "config": r#"
model_provider = "openai"
openai_base_url = "http://127.0.0.1:15721/v1"

[model_providers.router]
name = "Inactive Router"
base_url = "http://127.0.0.1:9999/v1"
"#
        }));

        let base_url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(base_url, "http://127.0.0.1:15721/v1");
    }

    #[test]
    fn test_build_url() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://api.openai.com/v1", "/responses");
        assert_eq!(url, "https://api.openai.com/v1/responses");
    }

    // ==================== anthropic upstream detection ====================

    #[test]
    fn test_uses_anthropic_from_settings_api_format() {
        let provider = create_provider(json!({ "apiFormat": "anthropic" }));
        assert!(codex_provider_uses_anthropic(&provider));

        let provider = create_provider(json!({ "api_format": "anthropic_messages" }));
        assert!(codex_provider_uses_anthropic(&provider));
    }

    #[test]
    fn test_uses_anthropic_from_meta_api_format() {
        let mut provider = create_provider(json!({}));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        });
        assert!(codex_provider_uses_anthropic(&provider));
    }

    #[test]
    fn test_uses_anthropic_from_toml_wire_api() {
        let provider = create_provider(json!({
            "config": r#"model_provider = "custom"

[model_providers.custom]
wire_api = "anthropic"
"#
        }));
        assert!(codex_provider_uses_anthropic(&provider));
    }

    #[test]
    fn test_anthropic_false_for_chat_and_responses() {
        let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
        assert!(!codex_provider_uses_anthropic(&chat));
        let responses = create_provider(json!({ "apiFormat": "openai_responses" }));
        assert!(!codex_provider_uses_anthropic(&responses));
    }

    #[test]
    fn test_anthropic_and_chat_are_mutually_exclusive() {
        let anth = create_provider(json!({ "apiFormat": "anthropic" }));
        assert!(codex_provider_uses_anthropic(&anth));
        assert!(!codex_provider_uses_chat_completions(&anth));

        let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
        assert!(codex_provider_uses_chat_completions(&chat));
        assert!(!codex_provider_uses_anthropic(&chat));
    }

    #[test]
    fn test_should_convert_responses_to_anthropic_path_guard() {
        let provider = create_provider(json!({ "apiFormat": "anthropic" }));
        assert!(should_convert_codex_responses_to_anthropic(
            &provider,
            "/responses"
        ));
        assert!(should_convert_codex_responses_to_anthropic(
            &provider,
            "/v1/responses/compact"
        ));
        assert!(should_convert_codex_responses_to_anthropic(
            &provider,
            "/responses?x=1"
        ));
        assert!(!should_convert_codex_responses_to_anthropic(
            &provider,
            "/chat/completions"
        ));
    }

    #[test]
    fn test_resolve_catalog_profile_matches_router() {
        use crate::codex_config::CodexCatalogToolProfile;

        // Anthropic declared only via TOML wire_api (no meta.api_format) must still
        // resolve to the Anthropic catalog profile — this is the routing/catalog
        // divergence that let apply_patch leak through.
        let toml_anthropic = create_provider(json!({
            "config": r#"model_provider = "custom"

[model_providers.custom]
wire_api = "anthropic"
"#
        }));
        assert_eq!(
            resolve_codex_catalog_tool_profile(&toml_anthropic),
            CodexCatalogToolProfile::Anthropic
        );

        // Anthropic via settings apiFormat.
        let settings_anthropic = create_provider(json!({ "apiFormat": "anthropic" }));
        assert_eq!(
            resolve_codex_catalog_tool_profile(&settings_anthropic),
            CodexCatalogToolProfile::Anthropic
        );

        // Native openai_responses (meta) → NativeResponses; chat → ProxyChat.
        let mut native = create_provider(json!({}));
        native.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });
        assert_eq!(
            resolve_codex_catalog_tool_profile(&native),
            CodexCatalogToolProfile::NativeResponses
        );

        let chat = create_provider(json!({ "apiFormat": "openai_chat" }));
        assert_eq!(
            resolve_codex_catalog_tool_profile(&chat),
            CodexCatalogToolProfile::ProxyChat
        );
    }

    #[test]
    fn test_apply_codex_upstream_model_preserves_one_m_catalog_model() {
        // Regression for the [1m] path: a request model carrying the [1m] marker must
        // match its catalog entry and be preserved (not overridden by the provider
        // default) so the transform can later strip [1m] and emit the context-1m beta.
        // This only works because the forwarder no longer strips [1m] before this call
        // on the Anthropic path.
        let provider = create_provider(json!({
            "config": r#"model_provider = "custom"
model = "claude-opus-4-1"

[model_providers.custom]
wire_api = "anthropic"
"#,
            "modelCatalog": {
                "models": [
                    { "model": "claude-opus-4-1[1m]" }
                ]
            }
        }));
        let mut body = json!({ "model": "claude-opus-4-1[1m]", "input": "hi" });
        let result = apply_codex_upstream_model(&provider, &mut body);
        assert_eq!(result.as_deref(), Some("claude-opus-4-1[1m]"));
        assert_eq!(
            body.get("model").and_then(|v| v.as_str()),
            Some("claude-opus-4-1[1m]")
        );
    }

    #[test]
    fn test_anthropic_auth_defaults_to_bearer() {
        // No meta.apiKeyField (defaults to ANTHROPIC_AUTH_TOKEN) → Authorization: Bearer only
        let adapter = CodexAdapter::new();
        let provider = create_provider(json!({
            "apiFormat": "anthropic",
            "auth": { "OPENAI_API_KEY": "sk-anthropic-key-123" }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::Bearer);

        let headers = adapter.get_auth_headers(&auth).unwrap();
        let names: Vec<String> = headers
            .iter()
            .map(|(name, _)| name.as_str().to_string())
            .collect();
        assert_eq!(names, vec!["authorization".to_string()]);
    }

    #[test]
    fn test_anthropic_auth_x_api_key_when_selected() {
        // meta.apiKeyField = ANTHROPIC_API_KEY → x-api-key only
        let adapter = CodexAdapter::new();
        let mut provider = create_provider(json!({
            "apiFormat": "anthropic",
            "auth": { "OPENAI_API_KEY": "sk-anthropic-key-123" }
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("anthropic".to_string()),
            api_key_field: Some("ANTHROPIC_API_KEY".to_string()),
            ..Default::default()
        });

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.strategy, AuthStrategy::Anthropic);

        let headers = adapter.get_auth_headers(&auth).unwrap();
        let names: Vec<String> = headers
            .iter()
            .map(|(name, _)| name.as_str().to_string())
            .collect();
        assert_eq!(names, vec!["x-api-key".to_string()]);
    }

    #[test]
    fn test_build_url_origin_adds_v1() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://api.openai.com", "/responses");
        assert_eq!(url, "https://api.openai.com/v1/responses");
    }

    #[test]
    fn test_build_url_custom_prefix_no_v1() {
        let adapter = CodexAdapter::new();
        let url = adapter.build_url("https://example.com/openai", "/responses");
        assert_eq!(url, "https://example.com/openai/responses");
    }

    #[test]
    fn test_build_url_dedup_v1() {
        let adapter = CodexAdapter::new();
        // base_url 已包含 /v1，endpoint 也包含 /v1
        let url = adapter.build_url("https://www.packyapi.com/v1", "/v1/responses");
        assert_eq!(url, "https://www.packyapi.com/v1/responses");
    }

    #[test]
    fn test_build_url_chatgpt_codex_backend_strips_openai_v1_prefix() {
        let adapter = CodexAdapter::new();

        let url = adapter.build_url("https://chatgpt.com/backend-api/codex", "/v1/responses");
        assert_eq!(url, "https://chatgpt.com/backend-api/codex/responses");

        let compact_url = adapter.build_url(
            "https://chatgpt.com/backend-api/codex",
            "/v1/responses/compact?conversation=1",
        );
        assert_eq!(
            compact_url,
            "https://chatgpt.com/backend-api/codex/responses/compact?conversation=1"
        );
    }

    #[test]
    fn multirouter_native_official_route_uses_codex_current_login_for_compact() {
        let router = Provider::with_id(
            "router".to_string(),
            "Router".to_string(),
            json!({
                "codexRouting": {
                    "enabled": true,
                    "routes": [{
                        "id": "official",
                        "enabled": true,
                        "targetProviderId": "codex-official",
                        "match": { "models": ["gpt-5.6"] },
                        "upstream": {
                            "apiFormat": "openai_responses",
                            "auth": { "source": "native_codex_auth" }
                        }
                    }]
                }
            }),
            None,
        );
        let route = &router.settings_config["codexRouting"]["routes"][0];
        let routed = build_codex_route_probe_provider(&router, route, None);
        let mut official = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        official.category = Some("official".to_string());

        let effective = materialize_codex_routed_provider_from_target(&routed, &official);
        assert!(is_codex_official_provider(&effective));
        assert_eq!(
            effective
                .settings_config
                .get(CODEX_NATIVE_AUTH_PASSTHROUGH)
                .and_then(JsonValue::as_bool),
            Some(true)
        );
        assert_ne!(
            effective
                .meta
                .as_ref()
                .and_then(|meta| meta.provider_type.as_deref()),
            Some("codex_oauth")
        );
        assert_eq!(
            explain_codex_responses_upstream_protocol(&effective).protocol,
            CodexResponsesUpstreamProtocol::Responses
        );
        let adapter = CodexAdapter::new();
        assert_eq!(
            adapter.build_url(
                &adapter
                    .extract_base_url(&effective)
                    .expect("official base URL"),
                "/v1/responses/compact?conversation=1",
            ),
            "https://chatgpt.com/backend-api/codex/responses/compact?conversation=1"
        );
        assert!(
            adapter.extract_auth(&effective).is_none(),
            "native route must not ask the CCSM OAuth manager for credentials"
        );
    }

    #[test]
    fn multirouter_account_pool_route_is_explicit_and_has_no_fixed_auth() {
        let router = Provider::with_id(
            "router".to_string(),
            "Router".to_string(),
            json!({
                "codexRouting": {
                    "enabled": true,
                    "officialAuth": { "mode": "account_pool" },
                    "routes": [{
                        "id": "official",
                        "enabled": true,
                        "targetProviderId": "codex-official",
                        "match": { "models": ["gpt-5.6"] },
                        "upstream": {
                            "apiFormat": "openai_responses",
                            "auth": { "source": "account_pool" }
                        }
                    }]
                }
            }),
            None,
        );
        let route = &router.settings_config["codexRouting"]["routes"][0];
        let routed = build_codex_route_probe_provider(&router, route, None);
        assert_eq!(
            routed.settings_config[CODEX_ACCOUNT_POOL_ENABLED],
            JsonValue::Bool(true)
        );
        assert_ne!(
            routed
                .settings_config
                .get(CODEX_NATIVE_AUTH_PASSTHROUGH)
                .and_then(JsonValue::as_bool),
            Some(true)
        );

        let mut official = Provider::with_id(
            "codex-official".to_string(),
            "OpenAI Official".to_string(),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        official.category = Some("official".to_string());
        let effective = materialize_codex_routed_provider_from_target(&routed, &official);

        assert!(is_codex_official_provider(&effective));
        assert_eq!(
            effective.settings_config[CODEX_ACCOUNT_POOL_ENABLED],
            JsonValue::Bool(true)
        );
        assert!(effective.settings_config.get("auth").is_none());
        assert!(effective
            .meta
            .as_ref()
            .and_then(|meta| meta.auth_binding.as_ref())
            .is_none());
        assert_eq!(
            explain_codex_responses_upstream_protocol(&effective).protocol,
            CodexResponsesUpstreamProtocol::Responses
        );
    }

    // 官方客户端检测测试
    #[test]
    fn test_is_official_client_vscode() {
        assert!(CodexAdapter::is_official_client("codex_vscode/1.0.0"));
        assert!(CodexAdapter::is_official_client("codex_vscode/2.3.4"));
        assert!(CodexAdapter::is_official_client("codex_vscode/0.1"));
    }

    #[test]
    fn test_is_official_client_cli() {
        assert!(CodexAdapter::is_official_client("codex_cli_rs/1.0.0"));
        assert!(CodexAdapter::is_official_client("codex_cli_rs/0.5.2"));
    }

    #[test]
    fn test_is_not_official_client() {
        assert!(!CodexAdapter::is_official_client("Mozilla/5.0"));
        assert!(!CodexAdapter::is_official_client("curl/7.68.0"));
        assert!(!CodexAdapter::is_official_client("python-requests/2.25.1"));
        assert!(!CodexAdapter::is_official_client("codex_other/1.0.0"));
        assert!(!CodexAdapter::is_official_client(""));
    }

    #[test]
    fn test_is_official_client_partial_match() {
        // 必须从开头匹配
        assert!(!CodexAdapter::is_official_client("some codex_vscode/1.0.0"));
        assert!(!CodexAdapter::is_official_client(
            "prefix_codex_cli_rs/1.0.0"
        ));
    }

    #[test]
    fn test_codex_provider_uses_chat_completions_from_active_wire_api() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "chat_only"
model = "gpt-5"

[model_providers.chat_only]
name = "Chat Only"
base_url = "https://example.com/v1"
wire_api = "chat"
"#
        }));

        assert!(codex_provider_uses_chat_completions(&provider));
        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/responses?stream=true"
        ));
        assert!(!should_convert_codex_responses_to_chat(
            &provider,
            "/chat/completions"
        ));
    }

    #[test]
    fn test_managed_codex_oauth_stays_on_native_responses() {
        let mut provider = create_provider(json!({
            "auth": {
                "auth_mode": "chatgpt"
            }
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("codex_oauth".to_string()),
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });

        let decision = explain_codex_responses_upstream_protocol(&provider);

        assert_eq!(decision.protocol, CodexResponsesUpstreamProtocol::Responses);
        assert_eq!(decision.source, "managed_codex_oauth");
        assert!(!should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses"
        ));
        assert!(!should_convert_codex_responses_to_messages(
            &provider,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_codex_provider_uses_chat_completions_for_legacy_deepseek_responses_wire_api() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
"#
        }));

        assert!(codex_provider_uses_chat_completions(&provider));
        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_codex_provider_keeps_openai_responses_wire_api() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "openai"
model = "gpt-5.4-mini"

[model_providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
wire_api = "responses"
"#
        }));

        assert!(!codex_provider_uses_chat_completions(&provider));
        assert!(!should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_codex_provider_uses_chat_completions_from_full_chat_url() {
        let provider = create_provider(json!({
            "base_url": "https://example.com/v1/chat/completions"
        }));

        assert!(codex_provider_uses_chat_completions(&provider));
        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses"
        ));
        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses/compact"
        ));
    }

    /// 验证 sense nova ShangTang 上游被识别为 Chat Completions-only。
    ///
    /// ShangTang 的 token.sensenova.cn 只有 Chat Completions 端点，
    /// 没有可用的 Responses 实现。如果协议检测返回 Responses（native），
    /// Codex 的 Response API 请求会直接发给 Chat-only 端点，导致
    /// HTTP 400 invalid_tool_call_id（Issue #12 / upstream #4973）。
    /// 检查 should_convert_codex_responses_to_chat 必须返回 true。
    #[test]
    fn test_codex_provider_uses_chat_completions_for_sensenova_url() {
        let provider = create_provider(json!({
            "base_url": "https://token.sensenova.cn/v1"
        }));

        assert!(
            codex_provider_uses_chat_completions(&provider),
            "ShangTang SenseNova (sensenova.cn) must be detected as Chat Completions-only"
        );
        assert!(
            should_convert_codex_responses_to_chat(&provider, "/v1/responses"),
            "Responses for ShangTang must be converted to Chat only"
        );
    }

    #[test]
    fn test_codex_provider_converts_remote_compact_for_chat_route() {
        let mut provider = create_provider(json!({
            "base_url": "https://example.com/v1"
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });

        assert!(codex_provider_uses_chat_completions(&provider));
        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/responses/compact?stream=true"
        ));
        assert!(!should_convert_codex_responses_to_messages(
            &provider,
            "/responses/compact?stream=true"
        ));
    }

    #[test]
    fn codex_route_supports_responses_compaction_uses_route_capability_and_official_id() {
        let third_party = create_provider(json!({
            "codexResolvedRouteId": "router-deepseek",
            "codexResolvedCapabilities": {}
        }));
        assert!(!codex_route_supports_responses_compaction(&third_party));

        let opted_in = create_provider(json!({
            "codexResolvedRouteId": "router-deepseek",
            "codexResolvedCapabilities": {
                "supportsRemoteCompaction": true
            }
        }));
        assert!(codex_route_supports_responses_compaction(&opted_in));

        let official_route = create_provider(json!({
            "codexResolvedRouteId": "router-codex-official",
            "codexResolvedCapabilities": {}
        }));
        assert!(codex_route_supports_responses_compaction(&official_route));
    }

    #[test]
    fn codex_provider_remote_compaction_enabled_defaults_false_for_third_party() {
        let provider = create_provider(json!({
            "config": r#"model_provider = "custom"

[model_providers.custom]
name = "DeepSeek"
"#
        }));

        assert!(!codex_provider_remote_compaction_enabled(&provider));
    }

    #[test]
    fn codex_provider_remote_compaction_enabled_honors_toml_and_structured_opt_in() {
        let toml_enabled = create_provider(json!({
            "config": r#"model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
"#
        }));
        assert!(codex_provider_remote_compaction_enabled(&toml_enabled));

        let structured_enabled = create_provider(json!({
            "codexRemoteCompaction": true
        }));
        assert!(codex_provider_remote_compaction_enabled(
            &structured_enabled
        ));
    }

    #[test]
    fn codex_provider_remote_compaction_enabled_tracks_route_backend() {
        let official_only = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "official",
                    "upstream": {
                        "auth": { "source": "managed_codex_oauth" }
                    }
                }]
            }
        }));
        assert!(codex_provider_remote_compaction_enabled(&official_only));

        let third_party_only = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "deepseek",
                    "upstream": {
                        "auth": { "source": "provider_config" }
                    }
                }]
            }
        }));
        assert!(!codex_provider_remote_compaction_enabled(&third_party_only));

        let mixed = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "official",
                        "upstream": {
                            "auth": { "source": "managed_codex_oauth" }
                        }
                    },
                    {
                        "id": "deepseek",
                        "upstream": {
                            "auth": { "source": "provider_config" }
                        }
                    }
                ]
            }
        }));
        assert!(!codex_provider_remote_compaction_enabled(&mixed));
    }

    #[test]
    fn test_codex_provider_uses_chat_completions_from_meta_api_format_for_responses() {
        let mut provider = create_provider(json!({
            "base_url": "https://api.deepseek.com/v1"
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });

        assert!(should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_codex_provider_uses_messages_from_explicit_api_format() {
        let provider = create_provider(json!({
            "apiFormat": "openai_messages",
            "base_url": "https://api.anthropic-gateway.local/v1"
        }));

        let decision = explain_codex_responses_upstream_protocol(&provider);

        assert_eq!(decision.protocol, CodexResponsesUpstreamProtocol::Messages);
        assert_eq!(decision.source, "settings_api_format");
        assert!(should_convert_codex_responses_to_messages(
            &provider,
            "/v1/responses"
        ));
        assert!(!should_convert_codex_responses_to_chat(
            &provider,
            "/v1/responses"
        ));
    }

    #[test]
    fn test_apply_codex_chat_upstream_model_uses_provider_config_model() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        let mut body = json!({
            "model": "placeholder-client-model",
            "input": "ping"
        });

        let upstream_model = apply_codex_chat_upstream_model(&provider, &mut body);

        assert_eq!(upstream_model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(
            body.get("model").and_then(|v| v.as_str()),
            Some("deepseek-v4-flash")
        );
    }

    #[test]
    fn test_apply_codex_chat_upstream_model_preserves_catalog_model_selection() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#,
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-flash" },
                    { "model": "kimi-k2" }
                ]
            }
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        let mut body = json!({
            "model": "kimi-k2",
            "input": "ping"
        });

        let upstream_model = apply_codex_chat_upstream_model(&provider, &mut body);

        assert_eq!(upstream_model.as_deref(), Some("kimi-k2"));
        assert_eq!(body.get("model").and_then(|v| v.as_str()), Some("kimi-k2"));
    }

    #[test]
    fn test_apply_codex_chat_upstream_model_uses_catalog_upstream_model() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "thirdparty"
model = "gpt-5.5-thirdparty"

[model_providers.thirdparty]
name = "Third-party GPT"
base_url = "https://api.thirdparty.example/v1"
wire_api = "responses"
"#,
            "modelCatalog": {
                "models": [
                    {
                        "model": "gpt-5.5-thirdparty",
                        "upstreamModel": "gpt-5.5"
                    }
                ]
            }
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        let mut body = json!({
            "model": "gpt-5.5-thirdparty",
            "input": "ping"
        });

        let upstream_model = apply_codex_chat_upstream_model(&provider, &mut body);

        assert_eq!(upstream_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(body.get("model").and_then(|v| v.as_str()), Some("gpt-5.5"));
    }

    #[test]
    fn test_apply_codex_request_upstream_model_uses_catalog_for_native_responses() {
        let provider = create_provider(json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "gpt-5.5-thirdparty",
                        "upstream_model": "gpt-5.5"
                    }
                ]
            }
        }));
        let mut body = json!({
            "model": "gpt-5.5-thirdparty",
            "input": "ping"
        });

        let upstream_model = apply_codex_request_upstream_model(&provider, &mut body);

        assert_eq!(upstream_model.as_deref(), Some("gpt-5.5"));
        assert_eq!(body.get("model").and_then(|v| v.as_str()), Some("gpt-5.5"));
    }

    #[test]
    fn test_apply_codex_request_upstream_model_route_override_takes_priority() {
        let mut settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "gpt-5.5-thirdparty",
                        "upstreamModel": "gpt-5.5"
                    }
                ]
            }
        });
        settings.as_object_mut().unwrap().insert(
            CODEX_RESOLVED_UPSTREAM_MODEL_OVERRIDE.to_string(),
            json!("route-overridden-model"),
        );
        let provider = create_provider(settings);
        let mut body = json!({
            "model": "gpt-5.5-thirdparty",
            "input": "ping"
        });

        let upstream_model = apply_codex_request_upstream_model(&provider, &mut body);

        assert_eq!(upstream_model.as_deref(), Some("route-overridden-model"));
        assert_eq!(
            body.get("model").and_then(|v| v.as_str()),
            Some("route-overridden-model")
        );
    }

    #[test]
    fn test_apply_codex_chat_upstream_model_forces_unmatched_fallback_route_model() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-flash"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com/v1"
wire_api = "responses"
"#,
            "modelCatalog": {
                "models": [
                    { "model": "gpt-5.5" },
                    { "model": "deepseek-v4-flash" }
                ]
            },
            "codexResolvedRouteMatched": false
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            api_format: Some("openai_chat".to_string()),
            ..Default::default()
        });
        let mut body = json!({
            "model": "gpt-5.5",
            "input": "ping"
        });

        let upstream_model = apply_codex_chat_upstream_model(&provider, &mut body);

        assert_eq!(upstream_model.as_deref(), Some("deepseek-v4-flash"));
        assert_eq!(
            body.get("model").and_then(|v| v.as_str()),
            Some("deepseek-v4-flash")
        );
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_infers_deepseek_effort_support() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-pro"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "chat"
"#
        }));

        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-pro" }))
                .unwrap();

        assert_eq!(config.supports_thinking, Some(true));
        assert_eq!(config.supports_effort, Some(true));
        assert_eq!(config.effort_value_mode.as_deref(), Some("deepseek"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_infers_glm_5_2_effort_support() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "zhipu_glm"
model = "glm-5.2"

[model_providers.zhipu_glm]
name = "Zhipu GLM"
base_url = "https://open.bigmodel.cn/api/coding/paas/v4"
wire_api = "chat"
"#
        }));

        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "glm-5.2" })).unwrap();

        assert_eq!(config.supports_thinking, Some(true));
        assert_eq!(config.thinking_param.as_deref(), Some("thinking"));
        assert_eq!(config.supports_effort, Some(true));
        assert_eq!(config.effort_param.as_deref(), Some("reasoning_effort"));
        assert_eq!(config.effort_value_mode.as_deref(), Some("deepseek"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_explicit_meta_overrides_inference() {
        let mut provider = create_provider(json!({
            "config": r#"
model_provider = "deepseek"
model = "deepseek-v4-pro"

[model_providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "chat"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            codex_chat_reasoning: Some(CodexChatReasoningConfig {
                supports_thinking: Some(false),
                supports_effort: Some(false),
                thinking_param: Some("none".to_string()),
                effort_param: Some("none".to_string()),
                effort_value_mode: None,
                min_output_tokens: None,
                default_output_tokens: None,
                output_format: Some("auto".to_string()),
            }),
            ..Default::default()
        });

        let config =
            resolve_codex_chat_reasoning_config(&provider, &json!({ "model": "deepseek-v4-pro" }))
                .unwrap();

        assert_eq!(config.supports_thinking, Some(false));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.thinking_param.as_deref(), Some("none"));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_openrouter_platform_overrides_model() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "openrouter"
model = "deepseek/deepseek-chat-v3.1"

[model_providers.openrouter]
name = "OpenRouter"
base_url = "https://openrouter.ai/api/v1"
wire_api = "chat"
"#
        }));

        // 模型名含 "deepseek"，但平台是 OpenRouter —— 平台规则必须覆盖模型规则。
        let config = resolve_codex_chat_reasoning_config(
            &provider,
            &json!({ "model": "deepseek/deepseek-chat-v3.1" }),
        )
        .unwrap();

        assert_eq!(config.thinking_param.as_deref(), Some("none"));
        assert_eq!(config.effort_param.as_deref(), Some("reasoning.effort"));
        assert_eq!(config.effort_value_mode.as_deref(), Some("openrouter"));
        assert_eq!(config.supports_effort, Some(true));
    }

    #[test]
    fn test_resolve_codex_chat_reasoning_siliconflow_platform_overrides_minimax() {
        let provider = create_provider(json!({
            "config": r#"
model_provider = "siliconflow"
model = "MiniMaxAI/MiniMax-M2.7"

[model_providers.siliconflow]
name = "SiliconFlow"
base_url = "https://api.siliconflow.cn/v1"
wire_api = "chat"
"#
        }));

        // 模型是 MiniMax（官方用 reasoning_split），但平台是 SiliconFlow —— 应走平台的 enable_thinking。
        let config = resolve_codex_chat_reasoning_config(
            &provider,
            &json!({ "model": "MiniMaxAI/MiniMax-M2.7" }),
        )
        .unwrap();

        assert_eq!(config.thinking_param.as_deref(), Some("enable_thinking"));
        assert_eq!(config.supports_effort, Some(false));
        assert_eq!(config.output_format.as_deref(), Some("reasoning_content"));
    }
    /// 验证 MultiRouter 的 modelCatalog 在路由物料化后仍可访问。
    ///
    /// 场景：两个 provider 暴露同名上游模型 "deepseek-v4-flash"，但分别使用不同可见名
    /// "deepseek-v4-flash" 和 "deepseek-v4-flash-provider-b"。route 不设 modelMap，
    /// 依赖 catalog 查找做 visible_name → upstream_model 映射。
    ///
    /// 验证点：
    /// - 物料化后 materialized provider 保留 modelCatalog（回归 #fix: materialize丢失catalog）
    /// - catalog 中两个不同可见名的条目都保留（不被 seen Set 去重）
    /// - apply_codex_request_upstream_model 能通过 catalog 把可见名映射回上游模型名
    #[test]
    fn test_materialize_routed_provider_preserves_model_catalog() {
        let router = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "route-a",
                        "label": "Provider A",
                        "targetProviderId": "provider-a",
                        "match": { "models": ["deepseek-v4-flash"] },
                        "upstream": {
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" }
                        }
                    },
                    {
                        "id": "route-b",
                        "label": "Provider B",
                        "targetProviderId": "provider-b",
                        "match": { "models": ["deepseek-v4-flash-provider-b"] },
                        "upstream": {
                            "apiFormat": "openai_chat",
                            "auth": { "source": "provider_config" }
                        }
                    }
                ]
            },
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-v4-flash", "upstreamModel": "deepseek-v4-flash" },
                    { "model": "deepseek-v4-flash-provider-b", "upstreamModel": "deepseek-v4-flash" }
                ]
            }
        }));

        // 目标 provider B：有自己的模型配置，但没有 modelCatalog
        let target_b = Provider::with_id(
            "provider-b".to_string(),
            "Provider B".to_string(),
            json!({
                "base_url": "https://api.provider-b.example",
                "auth": { "OPENAI_API_KEY": "sk-test" },
                "model": "deepseek-v4-flash"
            }),
            None,
        );

        // 路线 B 匹配可见名 "deepseek-v4-flash-provider-b"
        let routed = resolve_codex_model_routed_provider(
            &router,
            &json!({ "model": "deepseek-v4-flash-provider-b" }),
        )
        .expect("route-b should match");

        assert_eq!(codex_route_target_provider_id(&routed), Some("provider-b"));

        // 【关键验证】物料化前，route provider 仍有 modelCatalog
        let catalog_before = routed
            .settings_config
            .get("modelCatalog")
            .and_then(|c| c.get("models"))
            .and_then(|m| m.as_array());
        assert!(
            catalog_before.is_some(),
            "route provider must have modelCatalog"
        );

        // 【关键验证】物料化后，materialized provider 保留 modelCatalog
        let materialized = materialize_codex_routed_provider_from_target(&routed, &target_b);
        let catalog_after = materialized
            .settings_config
            .get("modelCatalog")
            .and_then(|c| c.get("models"))
            .and_then(|m| m.as_array());
        assert!(
            catalog_after.is_some(),
            "materialized provider must preserve modelCatalog from route (fix: materialize丢失catalog)"
        );

        // 验证 catalog 中有两个条目（不同可见名不被去重）
        let models = catalog_after.unwrap();
        assert_eq!(models.len(), 2, "both aliased models must survive");

        // 验证 apply_codex_request_upstream_model 能通过 catalog 映射可见名→上游模型名
        let mut body_a = json!({ "model": "deepseek-v4-flash", "input": "test" });
        let result_a = apply_codex_request_upstream_model(&materialized, &mut body_a);
        assert_eq!(
            result_a.as_deref(),
            Some("deepseek-v4-flash"),
            "visible name 'deepseek-v4-flash' should map to upstream 'deepseek-v4-flash'"
        );

        let mut body_b = json!({ "model": "deepseek-v4-flash-provider-b", "input": "test" });
        let result_b = apply_codex_request_upstream_model(&materialized, &mut body_b);
        assert_eq!(result_b.as_deref(), Some("deepseek-v4-flash"),
            "aliased visible name 'deepseek-v4-flash-provider-b' should map to upstream 'deepseek-v4-flash'");
    }

    /// 验证 materialize_codex_routed_provider_from_target 保留 route 的 apiFormat。
    ///
    /// 当 route 声明 upstream.apiFormat = "openai_chat"，build_codex_routed_provider
    /// 会将其写入 settings_config.apiFormat。但 materialize_codex_routed_provider_from_target
    /// 以 target provider 的 settings 为基底，如果不显式复制 apiFormat 字段，
    /// explain_codex_responses_upstream_protocol 就无法识别 Chat 协议，
    /// 导致 Responses API 请求被原生透传给 Chat-only 上游（Issue #12 / upstream #4973）。
    /// 回归确认：materialize 后 should_convert_codex_responses_to_chat 必须返回 true。
    #[test]
    fn test_materialize_routed_provider_preserves_api_format() {
        let router = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [{
                    "id": "shangtang",
                    "label": "ShangTang",
                    "targetProviderId": "shangtang-provider",
                    "match": { "models": ["deepseek-v4-flash-shangtang"] },
                    "upstream": {
                        "baseUrl": "https://token.sensenova.cn/v1",
                        "apiFormat": "openai_chat",
                        "auth": { "source": "provider_config" }
                    }
                }]
            }
        }));

        let mut target = Provider::with_id(
            "shangtang-provider".to_string(),
            "ShangTang SenseNova".to_string(),
            json!({
                "base_url": "https://token.sensenova.cn/v1",
                "auth": { "OPENAI_API_KEY": "sk-test" }
            }),
            None,
        );
        target.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            ..Default::default()
        });

        // 构建 routed provider 再 materialize
        let routed = resolve_codex_model_routed_provider(
            &router,
            &json!({ "model": "deepseek-v4-flash-shangtang" }),
        )
        .expect("route should match ShangTang");

        assert_eq!(
            codex_route_target_provider_id(&routed),
            Some("shangtang-provider")
        );

        // 验证 materialize 前 route provider 有 apiFormat
        let route_api_format = routed
            .settings_config
            .get("apiFormat")
            .and_then(|v| v.as_str());
        assert_eq!(
            route_api_format,
            Some("openai_chat"),
            "route provider must preserve apiFormat before materialization"
        );

        // materialize
        let materialized = materialize_codex_routed_provider_from_target(&routed, &target);

        // 验证 materialize 后 apiFormat 被保留
        let mat_api_format = materialized
            .settings_config
            .get("apiFormat")
            .and_then(|v| v.as_str());
        assert_eq!(
            mat_api_format,
            Some("openai_chat"),
            "materialized provider must preserve apiFormat from route (fix: materialize丢失apiFormat)"
        );
        assert_eq!(
            materialized
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_chat"),
            "route apiFormat must override stale target provider metadata"
        );

        // 验证 materialize 后的协议检测正确
        assert!(
            should_convert_codex_responses_to_chat(&materialized, "/v1/responses"),
            "materialized ShangTang provider must be detected as Chat Completions"
        );
    }

    #[test]
    fn compact_request_routes_by_its_own_model_after_context_switch() {
        let router = create_provider(json!({
            "codexRouting": {
                "enabled": true,
                "routes": [
                    {
                        "id": "official",
                        "match": {"models": ["gpt-5.6-sol"]},
                        "upstream": {
                            "baseUrl": "https://chatgpt.com/backend-api/codex",
                            "apiFormat": "openai_responses",
                            "model": "gpt-5.6-sol"
                        }
                    },
                    {
                        "id": "qwen",
                        "match": {"models": ["qwen3.6"]},
                        "upstream": {
                            "baseUrl": "https://example.test/v1",
                            "apiFormat": "openai_chat",
                            "model": "qwen3.6"
                        }
                    }
                ]
            }
        }));

        let previous_model_compact = resolve_codex_model_routed_provider(
            &router,
            &json!({"model":"gpt-5.6-sol","input":[]}),
        )
        .expect("official compact route");
        let current_model_compact =
            resolve_codex_model_routed_provider(&router, &json!({"model":"qwen3.6","input":[]}))
                .expect("qwen compact route");

        assert!(!should_convert_codex_responses_to_chat(
            &previous_model_compact,
            "/v1/responses/compact"
        ));
        assert!(should_convert_codex_responses_to_chat(
            &current_model_compact,
            "/v1/responses/compact"
        ));
        assert_eq!(
            previous_model_compact
                .settings_config
                .get("codexResolvedRouteId")
                .and_then(JsonValue::as_str),
            Some("official")
        );
        assert_eq!(
            current_model_compact
                .settings_config
                .get("codexResolvedRouteId")
                .and_then(JsonValue::as_str),
            Some("qwen")
        );
    }

    #[test]
    fn xai_oauth_invariants_ignore_editable_base_url_and_auth() {
        let adapter = CodexAdapter::new();
        let mut provider = create_provider(json!({
            "auth": { "OPENAI_API_KEY": "user-edited" },
            "config": r#"
model = "grok-4.5"

[model_providers.custom]
name = "xai"
base_url = "https://attacker.example/v1"
wire_api = "responses"
"#
        }));
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            ..Default::default()
        });

        // 可编辑字段（base_url / auth key）不得影响托管路由：
        // 端点硬定向 api.x.ai，凭据是占位符（真 token 由 forwarder 注入）。
        assert_eq!(
            adapter.extract_base_url(&provider).unwrap(),
            super::super::XAI_API_BASE_URL
        );
        let auth = adapter
            .extract_auth(&provider)
            .expect("managed auth placeholder");
        assert_eq!(auth.api_key, "xai_oauth_placeholder");
        assert_eq!(auth.strategy, AuthStrategy::XaiOAuth);
    }

    #[test]
    fn xai_oauth_pins_native_responses_catalog_profile() {
        let mut provider = create_provider(json!({ "auth": {}, "config": "" }));
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            // 即使 api_format 被改成 anthropic，catalog 画像也必须钉死原生 Responses
            api_format: Some("anthropic".to_string()),
            ..Default::default()
        });

        assert!(matches!(
            resolve_codex_catalog_tool_profile(&provider),
            crate::codex_config::CodexCatalogToolProfile::NativeResponses
        ));
    }

    #[test]
    fn namespace_flatten_gate_only_fires_for_xai_oauth() {
        // xAI OAuth: strict native gateway → needs namespace flattening.
        let mut xai = create_provider(json!({ "auth": {}, "config": "" }));
        xai.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("xai_oauth".to_string()),
            ..Default::default()
        });
        assert!(provider_needs_responses_namespace_flatten(&xai));

        // A plain third-party API-key Codex provider must not be flattened.
        let plain = create_provider(json!({
            "auth": { "OPENAI_API_KEY": "sk-x" },
            "config": "base_url = \"https://api.x.ai/v1\"\nwire_api = \"responses\""
        }));
        assert!(!provider_needs_responses_namespace_flatten(&plain));
    }
}
