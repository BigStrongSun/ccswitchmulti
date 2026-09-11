//! Codex OAuth Tauri Commands
//!
//! 提供 OpenAI ChatGPT Plus/Pro OAuth 认证相关的 Tauri 命令。
//!
//! 大部分认证命令通过通用 `auth_*` 命令（参见 `commands::auth`）暴露给前端，
//! 此处定义 State wrapper 以及 Codex OAuth 专属的订阅额度和模型列表查询命令。

use crate::proxy::providers::codex_oauth_auth::{
    CodexAccountPoolPolicy, CodexOAuthError, CodexOAuthManager,
};
use crate::services::model_fetch::FetchedModel;
use crate::services::subscription::{query_codex_quota, CredentialStatus, SubscriptionQuota};
use crate::store::AppState;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexAccountPoolQuotaStatus {
    account_id: String,
    remaining_percent: Option<f64>,
    queried_at: Option<i64>,
    error: Option<String>,
}

fn quota_remaining_percent(quota: &SubscriptionQuota) -> Option<f64> {
    quota
        .success
        .then(|| {
            quota
                .tiers
                .iter()
                .map(|tier| tier.utilization)
                .reduce(f64::max)
                .map(|used| (100.0 - used).clamp(0.0, 100.0))
        })
        .flatten()
}

fn credential_status_for_codex_oauth_error(error: &CodexOAuthError) -> CredentialStatus {
    match error {
        CodexOAuthError::IdentityUpgradeRequired(_) => CredentialStatus::ReauthRequired,
        CodexOAuthError::RefreshTokenInvalid => CredentialStatus::Expired,
        CodexOAuthError::AccountNotFound(_) => CredentialStatus::NotFound,
        CodexOAuthError::ParseError(_) | CodexOAuthError::IoError(_) => {
            CredentialStatus::ParseError
        }
        CodexOAuthError::AuthorizationPending
        | CodexOAuthError::AccessDenied
        | CodexOAuthError::ExpiredToken
        | CodexOAuthError::TokenFetchFailed(_)
        | CodexOAuthError::DuplicateAccount
        | CodexOAuthError::NetworkError(_) => CredentialStatus::Valid,
    }
}

/// Codex OAuth 认证状态
pub struct CodexOAuthState(pub Arc<RwLock<CodexOAuthManager>>);

/// 查询 Codex OAuth (ChatGPT Plus/Pro) 订阅额度
///
/// - `account_id` 未指定时回退到 `CodexOAuthManager` 的默认账号
/// - 没有任何账号时返回 `not_found`，前端 `SubscriptionQuotaView` 会静默不渲染
/// - 复用 `services::subscription::query_codex_quota`，因此 wham/usage 端点协议
///   与 Codex CLI 路径完全一致
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codex_oauth_quota(
    account_id: Option<String>,
    state: State<'_, CodexOAuthState>,
) -> Result<SubscriptionQuota, String> {
    let manager = state.0.read().await;

    // 解析最终使用的账号 ID：显式 > 默认账号 > 无账号 (not_found)
    let resolved = match account_id {
        Some(id) => Some(id),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Ok(SubscriptionQuota::not_found("codex_oauth"));
    };

    // 获取（必要时自动刷新）access_token
    let (token, workspace_id) = match manager.get_valid_token_and_workspace_for_account(&id).await {
        Ok(credentials) => credentials,
        Err(e) => {
            let credential_status = credential_status_for_codex_oauth_error(&e);
            return Ok(SubscriptionQuota::error(
                "codex_oauth",
                credential_status,
                format!("Codex OAuth token unavailable: {e}"),
            ));
        }
    };

    // 瞬时传输失败以 Err 传播（前端 reject → retry + 保留上次成功值）。
    query_codex_quota(
        &token,
        Some(&workspace_id),
        "codex_oauth",
        "Codex OAuth access token expired or rejected. Please re-login via cc-switch.",
    )
    .await
}

