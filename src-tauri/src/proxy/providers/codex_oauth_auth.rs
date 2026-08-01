//! Codex OAuth Authentication Module
//!
//! 实现 OpenAI ChatGPT Plus/Pro 订阅的 OAuth Device Code 流程。
//! 支持多账号管理，每个 Provider 可关联不同的 ChatGPT 账号。
//!
//! ## 认证流程
//! 1. 启动 Device Code 流程，获取 device_auth_id 和 user_code
//! 2. 用户在浏览器中完成 ChatGPT 授权
//! 3. 轮询获取 authorization_code 和 code_verifier（注意：verifier 由服务端返回）
//! 4. 使用 code + verifier 换取 access_token + refresh_token + id_token
//! 5. 自动刷新 access_token（到期前 60 秒）
//!
//! ## 多账号支持
//! - 每个 ChatGPT 账号独立存储 refresh_token
//! - Provider 通过 meta.authBinding 关联账号（auth_provider = "codex_oauth"）
//! - 通过 JWT id_token 提取 chatgpt_account_id 作为账号唯一标识

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use super::codex_oauth_pool::{CodexPoolAttemptOutcome, CodexPoolRuntimeState};
use super::copilot_auth::{GitHubAccount, GitHubDeviceCodeResponse};

/// OpenAI OAuth 客户端 ID（OpenCode 使用，与官方 Codex CLI 相同）
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

/// Device Code 启动 URL
const DEVICE_AUTH_USERCODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";

/// Device Code 轮询 URL
const DEVICE_AUTH_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";

/// OAuth Token URL（用于 code 换 token 和 refresh token）
const OAUTH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";

/// Device Code 验证 URL（向用户展示）
const DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";

/// Device Code 流程的 redirect_uri（OpenAI 服务端约定）
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";

/// Token 刷新提前量（毫秒）
const TOKEN_REFRESH_BUFFER_MS: i64 = 60_000;

/// Device Code 默认有效时长（秒），OpenAI 文档约定 15 分钟
const DEVICE_CODE_DEFAULT_EXPIRES_IN: u64 = 900;

/// 轮询间隔安全余量（秒）
const POLLING_SAFETY_MARGIN_SECS: u64 = 3;

/// User-Agent
const CODEX_USER_AGENT: &str = "cc-switch-codex-oauth";

/// Codex OAuth 错误
#[derive(Debug, thiserror::Error)]
pub enum CodexOAuthError {
    #[error("等待用户授权中")]
    AuthorizationPending,

    #[error("用户拒绝授权")]
    AccessDenied,

    #[error("Device Code 已过期")]
    ExpiredToken,

    #[error("OAuth Token 获取失败: {0}")]
    TokenFetchFailed(String),

    #[error("Refresh Token 失效或已过期")]
    RefreshTokenInvalid,

    #[error("网络错误: {0}")]
    NetworkError(String),

    #[error("解析错误: {0}")]
    ParseError(String),

    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("账号不存在: {0}")]
    AccountNotFound(String),
}