#[tauri::command]
pub async fn get_codex_account_pool_policy(
    state: State<'_, CodexOAuthState>,
) -> Result<CodexAccountPoolPolicy, String> {
    Ok(state.0.read().await.account_pool_policy().await)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn set_codex_account_pool_policy(
    policy: CodexAccountPoolPolicy,
    state: State<'_, CodexOAuthState>,
    app_state: State<'_, AppState>,
) -> Result<crate::services::proxy::CodexAuthFacadeReprojectionOutcome, String> {
    let manager = state.0.read().await;
    manager
        .set_account_pool_policy(policy)
        .await
        .map_err(|error| error.to_string())?;
    let normalized = manager.account_pool_policy().await;
    drop(manager);

    app_state
        .proxy_service
        .reproject_current_codex_auth_facade_for_pool_policy(&normalized)
        .map_err(|error| format!("账号池策略已保存，但当前 Codex Router 门面更新失败: {error}"))
}

#[tauri::command]
pub async fn refresh_codex_account_pool_quota(
    state: State<'_, CodexOAuthState>,
) -> Result<Vec<CodexAccountPoolQuotaStatus>, String> {
    use crate::proxy::providers::codex_oauth_auth::NATIVE_CODEX_ACCOUNT_ID;

    let manager = state.0.read().await;
    let policy = manager.account_pool_policy().await;
    let mut result = Vec::new();
    for entry in policy.entries.into_iter().filter(|entry| entry.enabled) {
        let quota = if entry.account_id == NATIVE_CODEX_ACCOUNT_ID {
            crate::services::subscription::get_subscription_quota("codex").await
        } else {
            match manager
                .get_valid_token_and_workspace_for_account(&entry.account_id)
                .await
            {
                Ok((token, workspace_id)) => {
                    query_codex_quota(
                        &token,
                        Some(&workspace_id),
                        "codex_oauth",
                        "Codex OAuth access token expired or rejected.",
                    )
                    .await
                }
                Err(error) => {
                    result.push(CodexAccountPoolQuotaStatus {
                        account_id: entry.account_id,
                        remaining_percent: None,
                        queried_at: None,
                        error: Some(error.to_string()),
                    });
                    continue;
                }
            }
        };
        match quota {
            Ok(quota) => {
                let remaining = quota_remaining_percent(&quota);
                if let Some(value) = remaining {
                    manager
                        .record_pool_remaining_percent(&entry.account_id, value)
                        .await;
                }
                result.push(CodexAccountPoolQuotaStatus {
                    account_id: entry.account_id,
                    remaining_percent: remaining,
                    queried_at: quota.queried_at,
                    error: quota.error,
                });
            }
            Err(error) => result.push(CodexAccountPoolQuotaStatus {
                account_id: entry.account_id,
                remaining_percent: None,
                queried_at: None,
                error: Some(error),
            }),
        }
    }
    Ok(result)
}

/// 获取 Codex OAuth (ChatGPT Plus/Pro) 可用模型列表
///
/// ChatGPT Codex 反代使用 `chatgpt.com/backend-api/codex/*`，不是 OpenAI 兼容
/// `/v1/models`。这里复用托管 OAuth 账号的 access_token，直接读取 Codex 后端
/// 暴露的模型列表端点。
#[tauri::command(rename_all = "camelCase")]
pub async fn get_codex_oauth_models(
    account_id: Option<String>,
    state: State<'_, CodexOAuthState>,
) -> Result<Vec<FetchedModel>, String> {
    let manager = state.0.read().await;
    let resolved = match account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => Some(id.to_string()),
        None => manager.default_account_id().await,
    };
    let Some(id) = resolved else {
        return Err("No ChatGPT account available".to_string());
    };

    let (token, workspace_id) = manager
        .get_valid_token_and_workspace_for_account(&id)
        .await
        .map_err(|e| format!("Codex OAuth token unavailable: {e}"))?;

    crate::services::codex_oauth_models::fetch_models_with_token(&token, &workspace_id).await
}

/// 获取无需 CCSM OAuth 的 Codex 官方备用模型目录。
///
/// 优先刷新 OpenAI/Codex 公共 catalog；失败时读取独立公共缓存、本机可信官方
/// cache/backup、当前 Codex bundled catalog 与 CCSM 随包基线。
#[tauri::command]
pub async fn get_codex_official_fallback_models() -> Result<Vec<FetchedModel>, String> {
    crate::services::codex_oauth_models::fetch_official_fallback_models().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_upgrade_is_not_reported_as_an_expired_credential() {
        let status = credential_status_for_codex_oauth_error(
            &CodexOAuthError::IdentityUpgradeRequired("legacy-account".to_string()),
        );

        assert!(matches!(status, CredentialStatus::ReauthRequired));
    }

    #[test]
    fn definitively_invalid_refresh_token_is_reported_as_expired() {
        let status = credential_status_for_codex_oauth_error(&CodexOAuthError::RefreshTokenInvalid);

        assert!(matches!(status, CredentialStatus::Expired));
    }

    #[test]
    fn device_authorization_expiry_is_not_reported_as_credential_expiry() {
        let status = credential_status_for_codex_oauth_error(&CodexOAuthError::ExpiredToken);

        assert!(matches!(status, CredentialStatus::Valid));
    }
}