impl From<reqwest::Error> for CodexOAuthError {
    fn from(err: reqwest::Error) -> Self {
        CodexOAuthError::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for CodexOAuthError {
    fn from(err: std::io::Error) -> Self {
        CodexOAuthError::IoError(err.to_string())
    }
}

/// OpenAI Device Code 响应
#[derive(Debug, Clone, Deserialize)]
struct DeviceCodeResponse {
    device_auth_id: String,
    user_code: String,
    #[serde(default)]
    interval: Option<serde_json::Value>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// OpenAI Device Code 轮询响应（成功）
#[derive(Debug, Clone, Deserialize)]
struct DevicePollSuccess {
    authorization_code: String,
    code_verifier: String,
}

/// OAuth Token 响应
#[derive(Debug, Clone, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

/// 解析后的 JWT claims（仅关心 chatgpt_account_id 等字段）
#[derive(Debug, Clone, Default, Deserialize)]
struct IdTokenClaims {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    organizations: Vec<OrgClaim>,
    #[serde(default, rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAiAuthClaim>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct OrgClaim {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct OpenAiAuthClaim {
    #[serde(default)]
    chatgpt_account_id: Option<String>,
}

/// 缓存的 access_token（含过期时间）
#[derive(Debug, Clone)]
struct CachedAccessToken {
    token: String,
    /// 过期时间戳（毫秒）
    expires_at_ms: i64,
}

impl CachedAccessToken {
    fn is_expiring_soon(&self) -> bool {
        let now = chrono::Utc::now().timestamp_millis();
        self.expires_at_ms - now < TOKEN_REFRESH_BUFFER_MS
    }
}

/// 进行中的 Device Code 条目，带过期时间以便清理放弃的登录流程
#[derive(Debug, Clone)]
struct PendingDeviceCode {
    user_code: String,
    /// Unix 毫秒时间戳，超时后可清理
    expires_at_ms: i64,
}

#[derive(Debug, Default)]
struct NativeCredentialRuntime {
    authorization_digest: Option<[u8; 32]>,
    credential_generation: u64,
}

/// 持久化的账号数据
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexAccountData {
    /// chatgpt_account_id（同时作为 HashMap 的 key）
    pub account_id: String,
    /// 账号邮箱（如果可获取）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Refresh Token（持久化）
    pub refresh_token: String,
    /// 最近一次仍有效的短期 access token。
    ///
    /// refresh token 已经保存在同一凭据文件中；一并保存短期 token 可以避免
    /// CCSM 每次重启都立刻消耗 refresh token，并降低轮换竞争概率。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token_expires_at_ms: Option<i64>,
    /// 明确收到 OAuth `invalid_grant` 后保留账号记录，但不再把它当作可用认证。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_error: Option<String>,
    /// 用户重新建立同一账号身份时递增；普通 token 刷新不改变。
    #[serde(default)]
    pub credential_generation: u64,
    /// 认证时间戳（秒）
    pub authenticated_at: i64,
}

impl CodexAccountData {
    fn is_usable(&self) -> bool {
        self.invalidated_at.is_none()
    }
}

/// 公开的账号信息（返回给前端，复用 GitHubAccount 结构）
impl From<&CodexAccountData> for GitHubAccount {
    fn from(data: &CodexAccountData) -> Self {
        GitHubAccount {
            id: data.account_id.clone(),
            // 用 email 作为显示名（若无则用 account_id）
            login: data
                .email
                .clone()
                .unwrap_or_else(|| format!("ChatGPT ({})", &data.account_id)),
            avatar_url: None,
            authenticated_at: data.authenticated_at,
            github_domain: "github.com".to_string(),
        }
    }
}

/// 持久化存储结构（v1）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CodexOAuthStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, CodexAccountData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
    #[serde(default)]
    pool_policy: CodexAccountPoolPolicy,
}

pub const NATIVE_CODEX_ACCOUNT_ID: &str = "native_codex_auth";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountPoolEntry {
    pub account_id: String,
    pub enabled: bool,
    pub reserve_percent: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CodexPoolCandidate {
    pub(crate) entry: CodexAccountPoolEntry,
    pub(crate) credential_generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountPoolPolicy {
    pub enabled: bool,
    pub entries: Vec<CodexAccountPoolEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_account_id: Option<String>,
}

impl Default for CodexAccountPoolPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            entries: vec![CodexAccountPoolEntry {
                account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                enabled: true,
                reserve_percent: 5.0,
            }],
            desktop_account_id: None,
        }
    }
}

/// 当 Desktop 当前登录与某个 CCSM OAuth 托管账号相同时，合并重复的池条目。
///
/// Desktop 和托管 OAuth 是两套凭据存储，但底层 ChatGPT account id 相同；如果
/// Desktop 条目启用，账号池不应把同一个账号拆成两个候选，否则会出现重复额度、
/// 重复会话绑定和同账号内空转切换。
fn apply_desktop_account_dedupe(
    policy: &mut CodexAccountPoolPolicy,
    managed_account_ids: &std::collections::HashSet<String>,
    desktop_account_id: Option<&str>,
) {
    let Some(desktop_account_id) = desktop_account_id
        .map(str::trim)
        .filter(|account_id| !account_id.is_empty())
    else {
        return;
    };
    if !managed_account_ids.contains(desktop_account_id) {
        return;
    }
    let native_enabled = policy
        .entries
        .iter()
        .any(|entry| entry.account_id == NATIVE_CODEX_ACCOUNT_ID && entry.enabled);
    if native_enabled {
        policy
            .entries
            .retain(|entry| entry.account_id != desktop_account_id);
    }
}

/// 只读取持久化账号池策略快照，不创建 OAuth manager，也不触发 token 刷新。
///
/// live 配置投影只需要知道账号池是否可能选中 Desktop 当前登录。返回值刻意
/// 不包含账号凭据，并复用 manager 的规范化语义补齐 Desktop 和新增托管账号。
pub fn read_persisted_account_pool_policy(
    data_dir: &Path,
) -> Result<Option<CodexAccountPoolPolicy>, CodexOAuthError> {
    let storage_path = data_dir.join("codex_oauth_auth.json");
    if !storage_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(storage_path)?;
    let store: CodexOAuthStore = serde_json::from_str(&content)
        .map_err(|error| CodexOAuthError::ParseError(error.to_string()))?;
    let mut policy = store.pool_policy;
    let mut known: std::collections::HashSet<String> = policy
        .entries
        .iter()
        .map(|entry| entry.account_id.clone())
        .collect();

    if !known.contains(NATIVE_CODEX_ACCOUNT_ID) {
        policy.entries.insert(
            0,
            CodexAccountPoolEntry {
                account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                enabled: true,
                reserve_percent: 5.0,
            },
        );
        known.insert(NATIVE_CODEX_ACCOUNT_ID.to_string());
    }

    let mut missing: Vec<_> = store
        .accounts
        .iter()
        .filter(|(account_id, account)| account.is_usable() && !known.contains(*account_id))
        .map(|(account_id, _)| account_id.clone())
        .collect();
    missing.sort();
    policy
        .entries
        .extend(missing.into_iter().map(|account_id| CodexAccountPoolEntry {
            account_id,
            enabled: true,
            reserve_percent: 5.0,
        }));
    policy.entries.retain(|entry| {
        entry.account_id == NATIVE_CODEX_ACCOUNT_ID
            || store
                .accounts
                .get(&entry.account_id)
                .is_some_and(CodexAccountData::is_usable)
    });

    let desktop_account_id = crate::codex_config::get_live_codex_oauth_account_id();
    policy.desktop_account_id = desktop_account_id.clone();
    apply_desktop_account_dedupe(
        &mut policy,
        &store
            .accounts
            .iter()
            .filter(|(_, account)| account.is_usable())
            .map(|(account_id, _)| account_id.clone())
            .collect(),
        desktop_account_id.as_deref(),
    );

    Ok(Some(policy))
}

/// Codex OAuth 认证管理器（多账号）
pub struct CodexOAuthManager {
    accounts: Arc<RwLock<HashMap<String, CodexAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    pool_policy: Arc<RwLock<CodexAccountPoolPolicy>>,
    pool_runtime: Arc<Mutex<CodexPoolRuntimeState>>,
    native_credential_runtime: Arc<Mutex<NativeCredentialRuntime>>,
    /// 内存缓存的 access_token（不持久化）
    access_tokens: Arc<RwLock<HashMap<String, CachedAccessToken>>>,
    /// 每个账号的刷新锁
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    /// 账号、默认账号与 token 的组合快照必须串行持久化，避免并发 mutation
    /// 用旧内存快照覆盖刚轮换的新 refresh token。
    persistence_lock: Arc<Mutex<()>>,
    /// 进行中的 Device Code 流程：device_auth_id -> {user_code, expires_at_ms}
    /// 过期条目会在 start_device_flow 时被清理，防止放弃的登录流程导致无界增长
    pending_device_codes: Arc<RwLock<HashMap<String, PendingDeviceCode>>>,
    /// OAuth token 端点；生产环境固定为 OpenAI，测试中可替换为本地假服务。
    oauth_token_url: String,
    storage_path: PathBuf,
}

impl CodexOAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self::new_with_oauth_token_url(data_dir, OAUTH_TOKEN_URL.to_string())
    }

    /// 创建 Codex OAuth 管理器，并允许测试注入 token 端点。
    ///
    /// 生产代码通过 `new` 使用官方端点；测试代码用本地 HTTP 服务精确模拟
    /// refresh token 失效、轮换和临时网络错误，避免依赖真实 OpenAI。
    fn new_with_oauth_token_url(data_dir: PathBuf, oauth_token_url: String) -> Self {
        let storage_path = data_dir.join("codex_oauth_auth.json");

        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            pool_policy: Arc::new(RwLock::new(CodexAccountPoolPolicy::default())),
            pool_runtime: Arc::new(Mutex::new(CodexPoolRuntimeState::default())),
            native_credential_runtime: Arc::new(Mutex::new(NativeCredentialRuntime::default())),
            access_tokens: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            persistence_lock: Arc::new(Mutex::new(())),
            pending_device_codes: Arc::new(RwLock::new(HashMap::new())),
            oauth_token_url,
            storage_path,
        };

        if let Err(e) = manager.load_from_disk_sync() {
            log::warn!("[CodexOAuth] 加载存储失败: {e}");
        }

        manager
    }

    // ==================== 设备码流程 ====================

    /// 启动 Device Code 流程
    ///
    /// 返回 GitHubDeviceCodeResponse 复用现有前端结构，但字段含义对应 OpenAI 的字段：
    /// - device_code = device_auth_id
    /// - user_code = user_code
    /// - verification_uri = https://auth.openai.com/codex/device
    pub async fn start_device_flow(&self) -> Result<GitHubDeviceCodeResponse, CodexOAuthError> {
        log::info!("[CodexOAuth] 启动 Device Code 流程");

        let response = crate::proxy::http_client::get()
            .post(DEVICE_AUTH_USERCODE_URL)
            .header("Content-Type", "application/json")
            .header("User-Agent", CODEX_USER_AGENT)
            .json(&serde_json::json!({ "client_id": CODEX_CLIENT_ID }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CodexOAuthError::NetworkError(format!(
                "Device Code 请求失败: {status} - {text}"
            )));
        }

        let device: DeviceCodeResponse = response
            .json()
            .await
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))?;

        let interval = parse_interval(device.interval.as_ref());
        let expires_in = device.expires_in.unwrap_or(DEVICE_CODE_DEFAULT_EXPIRES_IN);
        let expires_at_ms = chrono::Utc::now().timestamp_millis() + (expires_in as i64) * 1000;

        // 记录 device_auth_id -> 用户码映射；同时清理所有已过期的条目，
        // 避免用户放弃登录流程导致 HashMap 无界增长
        {
            let mut pending = self.pending_device_codes.write().await;
            let now_ms = chrono::Utc::now().timestamp_millis();
            pending.retain(|_, entry| entry.expires_at_ms > now_ms);
            pending.insert(
                device.device_auth_id.clone(),
                PendingDeviceCode {
                    user_code: device.user_code.clone(),
                    expires_at_ms,
                },
            );
        }

        log::info!(
            "[CodexOAuth] 获取 Device Code 成功，user_code: {}",
            device.user_code
        );

        Ok(GitHubDeviceCodeResponse {
            device_code: device.device_auth_id,
            user_code: device.user_code,
            verification_uri: DEVICE_VERIFICATION_URL.to_string(),
            expires_in,
            interval,
        })
    }

    /// 轮询 Device Code 状态
    ///
    /// 接收 device_code（即 device_auth_id），返回 Some(account) 表示授权成功
    pub async fn poll_for_token(
        &self,
        device_code: &str,
    ) -> Result<Option<GitHubAccount>, CodexOAuthError> {
        let entry = {
            let pending = self.pending_device_codes.read().await;
            pending.get(device_code).cloned()
        };

        let entry = entry.ok_or_else(|| {
            CodexOAuthError::TokenFetchFailed(
                "未找到对应的 user_code，请重新启动登录流程".to_string(),
            )
        })?;

        if entry.expires_at_ms <= chrono::Utc::now().timestamp_millis() {
            let mut pending = self.pending_device_codes.write().await;
            pending.remove(device_code);
            return Err(CodexOAuthError::ExpiredToken);
        }

        let user_code = entry.user_code;

        log::debug!("[CodexOAuth] 轮询 Device Code");

        let poll_response = crate::proxy::http_client::get()
            .post(DEVICE_AUTH_TOKEN_URL)
            .header("Content-Type", "application/json")
            .header("User-Agent", CODEX_USER_AGENT)
            .json(&serde_json::json!({
                "device_auth_id": device_code,
                "user_code": user_code,
            }))
            .send()
            .await?;

        let status = poll_response.status();

        // 403/404 表示用户未完成授权，继续轮询
        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
            return Err(CodexOAuthError::AuthorizationPending);
        }

        if status == reqwest::StatusCode::GONE {
            return Err(CodexOAuthError::ExpiredToken);
        }

        if !status.is_success() {
            let text = poll_response.text().await.unwrap_or_default();
            return Err(CodexOAuthError::TokenFetchFailed(format!(
                "{status} - {text}"
            )));
        }

        let success: DevicePollSuccess = poll_response
            .json()
            .await
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))?;

        log::info!("[CodexOAuth] 用户已授权，正在换取 OAuth Token");

        // 用 authorization_code + code_verifier 换 token
        let tokens = self
            .exchange_code_for_tokens(&success.authorization_code, &success.code_verifier)
            .await?;

        // 清理 pending device code
        {
            let mut pending = self.pending_device_codes.write().await;
            pending.remove(device_code);
        }

        let refresh_token = tokens.refresh_token.clone().ok_or_else(|| {
            CodexOAuthError::TokenFetchFailed("响应缺少 refresh_token".to_string())
        })?;

        let (account_id, email) = extract_identity_from_tokens(&tokens);
        let account_id = account_id.ok_or_else(|| {
            CodexOAuthError::ParseError("无法从 token 中提取 account_id".to_string())
        })?;

        // access_token 是短期 Bearer 凭据，只放内存；长期恢复依赖磁盘里的 refresh_token。
        {
            let mut tokens_cache = self.access_tokens.write().await;
            tokens_cache.insert(
                account_id.clone(),
                CachedAccessToken {
                    token: tokens.access_token.clone(),
                    expires_at_ms: compute_expires_at_ms(tokens.expires_in),
                },
            );
        }

        let expires_at_ms = compute_expires_at_ms(tokens.expires_in);
        let account = self
            .add_account_internal(
                account_id,
                refresh_token,
                email,
                Some(tokens.access_token),
                Some(expires_at_ms),
            )
            .await?;

        Ok(Some(account))
    }

    /// 用 authorization_code + code_verifier 换取 tokens
    async fn exchange_code_for_tokens(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<OAuthTokenResponse, CodexOAuthError> {
        let response = crate::proxy::http_client::get()
            .post(&self.oauth_token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", CODEX_USER_AGENT)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", DEVICE_REDIRECT_URI),
                ("client_id", CODEX_CLIENT_ID),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(CodexOAuthError::TokenFetchFailed(format!(
                "Token 交换失败: {status} - {text}"
            )));
        }

        response
            .json()
            .await
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))
    }

    /// 用 refresh_token 刷新 access_token
    async fn refresh_with_token(
        &self,
        refresh_token: &str,
    ) -> Result<OAuthTokenResponse, CodexOAuthError> {
        let response = crate::proxy::http_client::get()
            .post(&self.oauth_token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("User-Agent", CODEX_USER_AGENT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", CODEX_CLIENT_ID),
                ("scope", "openid profile email"),
            ])
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            // HTTP 状态本身不能证明 refresh token 已永久失效。代理、WAF、账号风控
            // 和上游临时认证故障都可能返回 401/403；只有 OAuth body 明确给出
            // invalid_grant/等价错误时才允许进入不可用状态。
            if oauth_error_definitively_invalidates_refresh_token(&text) {
                return Err(CodexOAuthError::RefreshTokenInvalid);
            }
            return Err(CodexOAuthError::TokenFetchFailed(format!(
                "Refresh 失败: {status} - {}",
                summarize_oauth_error(&text)
            )));
        }

        response
            .json()
            .await
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))
    }

    /// 获取指定账号的 refresh_token。
    ///
    /// 读取逻辑独立出来是为了让 access_token 持久缓存与 refresh fallback 的
    /// 顺序更清晰：先用内存短期 access_token，只有缺失或过期时才读取 refresh token。
    async fn persisted_refresh_token(&self, account_id: &str) -> Result<String, CodexOAuthError> {
        let accounts = self.accounts.read().await;
        accounts
            .get(account_id)
            .filter(|account| account.is_usable())
            .map(|a| a.refresh_token.clone())
            .ok_or_else(|| {
                if accounts.contains_key(account_id) {
                    CodexOAuthError::RefreshTokenInvalid
                } else {
                    CodexOAuthError::AccountNotFound(account_id.to_string())
                }
            })
    }

    /// 从持久化账号记录恢复仍有效的短期 access token。
    async fn load_persisted_access_token(&self, account_id: &str) -> Option<String> {
        let cached = {
            let accounts = self.accounts.read().await;
            let account = accounts.get(account_id)?;
            if !account.is_usable() {
                return None;
            }
            CachedAccessToken {
                token: account.access_token.clone()?,
                expires_at_ms: account.access_token_expires_at_ms?,
            }
        };

        if cached.is_expiring_soon() {
            return None;
        }

        self.access_tokens
            .write()
            .await
            .insert(account_id.to_string(), cached.clone());
        Some(cached.token)
    }

    // ==================== Token 获取（含自动刷新） ====================

    /// 获取指定账号的有效 access_token（必要时自动刷新）
    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, CodexOAuthError> {
        // 先检查缓存
        {
            let tokens = self.access_tokens.read().await;
            if let Some(cached) = tokens.get(account_id) {
                if !cached.is_expiring_soon() {
                    return Ok(cached.token.clone());
                }
            }
        }

        if let Some(token) = self.load_persisted_access_token(account_id).await {
            return Ok(token);
        }

        log::info!("[CodexOAuth] 账号 {account_id} 的 access_token 需要刷新");

        let refresh_lock = self.get_refresh_lock(account_id).await;
        let _guard = refresh_lock.lock().await;

        // double-check
        {
            let tokens = self.access_tokens.read().await;
            if let Some(cached) = tokens.get(account_id) {
                if !cached.is_expiring_soon() {
                    return Ok(cached.token.clone());
                }
            }
        }

        if let Some(token) = self.load_persisted_access_token(account_id).await {
            return Ok(token);
        }

        // refresh_token 是持久化凭据，可能已被另一个进程或历史独立 manager
        // 轮换并写回磁盘。真正刷新前先同步一次磁盘账号数据，避免拿内存旧
        // refresh_token 触发 invalid_grant 后误删本地账号。
        if let Err(e) = self.reload_from_disk().await {
            log::warn!("[CodexOAuth] 刷新前重新加载磁盘账号失败，将使用内存凭据: {e}");
        }

        let mut refresh_token = self.persisted_refresh_token(account_id).await?;

        let new_tokens = match self.refresh_with_token(&refresh_token).await {
            Ok(tokens) => tokens,
            Err(CodexOAuthError::RefreshTokenInvalid) => {
                // 另一个仍在退出/启动的进程可能恰好轮换了 refresh token。
                // 失效前重新读取磁盘；若 token 已变化，只用最新 token 再试一次，
                // 避免用旧请求的 invalid_grant 覆盖刚写入的新凭据。
                if let Err(error) = self.reload_from_disk().await {
                    log::warn!("[CodexOAuth] invalid_grant 后重新加载磁盘账号失败: {error}");
                }
                if let Some(token) = self.load_persisted_access_token(account_id).await {
                    return Ok(token);
                }
                let latest_refresh_token = self.persisted_refresh_token(account_id).await?;
                if latest_refresh_token != refresh_token {
                    refresh_token = latest_refresh_token;
                    match self.refresh_with_token(&refresh_token).await {
                        Ok(tokens) => tokens,
                        Err(CodexOAuthError::RefreshTokenInvalid) => {
                            self.mark_account_invalid_after_refresh_failure(account_id)
                                .await?;
                            return Err(CodexOAuthError::RefreshTokenInvalid);
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    self.mark_account_invalid_after_refresh_failure(account_id)
                        .await?;
                    return Err(CodexOAuthError::RefreshTokenInvalid);
                }
            }
            Err(error) => return Err(error),
        };

        let access_token = new_tokens.access_token.clone();
        let expires_at_ms = compute_expires_at_ms(new_tokens.expires_in);

        // refresh 成功后把轮换后的 refresh token 与仍有效的短期 access token
        // 作为同一个快照落盘。这样重启不会立即再次刷新，也不会把轮换结果拆开。
        {
            let mut accounts = self.accounts.write().await;
            if let Some(account) = accounts.get_mut(account_id) {
                if let Some(new_refresh) = new_tokens.refresh_token.clone() {
                    if new_refresh != refresh_token {
                        account.refresh_token = new_refresh;
                    }
                }
                account.access_token = Some(access_token.clone());
                account.access_token_expires_at_ms = Some(expires_at_ms);
                account.invalidated_at = None;
                account.auth_error = None;
            }
        }
        self.save_to_disk().await?;

        {
            let mut tokens = self.access_tokens.write().await;
            tokens.insert(
                account_id.to_string(),
                CachedAccessToken {
                    token: access_token.clone(),
                    expires_at_ms,
                },
            );
        }

        Ok(access_token)
    }

    /// 获取默认账号的有效 token
    pub async fn get_valid_token(&self) -> Result<String, CodexOAuthError> {
        match self.resolve_default_account_id().await {
            Some(id) => self.get_valid_token_for_account(&id).await,
            None => Err(CodexOAuthError::AccountNotFound(
                "无可用的 ChatGPT 账号".to_string(),
            )),
        }
    }

    /// 获取默认账号 ID（热路径使用，避免克隆整个账号 HashMap）
    pub async fn default_account_id(&self) -> Option<String> {
        self.resolve_default_account_id().await
    }

    pub async fn account_pool_policy(&self) -> CodexAccountPoolPolicy {
        self.normalized_pool_policy().await
    }

    fn observe_native_authorization(
        native_runtime: &mut NativeCredentialRuntime,
        native_authorization: Option<&str>,
    ) {
        let Some(authorization) = native_authorization
            .map(str::trim)
            .filter(|authorization| !authorization.is_empty())
        else {
            return;
        };
        let digest: [u8; 32] = Sha256::digest(authorization.as_bytes()).into();
        match native_runtime.authorization_digest {
            None => {
                native_runtime.authorization_digest = Some(digest);
                native_runtime.credential_generation = 1;
            }
            Some(previous) if previous != digest => {
                native_runtime.authorization_digest = Some(digest);
                native_runtime.credential_generation = native_runtime
                    .credential_generation
                    .saturating_add(1)
                    .max(1);
            }
            Some(_) => {}
        }
    }

    async fn with_prepared_pool_account_runtime<R>(
        &self,
        account_id: &str,
        update: impl FnOnce(&mut CodexPoolRuntimeState) -> R,
    ) -> Option<R> {
        if account_id == NATIVE_CODEX_ACCOUNT_ID {
            let native_runtime = self.native_credential_runtime.lock().await;
            let mut runtime = self.pool_runtime.lock().await;
            runtime.prepare_account(account_id, native_runtime.credential_generation);
            return Some(update(&mut runtime));
        }

        let accounts = self.accounts.read().await;
        let credential_generation = accounts
            .get(account_id)
            .filter(|account| account.is_usable())?
            .credential_generation;
        let mut runtime = self.pool_runtime.lock().await;
        runtime.prepare_account(account_id, credential_generation);
        Some(update(&mut runtime))
    }

    pub(crate) async fn ordered_pool_entries(
        &self,
        session_id: &str,
        native_authorization: Option<&str>,
    ) -> Vec<CodexPoolCandidate> {
        let policy = self.normalized_pool_policy().await;
        let accounts = self.accounts.read().await;
        let native_enabled = policy.enabled
            && policy
                .entries
                .iter()
                .any(|entry| entry.enabled && entry.account_id == NATIVE_CODEX_ACCOUNT_ID);
        let mut native_runtime = if native_enabled {
            Some(self.native_credential_runtime.lock().await)
        } else {
            None
        };
        if let Some(native_runtime) = native_runtime.as_mut() {
            Self::observe_native_authorization(native_runtime, native_authorization);
        }
        let enabled_account_generations: HashMap<String, u64> = policy
            .enabled
            .then(|| {
                policy
                    .entries
                    .iter()
                    .filter(|entry| entry.enabled)
                    .filter_map(|entry| {
                        if entry.account_id == NATIVE_CODEX_ACCOUNT_ID {
                            native_runtime.as_ref().map(|native_runtime| {
                                (
                                    entry.account_id.clone(),
                                    native_runtime.credential_generation,
                                )
                            })
                        } else {
                            accounts
                                .get(&entry.account_id)
                                .filter(|account| account.is_usable())
                                .map(|account| {
                                    (entry.account_id.clone(), account.credential_generation)
                                })
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let enabled_account_ids: HashSet<String> =
            enabled_account_generations.keys().cloned().collect();
        let now = chrono::Utc::now().timestamp_millis();
        let mut runtime = self.pool_runtime.lock().await;
        runtime.retain_enabled_accounts(&enabled_account_ids);
        for (account_id, credential_generation) in &enabled_account_generations {
            runtime.prepare_account(account_id, *credential_generation);
        }
        if !policy.enabled {
            return Vec::new();
        }
        // reservePercent 只阻止新 session 分配账号；已成功绑定的任务必须继续使用
        // 原账号完成/重连。429 cooldown 会主动删除 binding，仍能强制切走耗尽账号。
        let bound = runtime.bound_account(session_id, now);
        let mut entries: Vec<_> = policy
            .entries
            .into_iter()
            .filter_map(|entry| {
                let credential_generation = enabled_account_generations.get(&entry.account_id)?;
                (entry.enabled
                    && runtime.is_account_selectable(
                        &entry.account_id,
                        *credential_generation,
                        now,
                    )
                    && (bound.as_deref() == Some(entry.account_id.as_str())
                        || runtime
                            .remaining_percent(&entry.account_id)
                            .is_none_or(|value| value > entry.reserve_percent)))
                .then(|| CodexPoolCandidate {
                    entry,
                    credential_generation: *credential_generation,
                })
            })
            .collect();
        if let Some(bound) = bound {
            if let Some(index) = entries
                .iter()
                .position(|candidate| candidate.entry.account_id == bound)
            {
                let entry = entries.remove(index);
                entries.insert(0, entry);
            }
        }
        entries
    }

    pub async fn record_pool_remaining_percent(&self, account_id: &str, remaining: f64) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.with_prepared_pool_account_runtime(account_id, |runtime| {
            runtime.record_remaining_percent(account_id, remaining, now_ms)
        })
        .await;
    }

    pub async fn pool_quota_refresh_due(&self, account_id: &str) -> bool {
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.with_prepared_pool_account_runtime(account_id, |runtime| {
            runtime.quota_refresh_due(account_id, now_ms)
        })
        .await
        .unwrap_or(false)
    }

    pub async fn mark_pool_quota_checked(&self, account_id: &str) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.with_prepared_pool_account_runtime(account_id, |runtime| {
            runtime.mark_quota_checked(account_id, now_ms)
        })
        .await;
    }

    pub async fn bind_pool_session(
        &self,
        session_id: &str,
        account_id: &str,
        credential_generation: u64,
    ) {
        self.pool_runtime.lock().await.bind_session(
            session_id,
            account_id,
            credential_generation,
            chrono::Utc::now().timestamp_millis(),
        );
    }

    pub(crate) async fn record_pool_attempt(
        &self,
        account_id: &str,
        credential_generation: u64,
        session_id: &str,
        outcome: CodexPoolAttemptOutcome,
    ) -> bool {
        self.pool_runtime.lock().await.record_outcome_at(
            account_id,
            credential_generation,
            session_id,
            outcome,
            chrono::Utc::now().timestamp_millis(),
        )
    }

    pub async fn cool_down_pool_account(&self, account_id: &str, duration: std::time::Duration) {
        self.pool_runtime.lock().await.cool_down_account(
            account_id,
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
            chrono::Utc::now().timestamp_millis(),
        );
    }

    async fn reconcile_pool_runtime(&self, policy: &CodexAccountPoolPolicy) {
        let accounts = self.accounts.read().await;
        let native_runtime = self.native_credential_runtime.lock().await;
        let enabled_account_generations: HashMap<String, u64> = if policy.enabled {
            policy
                .entries
                .iter()
                .filter(|entry| entry.enabled)
                .filter_map(|entry| {
                    if entry.account_id == NATIVE_CODEX_ACCOUNT_ID {
                        Some((
                            entry.account_id.clone(),
                            native_runtime.credential_generation,
                        ))
                    } else {
                        accounts
                            .get(&entry.account_id)
                            .filter(|account| account.is_usable())
                            .map(|account| {
                                (entry.account_id.clone(), account.credential_generation)
                            })
                    }
                })
                .collect()
        } else {
            HashMap::new()
        };
        let enabled_account_ids: HashSet<String> =
            enabled_account_generations.keys().cloned().collect();
        let mut runtime = self.pool_runtime.lock().await;
        runtime.retain_enabled_accounts(&enabled_account_ids);
        for (account_id, credential_generation) in enabled_account_generations {
            runtime.prepare_account(&account_id, credential_generation);
        }
    }

    pub async fn set_account_pool_policy(
        &self,
        mut policy: CodexAccountPoolPolicy,
    ) -> Result<(), CodexOAuthError> {
        let accounts = self.accounts.read().await;
        let mut seen = HashSet::new();
        policy.entries.retain_mut(|entry| {
            entry.account_id = entry.account_id.trim().to_string();
            entry.reserve_percent = entry.reserve_percent.clamp(0.0, 100.0);
            !entry.account_id.is_empty()
                && (entry.account_id == NATIVE_CODEX_ACCOUNT_ID
                    || accounts
                        .get(&entry.account_id)
                        .is_some_and(CodexAccountData::is_usable))
                && seen.insert(entry.account_id.clone())
        });
        drop(accounts);
        *self.pool_policy.write().await = policy;
        let normalized = self.normalized_pool_policy().await;
        *self.pool_policy.write().await = normalized.clone();
        self.reconcile_pool_runtime(&normalized).await;
        self.save_to_disk().await
    }

    async fn normalized_pool_policy(&self) -> CodexAccountPoolPolicy {
        let mut policy = self.pool_policy.read().await.clone();
        let accounts = self.accounts.read().await;
        let mut known: HashSet<String> = policy
            .entries
            .iter()
            .map(|entry| entry.account_id.clone())
            .collect();
        if !known.contains(NATIVE_CODEX_ACCOUNT_ID) {
            policy.entries.insert(
                0,
                CodexAccountPoolEntry {
                    account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                    enabled: true,
                    reserve_percent: 5.0,
                },
            );
            known.insert(NATIVE_CODEX_ACCOUNT_ID.to_string());
        }
        let mut missing: Vec<_> = accounts
            .iter()
            .filter(|(account_id, account)| account.is_usable() && !known.contains(*account_id))
            .map(|(account_id, _)| account_id.clone())
            .collect();
        missing.sort();
        policy
            .entries
            .extend(missing.into_iter().map(|account_id| CodexAccountPoolEntry {
                account_id,
                enabled: true,
                reserve_percent: 5.0,
            }));
        policy.entries.retain(|entry| {
            entry.account_id == NATIVE_CODEX_ACCOUNT_ID
                || accounts
                    .get(&entry.account_id)
                    .is_some_and(CodexAccountData::is_usable)
        });
        let desktop_account_id = crate::codex_config::get_live_codex_oauth_account_id();
        policy.desktop_account_id = desktop_account_id.clone();
        apply_desktop_account_dedupe(
            &mut policy,
            &accounts
                .iter()
                .filter(|(_, account)| account.is_usable())
                .map(|(account_id, _)| account_id.clone())
                .collect(),
            desktop_account_id.as_deref(),
        );
        policy
    }

    // ==================== 多账号管理 ====================

    pub async fn list_accounts(&self) -> Vec<GitHubAccount> {
        let accounts = self.accounts.read().await.clone();
        let default_id = self.resolve_default_account_id().await;
        Self::sorted_accounts(&accounts, default_id.as_deref())
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), CodexOAuthError> {
        log::info!("[CodexOAuth] 移除账号: {account_id}");

        {
            let mut accounts = self.accounts.write().await;
            if accounts.remove(account_id).is_none() {
                return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
            }
        }

        {
            let mut tokens = self.access_tokens.write().await;
            tokens.remove(account_id);
        }
        {
            let mut locks = self.refresh_locks.write().await;
            locks.remove(account_id);
        }
        self.pool_runtime.lock().await.purge_account(account_id);

        {
            let accounts = self.accounts.read().await;
            let mut default = self.default_account_id.write().await;
            if default.as_deref() == Some(account_id) {
                *default = Self::fallback_default_account_id(&accounts);
            }
        }

        self.save_to_disk().await?;
        Ok(())
    }

    pub async fn set_default_account(&self, account_id: &str) -> Result<(), CodexOAuthError> {
        {
            let accounts = self.accounts.read().await;
            if !accounts
                .get(account_id)
                .is_some_and(CodexAccountData::is_usable)
            {
                return Err(CodexOAuthError::AccountNotFound(account_id.to_string()));
            }
        }

        {
            let mut default = self.default_account_id.write().await;
            *default = Some(account_id.to_string());
        }

        self.save_to_disk().await?;
        Ok(())
    }

    pub async fn clear_auth(&self) -> Result<(), CodexOAuthError> {
        log::info!("[CodexOAuth] 清除所有认证");

        {
            let mut accounts = self.accounts.write().await;
            accounts.clear();
        }
        {
            let mut default = self.default_account_id.write().await;
            *default = None;
        }
        {
            let mut tokens = self.access_tokens.write().await;
            tokens.clear();
        }
        {
            let mut locks = self.refresh_locks.write().await;
            locks.clear();
        }
        {
            let mut pending = self.pending_device_codes.write().await;
            pending.clear();
        }
        self.pool_runtime.lock().await.purge_all();

        if self.storage_path.exists() {
            std::fs::remove_file(&self.storage_path)?;
        }

        Ok(())
    }

    pub async fn is_authenticated(&self) -> bool {
        let accounts = self.accounts.read().await;
        accounts.values().any(CodexAccountData::is_usable)
    }

    /// 获取认证状态摘要（与 Copilot 的格式保持一致，便于复用前端）
    pub async fn get_status(&self) -> CodexOAuthStatus {
        let accounts_map = self.accounts.read().await.clone();
        let default_id = self.resolve_default_account_id().await;
        let account_list = Self::sorted_accounts(&accounts_map, default_id.as_deref());
        let authenticated = !account_list.is_empty();
        let auth_error = accounts_map
            .values()
            .filter(|account| !account.is_usable())
            .filter_map(|account| account.auth_error.clone())
            .next();
        let username = default_id
            .as_ref()
            .and_then(|id| accounts_map.get(id))
            .and_then(|a| a.email.clone())
            .or_else(|| account_list.first().map(|a| a.login.clone()));

        CodexOAuthStatus {
            accounts: account_list,
            default_account_id: default_id,
            authenticated,
            username,
            auth_error,
        }
    }

    // ==================== 内部方法 ====================

    /// 明确收到 OAuth invalid_grant 时把账号置为不可用，但保留凭据记录。
    /// 用户重新登录同一账号会原位覆盖并恢复；只有显式注销才物理删除。
    async fn mark_account_invalid_after_refresh_failure(
        &self,
        account_id: &str,
    ) -> Result<(), CodexOAuthError> {
        log::warn!(
            "[CodexOAuth] OAuth 明确返回 invalid_grant，账号进入待重新认证状态: {account_id}"
        );
        {
            let mut accounts = self.accounts.write().await;
            let account = accounts
                .get_mut(account_id)
                .ok_or_else(|| CodexOAuthError::AccountNotFound(account_id.to_string()))?;
            account.invalidated_at = Some(chrono::Utc::now().timestamp());
            account.auth_error = Some("refresh_token_invalid".to_string());
            account.access_token = None;
            account.access_token_expires_at_ms = None;
        }
        self.access_tokens.write().await.remove(account_id);
        self.pool_runtime.lock().await.purge_account(account_id);
        self.save_to_disk().await
    }

    async fn add_account_internal(
        &self,
        account_id: String,
        refresh_token: String,
        email: Option<String>,
        access_token: Option<String>,
        access_token_expires_at_ms: Option<i64>,
    ) -> Result<GitHubAccount, CodexOAuthError> {
        let now = chrono::Utc::now().timestamp();
        let account = {
            let mut accounts = self.accounts.write().await;
            let credential_generation = accounts
                .get(&account_id)
                .map(|account| account.credential_generation.saturating_add(1).max(1))
                .unwrap_or(1);
            let data = CodexAccountData {
                account_id: account_id.clone(),
                email,
                refresh_token,
                access_token,
                access_token_expires_at_ms,
                invalidated_at: None,
                auth_error: None,
                credential_generation,
                authenticated_at: now,
            };
            let account = GitHubAccount::from(&data);
            accounts.insert(account_id.clone(), data);
            account
        };
        self.pool_runtime.lock().await.purge_account(&account_id);

        {
            let mut default = self.default_account_id.write().await;
            if default.is_none() {
                *default = Some(account_id);
            }
        }

        self.save_to_disk().await?;
        Ok(account)
    }

    fn fallback_default_account_id(accounts: &HashMap<String, CodexAccountData>) -> Option<String> {
        accounts
            .iter()
            .filter(|(_, account)| account.is_usable())
            .max_by(|(id_a, a), (id_b, b)| {
                a.authenticated_at
                    .cmp(&b.authenticated_at)
                    .then_with(|| id_b.cmp(id_a))
            })
            .map(|(id, _)| id.clone())
    }

    fn sorted_accounts(
        accounts: &HashMap<String, CodexAccountData>,
        default_account_id: Option<&str>,
    ) -> Vec<GitHubAccount> {
        let mut list: Vec<GitHubAccount> = accounts
            .values()
            .filter(|account| account.is_usable())
            .map(GitHubAccount::from)
            .collect();
        list.sort_by(|a, b| {
            let a_default = default_account_id == Some(a.id.as_str());
            let b_default = default_account_id == Some(b.id.as_str());
            b_default
                .cmp(&a_default)
                .then_with(|| b.authenticated_at.cmp(&a.authenticated_at))
                .then_with(|| a.login.cmp(&b.login))
        });
        list
    }

    async fn resolve_default_account_id(&self) -> Option<String> {
        let stored = self.default_account_id.read().await.clone();
        let accounts = self.accounts.read().await;

        if let Some(id) = stored {
            if accounts.get(&id).is_some_and(CodexAccountData::is_usable) {
                return Some(id);
            }
        }

        Self::fallback_default_account_id(&accounts)
    }

    async fn get_refresh_lock(&self, account_id: &str) -> Arc<Mutex<()>> {
        {
            let locks = self.refresh_locks.read().await;
            if let Some(lock) = locks.get(account_id) {
                return Arc::clone(lock);
            }
        }

        let mut locks = self.refresh_locks.write().await;
        Arc::clone(
            locks
                .entry(account_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), CodexOAuthError> {
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| CodexOAuthError::IoError("无效的存储路径".to_string()))?;
        let file_name = self
            .storage_path
            .file_name()
            .ok_or_else(|| CodexOAuthError::IoError("无效的存储文件名".to_string()))?
            .to_string_lossy()
            .to_string();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = parent.join(format!("{file_name}.tmp.{ts}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;

            fs::rename(&tmp_path, &self.storage_path)?;
            fs::set_permissions(&self.storage_path, fs::Permissions::from_mode(0o600))?;
        }

        #[cfg(windows)]
        {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;

            if self.storage_path.exists() {
                let _ = fs::remove_file(&self.storage_path);
            }
            fs::rename(&tmp_path, &self.storage_path)?;
        }

        Ok(())
    }

    fn load_from_disk_sync(&self) -> Result<(), CodexOAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.storage_path)?;
        let store: CodexOAuthStore = serde_json::from_str(&content)
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))?;

        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
            log::info!("[CodexOAuth] 从磁盘加载 {} 个账号", accounts.len());
        }
        if let Ok(mut default) = self.default_account_id.try_write() {
            *default = store.default_account_id;
            if default.is_none() {
                if let Ok(accounts) = self.accounts.try_read() {
                    *default = Self::fallback_default_account_id(&accounts);
                }
            }
        }
        if let Ok(mut policy) = self.pool_policy.try_write() {
            *policy = store.pool_policy;
        }

        Ok(())
    }

    async fn reload_from_disk(&self) -> Result<(), CodexOAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.storage_path)?;
        let store: CodexOAuthStore = serde_json::from_str(&content)
            .map_err(|error| CodexOAuthError::ParseError(error.to_string()))?;
        let fallback_default = Self::fallback_default_account_id(&store.accounts);
        let default_account_id = store.default_account_id.or(fallback_default);

        {
            let mut accounts = self.accounts.write().await;
            *accounts = store.accounts;
            log::info!("[CodexOAuth] 从磁盘重新加载 {} 个账号", accounts.len());
        }
        *self.default_account_id.write().await = default_account_id;
        *self.pool_policy.write().await = store.pool_policy;

        let normalized = self.normalized_pool_policy().await;
        self.reconcile_pool_runtime(&normalized).await;
        Ok(())
    }

    async fn save_to_disk(&self) -> Result<(), CodexOAuthError> {
        let _persist_guard = self.persistence_lock.lock().await;
        let accounts = self.accounts.read().await.clone();
        let default = self.resolve_default_account_id().await;
        let pool_policy = self.normalized_pool_policy().await;

        let store = CodexOAuthStore {
            version: 1,
            accounts,
            default_account_id: default,
            pool_policy,
        };

        let content = serde_json::to_string_pretty(&store)
            .map_err(|e| CodexOAuthError::ParseError(e.to_string()))?;

        self.write_store_atomic(&content)?;

        log::info!(
            "[CodexOAuth] 保存到磁盘成功（{} 个账号）",
            store.accounts.len()
        );

        Ok(())
    }
}

/// Codex OAuth 状态摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexOAuthStatus {
    pub accounts: Vec<GitHubAccount>,
    pub default_account_id: Option<String>,
    pub authenticated: bool,
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_error: Option<String>,
}

// ==================== 工具函数 ====================

/// 解析 OpenAI Device Code 响应中的 interval 字段
///
/// 服务端可能返回字符串或数字，需要兼容
fn parse_interval(value: Option<&serde_json::Value>) -> u64 {
    let raw = match value {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(5),
        Some(serde_json::Value::String(s)) => s.parse::<u64>().unwrap_or(5),
        _ => 5,
    };
    raw.max(1) + POLLING_SAFETY_MARGIN_SECS
}

/// 从 expires_in（秒）计算过期时间戳（毫秒）
fn compute_expires_at_ms(expires_in: Option<i64>) -> i64 {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let secs = expires_in.unwrap_or(3600);
    now_ms + secs * 1000
}

/// 仅依据 OAuth 结构化错误码判断 refresh token 是否确定失效。
/// HTTP 401/403 不能单独作为删除或失效凭据的证据。
fn oauth_error_definitively_invalidates_refresh_token(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };

    let error = value.get("error");
    let candidates = [
        error.and_then(serde_json::Value::as_str),
        error
            .and_then(serde_json::Value::as_object)
            .and_then(|error| error.get("code"))
            .and_then(serde_json::Value::as_str),
        error
            .and_then(serde_json::Value::as_object)
            .and_then(|error| error.get("type"))
            .and_then(serde_json::Value::as_str),
        value.get("code").and_then(serde_json::Value::as_str),
    ];

    let invalid = candidates.into_iter().flatten().any(|code| {
        matches!(
            code.trim().to_ascii_lowercase().as_str(),
            "invalid_grant" | "invalid_refresh_token" | "refresh_token_invalid"
        )
    });
    invalid
}

/// OAuth 错误只保留结构化 error code，绝不回显原始 body。
fn summarize_oauth_error(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return if body.trim().is_empty() {
            "empty response".to_string()
        } else {
            "non-JSON error response".to_string()
        };
    };
    let error = value.get("error");
    let code = error
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            error
                .and_then(serde_json::Value::as_object)
                .and_then(|error| error.get("code"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| value.get("code").and_then(serde_json::Value::as_str));

    code.map(|code| {
        let safe = code
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
            .take(80)
            .collect::<String>();
        if safe.is_empty() {
            "unknown OAuth error".to_string()
        } else {
            format!("oauth_error={safe}")
        }
    })
    .unwrap_or_else(|| "unknown OAuth error".to_string())
}

/// 解析 JWT 中的 claims
fn parse_jwt_claims(token: &str) -> Option<IdTokenClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&decoded).ok()
}

/// 从 token 响应中提取 (account_id, email)
fn extract_identity_from_tokens(tokens: &OAuthTokenResponse) -> (Option<String>, Option<String>) {
    let mut account_id: Option<String> = None;
    let mut email: Option<String> = None;

    if let Some(id_token) = tokens.id_token.as_deref() {
        if let Some(claims) = parse_jwt_claims(id_token) {
            account_id = claims
                .chatgpt_account_id
                .clone()
                .or_else(|| {
                    claims
                        .openai_auth
                        .as_ref()
                        .and_then(|a| a.chatgpt_account_id.clone())
                })
                .or_else(|| claims.organizations.first().and_then(|o| o.id.clone()));
            email = claims.email.clone();
        }
    }

    if account_id.is_none() {
        if let Some(claims) = parse_jwt_claims(&tokens.access_token) {
            account_id = claims
                .chatgpt_account_id
                .clone()
                .or_else(|| {
                    claims
                        .openai_auth
                        .as_ref()
                        .and_then(|a| a.chatgpt_account_id.clone())
                })
                .or_else(|| claims.organizations.first().and_then(|o| o.id.clone()));
            if email.is_none() {
                email = claims.email.clone();
            }
        }
    }

    (account_id, email)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn oauth_error_classification_requires_explicit_invalid_grant_semantics() {
        assert!(oauth_error_definitively_invalidates_refresh_token(
            r#"{"error":"invalid_grant"}"#
        ));
        assert!(oauth_error_definitively_invalidates_refresh_token(
            r#"{"error":{"code":"refresh_token_invalid"}}"#
        ));
        assert!(!oauth_error_definitively_invalidates_refresh_token(
            r#"{"error":"temporarily_unavailable"}"#
        ));
        assert!(!oauth_error_definitively_invalidates_refresh_token(
            "Unauthorized"
        ));
        assert_eq!(
            summarize_oauth_error(r#"{"error":"temporarily_unavailable","token":"must-not-leak"}"#),
            "oauth_error=temporarily_unavailable"
        );
        assert_eq!(
            summarize_oauth_error("secret non-json body"),
            "non-JSON error response"
        );
    }

    #[test]
    fn test_parse_interval_number() {
        let v = serde_json::Value::Number(serde_json::Number::from(5));
        assert_eq!(parse_interval(Some(&v)), 5 + POLLING_SAFETY_MARGIN_SECS);
    }

    #[test]
    fn test_parse_interval_string() {
        let v = serde_json::Value::String("10".to_string());
        assert_eq!(parse_interval(Some(&v)), 10 + POLLING_SAFETY_MARGIN_SECS);
    }

    #[test]
    fn test_parse_interval_default() {
        assert_eq!(parse_interval(None), 5 + POLLING_SAFETY_MARGIN_SECS);
    }

    #[test]
    fn test_parse_interval_min() {
        let v = serde_json::Value::Number(serde_json::Number::from(0));
        // 0 应被提升到 1
        assert_eq!(parse_interval(Some(&v)), 1 + POLLING_SAFETY_MARGIN_SECS);
    }

    #[test]
    fn test_compute_expires_at_ms() {
        let result = compute_expires_at_ms(Some(3600));
        let now = chrono::Utc::now().timestamp_millis();
        // 应在未来约 3600 秒处（允许少量误差）
        assert!(result > now + 3500 * 1000);
        assert!(result < now + 3700 * 1000);
    }

    #[test]
    fn test_compute_expires_at_ms_default() {
        let result = compute_expires_at_ms(None);
        let now = chrono::Utc::now().timestamp_millis();
        assert!(result > now);
    }

    #[test]
    fn test_cached_token_expiring_soon() {
        let now = chrono::Utc::now().timestamp_millis();
        // 30 秒后过期 - 在缓冲期内
        let expiring = CachedAccessToken {
            token: "t".to_string(),
            expires_at_ms: now + 30_000,
        };
        assert!(expiring.is_expiring_soon());

        // 1 小时后过期 - 不在缓冲期内
        let valid = CachedAccessToken {
            token: "t".to_string(),
            expires_at_ms: now + 3_600_000,
        };
        assert!(!valid.is_expiring_soon());
    }

    #[test]
    fn test_parse_jwt_claims_invalid() {
        assert!(parse_jwt_claims("not-a-jwt").is_none());
        assert!(parse_jwt_claims("only.two").is_none());
    }

    #[test]
    fn test_parse_jwt_claims_valid() {
        // Header: {"alg":"none"}
        // Payload: {"chatgpt_account_id":"acc-123","email":"test@example.com"}
        // Signature: empty
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD
            .encode(b"{\"chatgpt_account_id\":\"acc-123\",\"email\":\"test@example.com\"}");
        let jwt = format!("{header}.{payload}.");
        let claims = parse_jwt_claims(&jwt).unwrap();
        assert_eq!(claims.chatgpt_account_id.as_deref(), Some("acc-123"));
        assert_eq!(claims.email.as_deref(), Some("test@example.com"));
    }

    #[test]
    fn pool_policy_deduplicates_managed_account_when_desktop_same_and_native_enabled() {
        let mut policy = CodexAccountPoolPolicy {
            enabled: true,
            entries: vec![
                CodexAccountPoolEntry {
                    account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                    enabled: true,
                    reserve_percent: 5.0,
                },
                CodexAccountPoolEntry {
                    account_id: "acc-desktop".to_string(),
                    enabled: true,
                    reserve_percent: 9.0,
                },
                CodexAccountPoolEntry {
                    account_id: "acc-other".to_string(),
                    enabled: true,
                    reserve_percent: 5.0,
                },
            ],
            desktop_account_id: None,
        };
        let managed_ids =
            std::collections::HashSet::from(["acc-desktop".to_string(), "acc-other".to_string()]);

        apply_desktop_account_dedupe(&mut policy, &managed_ids, Some("acc-desktop"));

        assert_eq!(policy.entries.len(), 2);
        assert!(policy
            .entries
            .iter()
            .all(|entry| entry.account_id != "acc-desktop"));
        assert_eq!(policy.entries[0].account_id, NATIVE_CODEX_ACCOUNT_ID);
        assert_eq!(policy.entries[1].account_id, "acc-other");
    }

    #[test]
    fn pool_policy_keeps_managed_account_when_desktop_native_entry_is_disabled() {
        let mut policy = CodexAccountPoolPolicy {
            enabled: true,
            entries: vec![
                CodexAccountPoolEntry {
                    account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                    enabled: false,
                    reserve_percent: 5.0,
                },
                CodexAccountPoolEntry {
                    account_id: "acc-desktop".to_string(),
                    enabled: true,
                    reserve_percent: 9.0,
                },
            ],
            desktop_account_id: None,
        };
        let managed_ids = std::collections::HashSet::from(["acc-desktop".to_string()]);

        apply_desktop_account_dedupe(&mut policy, &managed_ids, Some("acc-desktop"));

        assert_eq!(policy.entries.len(), 2);
        assert_eq!(policy.entries[1].account_id, "acc-desktop");
    }

    #[test]
    fn test_parse_jwt_claims_organizations_fallback() {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"organizations\":[{\"id\":\"org-456\"}]}");
        let jwt = format!("{header}.{payload}.");
        let claims = parse_jwt_claims(&jwt).unwrap();
        assert_eq!(
            claims
                .organizations
                .first()
                .and_then(|o| o.id.clone())
                .as_deref(),
            Some("org-456")
        );
    }

    #[tokio::test]
    async fn test_manager_initial_state() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        assert!(!manager.is_authenticated().await);
        assert!(manager.list_accounts().await.is_empty());
    }

    async fn add_pool_test_account(
        manager: &CodexOAuthManager,
        account_id: &str,
        refresh_token: &str,
    ) {
        manager
            .add_account_internal(
                account_id.to_string(),
                refresh_token.to_string(),
                Some(format!("{account_id}@example.test")),
                None,
                None,
            )
            .await
            .unwrap();
    }

    async fn set_single_managed_pool_enabled(
        manager: &CodexOAuthManager,
        account_id: &str,
        enabled: bool,
        reserve_percent: f64,
    ) {
        manager
            .set_account_pool_policy(CodexAccountPoolPolicy {
                enabled: true,
                entries: vec![
                    CodexAccountPoolEntry {
                        account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                        enabled: false,
                        reserve_percent: 0.0,
                    },
                    CodexAccountPoolEntry {
                        account_id: account_id.to_string(),
                        enabled,
                        reserve_percent,
                    },
                ],
                desktop_account_id: None,
            })
            .await
            .unwrap();
    }

    async fn enable_single_managed_pool(
        manager: &CodexOAuthManager,
        account_id: &str,
        reserve_percent: f64,
    ) {
        set_single_managed_pool_enabled(manager, account_id, true, reserve_percent).await;
    }

    #[tokio::test]
    async fn test_manager_save_and_load() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_path_buf();

        // Manually inject an account through internal methods
        {
            let manager = CodexOAuthManager::new(path.clone());
            manager
                .add_account_internal(
                    "acc-123".to_string(),
                    "rt-secret".to_string(),
                    Some("user@example.com".to_string()),
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        // New manager should load from disk
        let manager2 = CodexOAuthManager::new(path);
        let accounts = manager2.list_accounts().await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "acc-123");
    }

    #[test]
    fn codex_multirouter_auth_facade_reads_normalized_pool_policy_without_loading_tokens() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("codex_oauth_auth.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "accounts": {
                    "managed-account": {
                        "account_id": "managed-account",
                        "refresh_token": "must-not-be-returned",
                        "authenticated_at": 1
                    }
                },
                "pool_policy": {
                    "enabled": true,
                    "entries": [{
                        "accountId": "managed-account",
                        "enabled": true,
                        "reservePercent": 7.5
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let policy = read_persisted_account_pool_policy(temp.path())
            .expect("read policy")
            .expect("policy exists");
        assert!(policy.enabled);
        assert_eq!(policy.entries.len(), 2);
        assert_eq!(policy.entries[0].account_id, NATIVE_CODEX_ACCOUNT_ID);
        assert!(policy.entries[0].enabled);
        assert_eq!(policy.entries[1].account_id, "managed-account");
        assert_eq!(policy.entries[1].reserve_percent, 7.5);
        assert!(!format!("{policy:?}").contains("must-not-be-returned"));
    }

    #[tokio::test]
    async fn test_remove_account() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());

        manager
            .add_account_internal(
                "acc-123".to_string(),
                "rt".to_string(),
                Some("a@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();
        manager
            .add_account_internal(
                "acc-456".to_string(),
                "rt2".to_string(),
                Some("b@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        manager.remove_account("acc-123").await.unwrap();
        let accounts = manager.list_accounts().await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "acc-456");
    }

    #[tokio::test]
    async fn removing_and_readding_account_does_not_restore_old_pool_binding() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        add_pool_test_account(&manager, "acc-a", "refresh-a").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        manager.bind_pool_session("thread-old", "acc-a", 1).await;
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        manager.remove_account("acc-a").await.unwrap();
        add_pool_test_account(&manager, "acc-a", "refresh-b").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        assert!(manager
            .ordered_pool_entries("thread-old", None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn invalidated_account_is_never_returned_by_pool_policy() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        add_pool_test_account(&manager, "acc-a", "refresh-a").await;
        enable_single_managed_pool(&manager, "acc-a", 0.0).await;

        manager
            .mark_account_invalid_after_refresh_failure("acc-a")
            .await
            .unwrap();

        assert!(manager
            .ordered_pool_entries("thread-new", None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn disabling_then_reenabling_pool_entry_does_not_restore_old_binding() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        add_pool_test_account(&manager, "acc-a", "refresh-a").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        manager.bind_pool_session("thread-old", "acc-a", 1).await;
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        set_single_managed_pool_enabled(&manager, "acc-a", false, 10.0).await;
        set_single_managed_pool_enabled(&manager, "acc-a", true, 10.0).await;
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        assert!(manager
            .ordered_pool_entries("thread-old", None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn clearing_auth_does_not_restore_old_pool_binding_after_relogin() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        add_pool_test_account(&manager, "acc-a", "refresh-a").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        manager.bind_pool_session("thread-old", "acc-a", 1).await;
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        manager.clear_auth().await.unwrap();
        add_pool_test_account(&manager, "acc-a", "refresh-b").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        assert!(manager
            .ordered_pool_entries("thread-old", None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn legacy_generation_relogin_persists_first_credential_generation() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("codex_oauth_auth.json"),
            r#"{
              "version": 1,
              "accounts": {
                "acc-a": {
                  "account_id": "acc-a",
                  "email": "a@example.test",
                  "refresh_token": "refresh-old",
                  "authenticated_at": 1
                }
              },
              "default_account_id": "acc-a",
              "pool_policy": { "enabled": false, "entries": [] }
            }"#,
        )
        .unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());

        add_pool_test_account(&manager, "acc-a", "refresh-new").await;

        let persisted: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&manager.storage_path).unwrap()).unwrap();
        assert_eq!(
            persisted["accounts"]["acc-a"]["credential_generation"].as_u64(),
            Some(1)
        );
    }

    #[tokio::test]
    async fn stale_managed_candidate_cannot_rebind_after_relogin() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        add_pool_test_account(&manager, "acc-a", "refresh-a").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        let stale_candidate = manager
            .ordered_pool_entries("thread-old", None)
            .await
            .pop()
            .unwrap();
        assert_eq!(stale_candidate.credential_generation, 1);

        add_pool_test_account(&manager, "acc-a", "refresh-b").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        assert_eq!(
            manager.accounts.read().await["acc-a"].credential_generation,
            2
        );
        manager.record_pool_remaining_percent("acc-a", 9.0).await;
        manager
            .bind_pool_session(
                "thread-late",
                "acc-a",
                stale_candidate.credential_generation,
            )
            .await;

        assert!(manager
            .ordered_pool_entries("thread-late", None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn manager_pool_attempt_outcomes_respect_selected_generation() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        add_pool_test_account(&manager, "acc-a", "refresh-a").await;
        enable_single_managed_pool(&manager, "acc-a", 0.0).await;
        let candidate = manager
            .ordered_pool_entries("thread", None)
            .await
            .pop()
            .unwrap();
        manager
            .bind_pool_session("thread", "acc-a", candidate.credential_generation)
            .await;

        assert!(
            manager
                .record_pool_attempt(
                    "acc-a",
                    candidate.credential_generation,
                    "thread",
                    CodexPoolAttemptOutcome::Credential { status: Some(401) },
                )
                .await
        );
        assert!(manager
            .ordered_pool_entries("thread", None)
            .await
            .is_empty());
        assert!(
            !manager
                .record_pool_attempt(
                    "acc-a",
                    candidate.credential_generation.saturating_sub(1),
                    "late",
                    CodexPoolAttemptOutcome::Success,
                )
                .await
        );
        assert!(
            manager
                .record_pool_attempt(
                    "acc-a",
                    candidate.credential_generation,
                    "recovered",
                    CodexPoolAttemptOutcome::Success,
                )
                .await
        );
        let recovered = manager.ordered_pool_entries("recovered", None).await;
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].entry.account_id, "acc-a");
    }

    #[tokio::test]
    async fn first_quota_snapshot_after_restart_respects_reserve_percent() {
        let temp = tempfile::tempdir().unwrap();
        {
            let manager = CodexOAuthManager::new(temp.path().to_path_buf());
            add_pool_test_account(&manager, "acc-a", "refresh-a").await;
            enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        }

        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        assert!(manager
            .ordered_pool_entries("thread-new", None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn native_authorization_change_invalidates_old_binding_without_storing_token() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .set_account_pool_policy(CodexAccountPoolPolicy {
                enabled: true,
                entries: vec![CodexAccountPoolEntry {
                    account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                    enabled: true,
                    reserve_percent: 10.0,
                }],
                desktop_account_id: None,
            })
            .await
            .unwrap();
        let first = manager
            .ordered_pool_entries("native-thread", Some("Bearer native-a"))
            .await
            .pop()
            .unwrap();
        manager
            .bind_pool_session(
                "native-thread",
                NATIVE_CODEX_ACCOUNT_ID,
                first.credential_generation,
            )
            .await;
        manager
            .record_pool_remaining_percent(NATIVE_CODEX_ACCOUNT_ID, 9.0)
            .await;
        assert_eq!(
            manager
                .ordered_pool_entries("native-thread", Some("Bearer native-a"))
                .await
                .len(),
            1
        );

        let changed = manager
            .ordered_pool_entries("native-thread", Some("Bearer native-b"))
            .await;
        assert_eq!(changed.len(), 1);
        assert_ne!(
            changed[0].credential_generation,
            first.credential_generation
        );
        manager
            .record_pool_remaining_percent(NATIVE_CODEX_ACCOUNT_ID, 9.0)
            .await;
        assert!(manager
            .ordered_pool_entries("native-thread", Some("Bearer native-b"))
            .await
            .is_empty());
        assert!(
            !manager.storage_path.exists()
                || !std::fs::read_to_string(&manager.storage_path)
                    .unwrap()
                    .contains("native-a")
        );
    }

    #[tokio::test]
    async fn external_account_removal_purges_runtime_before_same_id_returns() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        add_pool_test_account(&manager, "acc-a", "refresh-a").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        manager.bind_pool_session("old-thread", "acc-a", 1).await;
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        std::fs::write(
            &manager.storage_path,
            r#"{ "version": 1, "accounts": {}, "pool_policy": { "enabled": false, "entries": [] } }"#,
        )
        .unwrap();
        manager.reload_from_disk().await.unwrap();
        std::fs::write(
            &manager.storage_path,
            r#"{
              "version": 1,
              "accounts": {
                "acc-a": {
                  "account_id": "acc-a",
                  "refresh_token": "refresh-b",
                  "credential_generation": 1,
                  "authenticated_at": 2
                }
              },
              "default_account_id": "acc-a",
              "pool_policy": {
                "enabled": true,
                "entries": [
                  { "accountId": "native_codex_auth", "enabled": false, "reservePercent": 0.0 },
                  { "accountId": "acc-a", "enabled": true, "reservePercent": 10.0 }
                ]
              }
            }"#,
        )
        .unwrap();
        manager.reload_from_disk().await.unwrap();
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        assert!(manager
            .ordered_pool_entries("old-thread", None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn external_generation_change_invalidates_old_binding() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        add_pool_test_account(&manager, "acc-a", "refresh-a").await;
        enable_single_managed_pool(&manager, "acc-a", 10.0).await;
        manager.bind_pool_session("old-thread", "acc-a", 1).await;
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        std::fs::write(
            &manager.storage_path,
            r#"{
              "version": 1,
              "accounts": {
                "acc-a": {
                  "account_id": "acc-a",
                  "refresh_token": "refresh-b",
                  "credential_generation": 2,
                  "authenticated_at": 2
                }
              },
              "default_account_id": "acc-a",
              "pool_policy": {
                "enabled": true,
                "entries": [
                  { "accountId": "native_codex_auth", "enabled": false, "reservePercent": 0.0 },
                  { "accountId": "acc-a", "enabled": true, "reservePercent": 10.0 }
                ]
              }
            }"#,
        )
        .unwrap();
        manager.reload_from_disk().await.unwrap();
        manager.record_pool_remaining_percent("acc-a", 9.0).await;

        assert_eq!(
            manager.accounts.read().await["acc-a"].credential_generation,
            2
        );
        assert!(manager
            .ordered_pool_entries("old-thread", None)
            .await
            .is_empty());
    }

    #[tokio::test]
    async fn account_pool_honors_order_reserve_cooldown_and_session_binding() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(temp.path().to_path_buf());
        manager
            .add_account_internal(
                "acc-a".to_string(),
                "rt-a".to_string(),
                Some("a@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();
        manager
            .set_account_pool_policy(CodexAccountPoolPolicy {
                enabled: true,
                entries: vec![
                    CodexAccountPoolEntry {
                        account_id: "acc-a".to_string(),
                        enabled: true,
                        reserve_percent: 5.0,
                    },
                    CodexAccountPoolEntry {
                        account_id: NATIVE_CODEX_ACCOUNT_ID.to_string(),
                        enabled: true,
                        reserve_percent: 10.0,
                    },
                ],
                desktop_account_id: None,
            })
            .await
            .unwrap();

        manager
            .bind_pool_session("thread-1", NATIVE_CODEX_ACCOUNT_ID, 0)
            .await;
        let entries = manager.ordered_pool_entries("thread-1", None).await;
        assert_eq!(entries[0].entry.account_id, NATIVE_CODEX_ACCOUNT_ID);

        manager
            .record_pool_remaining_percent(NATIVE_CODEX_ACCOUNT_ID, 9.0)
            .await;
        let entries = manager.ordered_pool_entries("thread-1", None).await;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].entry.account_id, NATIVE_CODEX_ACCOUNT_ID);

        let entries = manager.ordered_pool_entries("thread-2", None).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry.account_id, "acc-a");

        manager
            .cool_down_pool_account(NATIVE_CODEX_ACCOUNT_ID, std::time::Duration::from_secs(60))
            .await;
        let entries = manager.ordered_pool_entries("thread-1", None).await;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry.account_id, "acc-a");
    }

    /// 启动一次性 OAuth token 假端点。
    ///
    /// 每个测试只需要接收一次 refresh 请求；返回 URL 后后台任务会读取请求并写回指定状态码和响应体。
    async fn spawn_single_refresh_endpoint(status: u16, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test oauth endpoint");
        let addr = listener
            .local_addr()
            .expect("read test oauth endpoint addr");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept refresh request");
            let mut buffer = [0_u8; 4096];
            let _ = socket
                .read(&mut buffer)
                .await
                .expect("read refresh request");
            let status_text = match status {
                200 => "OK",
                401 => "Unauthorized",
                403 => "Forbidden",
                _ => "Test",
            };
            let response = format!(
                "HTTP/1.1 {status} {status_text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write refresh response");
        });

        format!("http://{addr}/oauth/token")
    }

    /// 启动可验证 refresh token 轮换的假端点。
    ///
    /// 第一次请求必须使用 old-refresh 并返回 fresh-refresh；第二次请求只有使用
    /// fresh-refresh 才返回成功，否则模拟官方 invalid_grant。这个端点用于复现
    /// 多 manager 内存凭据落后于磁盘时的账号误删风险。
    async fn spawn_rotating_refresh_endpoint() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rotating oauth endpoint");
        let addr = listener
            .local_addr()
            .expect("read rotating oauth endpoint addr");

        tokio::spawn(async move {
            for request_index in 0..2 {
                let (mut socket, _) = listener.accept().await.expect("accept refresh request");
                let mut buffer = [0_u8; 4096];
                let n = socket
                    .read(&mut buffer)
                    .await
                    .expect("read refresh request");
                let request = String::from_utf8_lossy(&buffer[..n]);
                let body = if request_index == 0 && request.contains("old-refresh") {
                    r#"{"access_token":"access-one","refresh_token":"fresh-refresh","expires_in":0}"#
                } else if request_index == 1 && request.contains("fresh-refresh") {
                    r#"{"access_token":"access-two","refresh_token":"fresh-refresh","expires_in":3600}"#
                } else {
                    r#"{"error":"invalid_grant"}"#
                };
                let status = if body.contains("invalid_grant") {
                    "401 Unauthorized"
                } else {
                    "200 OK"
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write refresh response");
            }
        });

        format!("http://{addr}/oauth/token")
    }

    /// 模拟另一个进程在本进程 refresh 请求飞行期间完成 token 轮换并写盘。
    async fn spawn_cross_process_rotation_endpoint(storage_path: PathBuf) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind racing oauth endpoint");
        let addr = listener.local_addr().expect("read racing endpoint addr");

        tokio::spawn(async move {
            for request_index in 0..2 {
                let (mut socket, _) = listener.accept().await.expect("accept refresh request");
                let mut buffer = [0_u8; 4096];
                let n = socket
                    .read(&mut buffer)
                    .await
                    .expect("read refresh request");
                let request = String::from_utf8_lossy(&buffer[..n]);

                let (status, body) = if request_index == 0 && request.contains("old-refresh") {
                    let rotated_store = serde_json::json!({
                        "version": 1,
                        "default_account_id": "acc-race",
                        "accounts": {
                            "acc-race": {
                                "account_id": "acc-race",
                                "email": "race@example.com",
                                "refresh_token": "rotated-refresh",
                                "authenticated_at": 1
                            }
                        }
                    });
                    std::fs::write(
                        &storage_path,
                        serde_json::to_string_pretty(&rotated_store).unwrap(),
                    )
                    .expect("persist cross-process rotation");
                    ("401 Unauthorized", r#"{"error":"invalid_grant"}"#)
                } else if request_index == 1 && request.contains("rotated-refresh") {
                    (
                        "200 OK",
                        r#"{"access_token":"race-access","refresh_token":"rotated-refresh","expires_in":3600}"#,
                    )
                } else {
                    ("401 Unauthorized", r#"{"error":"invalid_grant"}"#)
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .await
                    .expect("write refresh response");
            }
        });

        format!("http://{addr}/oauth/token")
    }

    #[tokio::test]
    async fn get_status_does_not_refresh_or_remove_invalid_account() {
        let temp = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new_with_oauth_token_url(
            temp.path().to_path_buf(),
            "http://127.0.0.1:9/oauth/token".to_string(),
        );

        manager
            .add_account_internal(
                "acc-expired".to_string(),
                "rt-expired".to_string(),
                Some("expired@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        let status = manager.get_status().await;

        assert!(status.authenticated);
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(status.default_account_id.as_deref(), Some("acc-expired"));
    }

    #[tokio::test]
    async fn token_request_quarantines_account_when_refresh_token_is_invalid() {
        let temp = tempfile::tempdir().unwrap();
        let token_url = spawn_single_refresh_endpoint(401, r#"{"error":"invalid_grant"}"#).await;
        let manager =
            CodexOAuthManager::new_with_oauth_token_url(temp.path().to_path_buf(), token_url);

        manager
            .add_account_internal(
                "acc-expired".to_string(),
                "rt-expired".to_string(),
                Some("expired@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        let result = manager.get_valid_token_for_account("acc-expired").await;
        let status = manager.get_status().await;

        assert!(matches!(result, Err(CodexOAuthError::RefreshTokenInvalid)));
        assert!(!status.authenticated);
        assert!(status.accounts.is_empty());
        assert!(status.default_account_id.is_none());
        assert_eq!(status.auth_error.as_deref(), Some("refresh_token_invalid"));
        let persisted = std::fs::read_to_string(&manager.storage_path)
            .expect("invalid account state should remain persisted for recovery");
        assert!(persisted.contains(r#""acc-expired""#));
        assert!(persisted.contains(r#""invalidated_at""#));
        assert!(persisted.contains(r#""auth_error": "refresh_token_invalid""#));
    }

    #[tokio::test]
    async fn transient_refresh_401_does_not_delete_or_invalidate_account() {
        let temp = tempfile::tempdir().unwrap();
        let token_url = spawn_single_refresh_endpoint(
            401,
            r#"{"error":"temporarily_unavailable","error_description":"upstream auth edge unavailable"}"#,
        )
        .await;
        let manager =
            CodexOAuthManager::new_with_oauth_token_url(temp.path().to_path_buf(), token_url);

        manager
            .add_account_internal(
                "acc-retry".to_string(),
                "rt-retry".to_string(),
                Some("retry@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        let result = manager.get_valid_token_for_account("acc-retry").await;
        let status = manager.get_status().await;

        assert!(matches!(result, Err(CodexOAuthError::TokenFetchFailed(_))));
        assert!(status.authenticated);
        assert_eq!(status.accounts.len(), 1);
        assert!(status.auth_error.is_none());
        let persisted = std::fs::read_to_string(&manager.storage_path)
            .expect("transient refresh failure must keep the account store");
        assert!(persisted.contains(r#""acc-retry""#));
        assert!(!persisted.contains("invalidated_at"));
    }

    #[tokio::test]
    async fn relogin_same_account_clears_quarantined_state_in_place() {
        let temp = tempfile::tempdir().unwrap();
        let token_url = spawn_single_refresh_endpoint(400, r#"{"error":"invalid_grant"}"#).await;
        let manager =
            CodexOAuthManager::new_with_oauth_token_url(temp.path().to_path_buf(), token_url);
        manager
            .add_account_internal(
                "acc-relogin".to_string(),
                "expired-refresh".to_string(),
                Some("relogin@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(matches!(
            manager.get_valid_token_for_account("acc-relogin").await,
            Err(CodexOAuthError::RefreshTokenInvalid)
        ));

        manager
            .add_account_internal(
                "acc-relogin".to_string(),
                "new-refresh".to_string(),
                Some("relogin@example.com".to_string()),
                Some("new-access".to_string()),
                Some(chrono::Utc::now().timestamp_millis() + 3_600_000),
            )
            .await
            .unwrap();
        let status = manager.get_status().await;

        assert!(status.authenticated);
        assert_eq!(status.accounts.len(), 1);
        assert!(status.auth_error.is_none());
        assert_eq!(
            manager
                .get_valid_token_for_account("acc-relogin")
                .await
                .unwrap(),
            "new-access"
        );
    }

    #[tokio::test]
    async fn persisted_access_token_survives_restart_without_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_path_buf();
        let expires_at_ms = chrono::Utc::now().timestamp_millis() + 3_600_000;

        {
            let manager = CodexOAuthManager::new(path.clone());
            manager
                .add_account_internal(
                    "acc-cached".to_string(),
                    "refresh-should-not-be-used".to_string(),
                    Some("cached@example.com".to_string()),
                    Some("persisted-access".to_string()),
                    Some(expires_at_ms),
                )
                .await
                .unwrap();
        }

        let manager = CodexOAuthManager::new_with_oauth_token_url(
            path,
            "http://127.0.0.1:9/oauth/token".to_string(),
        );
        let token = manager
            .get_valid_token_for_account("acc-cached")
            .await
            .unwrap();

        assert_eq!(token, "persisted-access");
    }

    #[tokio::test]
    async fn token_refresh_preserves_credential_generation_for_valid_default_account() {
        let temp = tempfile::tempdir().unwrap();
        let token_url = spawn_single_refresh_endpoint(
            200,
            r#"{"access_token":"fresh-access","refresh_token":"fresh-refresh","expires_in":3600}"#,
        )
        .await;
        let manager =
            CodexOAuthManager::new_with_oauth_token_url(temp.path().to_path_buf(), token_url);

        manager
            .add_account_internal(
                "acc-valid".to_string(),
                "old-refresh".to_string(),
                Some("valid@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();
        let generation_before = manager.accounts.read().await["acc-valid"].credential_generation;
        assert_eq!(generation_before, 1);

        let token = manager
            .get_valid_token_for_account("acc-valid")
            .await
            .unwrap();
        assert_eq!(
            manager.accounts.read().await["acc-valid"].credential_generation,
            generation_before
        );
        let status = manager.get_status().await;

        assert!(status.authenticated);
        assert_eq!(status.accounts.len(), 1);
        assert_eq!(token, "fresh-access");

        let accounts = manager.accounts.read().await;
        assert_eq!(
            accounts
                .get("acc-valid")
                .map(|account| account.refresh_token.as_str()),
            Some("fresh-refresh")
        );
    }

    #[tokio::test]
    async fn token_request_reloads_rotated_refresh_token_from_disk_before_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let token_url = spawn_rotating_refresh_endpoint().await;
        let manager_one = CodexOAuthManager::new_with_oauth_token_url(
            temp.path().to_path_buf(),
            token_url.clone(),
        );

        manager_one
            .add_account_internal(
                "acc-valid".to_string(),
                "old-refresh".to_string(),
                Some("valid@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        let manager_two =
            CodexOAuthManager::new_with_oauth_token_url(temp.path().to_path_buf(), token_url);

        assert_eq!(
            manager_one
                .get_valid_token_for_account("acc-valid")
                .await
                .unwrap(),
            "access-one"
        );
        assert_eq!(
            manager_two
                .get_valid_token_for_account("acc-valid")
                .await
                .unwrap(),
            "access-two",
            "second manager should reload the rotated refresh token from disk before refreshing"
        );

        let status = manager_two.get_status().await;
        assert!(status.authenticated);
        assert_eq!(status.accounts.len(), 1);
    }

    #[tokio::test]
    async fn invalid_grant_retries_new_token_rotated_by_another_process() {
        let temp = tempfile::tempdir().unwrap();
        let storage_path = temp.path().join("codex_oauth_auth.json");
        let token_url = spawn_cross_process_rotation_endpoint(storage_path).await;
        let manager =
            CodexOAuthManager::new_with_oauth_token_url(temp.path().to_path_buf(), token_url);
        manager
            .add_account_internal(
                "acc-race".to_string(),
                "old-refresh".to_string(),
                Some("race@example.com".to_string()),
                None,
                None,
            )
            .await
            .unwrap();

        let token = manager
            .get_valid_token_for_account("acc-race")
            .await
            .expect("rotated token should recover the refresh race");
        let status = manager.get_status().await;

        assert_eq!(token, "race-access");
        assert!(status.authenticated);
        assert!(status.auth_error.is_none());
    }
}
