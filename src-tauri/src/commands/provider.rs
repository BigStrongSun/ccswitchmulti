use indexmap::IndexMap;
use tauri::{Emitter, Manager, State};

use crate::app_config::AppType;
use crate::commands::copilot::CopilotAuthState;
use crate::commands::xai_oauth::XaiOAuthState;
use crate::error::AppError;
use crate::provider::{ClaudeDesktopMode, Provider};
use crate::services::{
    EndpointLatency, ProviderService, ProviderSortUpdate, SpeedtestService, SwitchResult,
};
use crate::store::AppState;
use sha2::{Digest, Sha256};
use std::{future::Future, str::FromStr};

const CODEX_OFFICIAL_PROVIDER_ID: &str = "codex-official";

/// 一键切回 Codex 官方链路后的结构化结果。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexOfficialRestoreOutcome {
    pub official_provider_id: String,
    pub switch_warnings: Vec<String>,
    pub history: crate::codex_history_migration::CodexHistoryProviderBucketMigrationOutcome,
}

// 常量定义
const TEMPLATE_TYPE_GITHUB_COPILOT: &str = "github_copilot";
const TEMPLATE_TYPE_TOKEN_PLAN: &str = "token_plan";
const TEMPLATE_TYPE_BALANCE: &str = "balance";
const TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION: &str = "official_subscription";
const COPILOT_UNIT_PREMIUM: &str = "requests";

/// 获取所有供应商
#[tauri::command]
pub fn get_providers(
    state: State<'_, AppState>,
    app: String,
) -> Result<IndexMap<String, Provider>, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::list(state.inner(), app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_current_provider(state: State<'_, AppState>, app: String) -> Result<String, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::current(state.inner(), app_type).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_codex_logical_provider_for_editing(
    state: State<'_, AppState>,
    providerId: String,
) -> Result<Provider, String> {
    get_codex_logical_provider_for_editing_internal(state.inner(), &providerId)
        .map_err(|error| error.to_string())
}

fn get_codex_logical_provider_for_editing_internal(
    state: &AppState,
    provider_id: &str,
) -> Result<Provider, AppError> {
    let providers = state
        .db
        .get_all_providers(AppType::Codex.as_str())?
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    let provider = providers.get(provider_id).cloned().ok_or_else(|| {
        AppError::InvalidInput(format!("codex_provider_not_found: {provider_id}"))
    })?;
    match provider
        .settings_config
        .pointer("/codexProtocolSet/role")
        .and_then(serde_json::Value::as_str)
    {
        Some("facade") => crate::codex_multirouter::provider_set::restore_logical_codex_provider(
            &provider, &providers,
        )
        .map_err(|error| AppError::InvalidInput(error.to_string())),
        Some("leaf") => Err(AppError::InvalidInput(
            "codex_provider_set_generated_leaf_not_editable".to_string(),
        )),
        _ => Ok(provider),
    }
}

#[tauri::command]
pub async fn add_provider(
    state: State<'_, AppState>,
    app: String,
    provider: Provider,
    #[allow(non_snake_case)] addToLive: Option<bool>,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    add_provider_internal(state.inner(), app_type, provider, addToLive.unwrap_or(true))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_provider(
    state: State<'_, AppState>,
    app: String,
    provider: Provider,
    #[allow(non_snake_case)] originalId: Option<String>,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    update_provider_internal(state.inner(), app_type, originalId.as_deref(), provider)
        .await
        .map_err(|e| e.to_string())
}

async fn add_provider_internal(
    state: &AppState,
    app_type: AppType,
    provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    add_provider_internal_with_probe(state, app_type, provider, add_to_live, |provider| {
        crate::commands::protocol_compatibility::automatic_codex_provider_preflight(state, provider)
    })
    .await
}

async fn update_provider_internal(
    state: &AppState,
    app_type: AppType,
    original_id: Option<&str>,
    provider: Provider,
) -> Result<bool, AppError> {
    update_provider_internal_with_probe(state, app_type, original_id, provider, |provider| {
        crate::commands::protocol_compatibility::automatic_codex_provider_preflight(state, provider)
    })
    .await
}

async fn add_provider_internal_with_probe<F, Fut>(
    state: &AppState,
    app_type: AppType,
    provider: Provider,
    add_to_live: bool,
    probe: F,
) -> Result<bool, AppError>
where
    F: FnOnce(Provider) -> Fut,
    Fut: Future<
        Output = Result<
            (
                Provider,
                Vec<crate::protocol_compatibility::ProtocolCompatibilityRecord>,
            ),
            String,
        >,
    >,
{
    if app_type != AppType::Codex {
        return ProviderService::add(state, app_type, provider, add_to_live);
    }

    let provider = ProviderService::prepare_provider_for_mutation(state, &app_type, provider)?;
    let (provider, record) = resolve_automatic_probe_outcome(provider, probe).await?;
    ProviderService::add_with_protocol_profiles(state, app_type, provider, add_to_live, &record)
}

async fn update_provider_internal_with_probe<F, Fut>(
    state: &AppState,
    app_type: AppType,
    original_id: Option<&str>,
    provider: Provider,
    probe: F,
) -> Result<bool, AppError>
where
    F: FnOnce(Provider) -> Fut,
    Fut: Future<
        Output = Result<
            (
                Provider,
                Vec<crate::protocol_compatibility::ProtocolCompatibilityRecord>,
            ),
            String,
        >,
    >,
{
    if app_type != AppType::Codex {
        return ProviderService::update(state, app_type, original_id, provider);
    }

    let provider = ProviderService::prepare_provider_for_mutation(state, &app_type, provider)?;
    if original_id.is_some_and(|original_id| original_id != provider.id) {
        return Err(AppError::Message(
            "Only additive-mode providers support changing provider key".to_string(),
        ));
    }
    let (provider, record) = resolve_automatic_probe_outcome(provider, probe).await?;
    ProviderService::update_with_protocol_profiles(state, app_type, original_id, provider, &record)
}

async fn resolve_automatic_probe_outcome<F, Fut>(
    provider: Provider,
    probe: F,
) -> Result<
    (
        Provider,
        Vec<crate::protocol_compatibility::ProtocolCompatibilityRecord>,
    ),
    AppError,
>
where
    F: FnOnce(Provider) -> Fut,
    Fut: Future<
        Output = Result<
            (
                Provider,
                Vec<crate::protocol_compatibility::ProtocolCompatibilityRecord>,
            ),
            String,
        >,
    >,
{
    if provider.uses_manual_codex_protocol() {
        return Ok((provider, Vec::new()));
    }
    match probe(provider.clone()).await {
        Ok(outcome) => Ok(outcome),
        Err(error) => Err(AppError::Message(error)),
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn update_codex_subagent_v2(
    state: State<'_, AppState>,
    providerId: String,
    subagentV2: serde_json::Value,
) -> Result<crate::services::CodexSubagentV2MutationResult, String> {
    ProviderService::update_codex_subagent_v2(state.inner(), &providerId, subagentV2)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn initialize_codex_subagent_v2(
    state: State<'_, AppState>,
    providerId: String,
) -> Result<crate::services::CodexSubagentV2MutationResult, String> {
    ProviderService::initialize_codex_subagent_v2(state.inner(), &providerId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn reconcile_codex_subagent_v2_profiles(
    state: State<'_, AppState>,
    providerId: String,
    action: crate::codex_config::CodexSubagentV2ReconcileAction,
    subagentV2: Option<serde_json::Value>,
) -> Result<crate::services::CodexSubagentV2MutationResult, String> {
    ProviderService::reconcile_codex_subagent_v2_profiles(
        state.inner(),
        &providerId,
        action,
        subagentV2,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn inspect_codex_multirouter_projection(
    state: State<'_, AppState>,
    providerId: String,
) -> Result<crate::codex_multirouter::projection::CodexRoutingProjectionStatus, String> {
    crate::codex_multirouter::projection::inspect_codex_multirouter_projection(
        &state.db,
        &providerId,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn inspect_active_codex_multirouter_projection(
    state: State<'_, AppState>,
) -> Result<Option<crate::codex_multirouter::projection::CodexRoutingProjectionStatus>, String> {
    crate::codex_multirouter::projection::inspect_active_codex_multirouter_projection(&state.db)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn retry_codex_multirouter_projection(
    state: State<'_, AppState>,
    providerId: String,
) -> Result<crate::codex_multirouter::projection::CodexRoutingProjectionStatus, String> {
    crate::codex_multirouter::projection::ensure_codex_multirouter_projection(
        &state.db,
        &providerId,
        true,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn get_codex_multirouter_revision(
    state: State<'_, AppState>,
    providerId: String,
) -> Result<String, String> {
    crate::codex_multirouter::migration::codex_multirouter_revision(&state.db, &providerId)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn preview_codex_multirouter_migration(
    state: State<'_, AppState>,
    providerId: String,
    expectedRevision: String,
) -> Result<crate::codex_multirouter::migration::CodexMultiRouterMigrationPreview, String> {
    crate::codex_multirouter::migration::preview_codex_multirouter_migration(
        &state.db,
        &providerId,
        &expectedRevision,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn apply_codex_multirouter_migration(
    state: State<'_, AppState>,
    providerId: String,
    expectedRevision: String,
    planToken: String,
) -> Result<crate::codex_multirouter::migration::CodexMultiRouterMigrationApplyOutcome, String> {
    let outcome = crate::codex_multirouter::migration::apply_codex_multirouter_migration(
        &state.db,
        &providerId,
        &expectedRevision,
        &planToken,
    )
    .map_err(|error| error.to_string())?;
    if crate::codex_multirouter::active_codex_router_id(&state.db)
        .map_err(|error| error.to_string())?
        .as_deref()
        == Some(providerId.as_str())
    {
        crate::codex_multirouter::projection::ensure_codex_multirouter_projection(
            &state.db,
            &providerId,
            false,
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(outcome)
}

#[tauri::command]
pub fn delete_provider(
    state: State<'_, AppState>,
    app: String,
    id: String,
) -> Result<crate::codex_multirouter::mutation::CodexProviderDeleteOutcome, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::delete(state.inner(), app_type, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_provider_from_live_config(
    state: tauri::State<'_, AppState>,
    app: String,
    id: String,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::remove_from_live_config(state.inner(), app_type, &id)
        .map(|_| true)
        .map_err(|e| e.to_string())
}

fn switch_provider_internal(
    state: &AppState,
    app_type: AppType,
    id: &str,
) -> Result<SwitchResult, AppError> {
    ProviderService::switch(state, app_type, id)
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn switch_provider_test_hook(
    state: &AppState,
    app_type: AppType,
    id: &str,
) -> Result<SwitchResult, AppError> {
    switch_provider_internal(state, app_type, id)
}

#[tauri::command]
pub async fn switch_provider(
    app_handle: tauri::AppHandle,
    app: String,
    id: String,
) -> Result<SwitchResult, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle
            .try_state::<AppState>()
            .ok_or_else(|| "应用状态不可用".to_string())?;
        switch_provider_internal(state.inner(), app_type, &id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("供应商切换任务执行失败: {e}"))?
}

/// Explicitly repair legacy Codex configuration and retry the requested provider switch.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn force_repair_and_switch_codex_provider(
    app_handle: tauri::AppHandle,
    providerId: String,
) -> Result<crate::services::CodexForceRepairOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle
            .try_state::<AppState>()
            .ok_or_else(|| "应用状态不可用".to_string())?;
        ProviderService::force_repair_and_switch_codex_provider(state.inner(), &providerId)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Codex 强制覆盖任务执行失败: {error}"))?
}

/// 一键退出 Codex 接管、切回内建 OpenAI，并尽量把全部历史归并到 `openai` 桶。
///
/// 官方切换复用既有链路以保留 OAuth `auth.json`。历史归桶仍要求
/// Codex/ChatGPT App 完全退出；如果 App 还在运行，命令会先完成官方切换，
/// 再用 warning 告知历史修复被跳过，避免用户被留在本地 15721 代理上。
#[tauri::command]
pub fn switch_codex_to_official_and_repair_history(
    state: State<'_, AppState>,
) -> Result<CodexOfficialRestoreOutcome, String> {
    state
        .db
        .ensure_official_seed_by_id(CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex)
        .map_err(|e| e.to_string())?;
    let previous_settings = crate::settings::get_settings();
    crate::settings::disable_codex_session_history_unify().map_err(|e| e.to_string())?;

    let switch_result =
        match switch_provider_internal(&state, AppType::Codex, CODEX_OFFICIAL_PROVIDER_ID) {
            Ok(result) => result,
            Err(error) => {
                // provider 尚未切换成功时恢复统一历史开关，避免一次失败点击永久改变用户设置。
                if let Err(restore_error) = crate::settings::update_settings(previous_settings) {
                    return Err(format!(
                        "切回 OpenAI 官方失败: {error}; 恢复原设置也失败: {restore_error}"
                    ));
                }
                return Err(error.to_string());
            }
        };
    crate::codex_config::force_codex_builtin_openai_live_provider().map_err(|e| e.to_string())?;
    let mut switch_warnings = switch_result.warnings;
    let history =
        match crate::codex_history_migration::sync_all_codex_history_provider_buckets_to_openai() {
            Ok(history) => history,
            Err(error) => {
                let message = error.to_string();
                switch_warnings.push(format!(
                "已切回 OpenAI 官方；历史修复未完成，请完全退出 Codex/ChatGPT App 后再执行历史修复。原因: {message}"
            ));
                crate::codex_history_migration::CodexHistoryProviderBucketMigrationOutcome {
                    skipped_reason: Some(
                        "history_repair_skipped_after_official_switch".to_string(),
                    ),
                    ..Default::default()
                }
            }
        };

    Ok(CodexOfficialRestoreOutcome {
        official_provider_id: CODEX_OFFICIAL_PROVIDER_ID.to_string(),
        switch_warnings,
        history,
    })
}

fn import_default_config_internal(state: &AppState, app_type: AppType) -> Result<bool, AppError> {
    if matches!(app_type, AppType::GrokBuild) {
        // 官方登录态（live 语法合法且无自定义模型表）+ 用户手动导入：
        // 导入的正确结果是让 Grok Official 成为当前供应商，而非报错。
        // 只挂在命令层 = 只有手动动作可达；启动自动导入走 service 层、
        // 官方态照旧报错静默跳过，删掉的官方条目不会被重启复活
        //（全项目惯例：启动自动导入只产出 default，从不产出官方条目）。
        if let Ok(settings) = crate::grok_config::read_grok_live_settings() {
            let config = settings
                .get("config")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if crate::grok_config::is_official_live_config(config) {
                state.db.ensure_official_seed_by_id(
                    crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID,
                    AppType::GrokBuild,
                )?;
                state.db.set_current_provider(
                    app_type.as_str(),
                    crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID,
                )?;
                crate::settings::set_current_provider(
                    &app_type,
                    Some(crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID),
                )?;
                return Ok(true);
            }
        }

        // Safety net: 与 claude-desktop 导入同语义 —— 用户主动点导入是"重新
        // 整理该表"的隐式信号，把官方入口补回来。覆盖导入必然失败的场景
        //（live 文件缺失 / TOML 语法错误 / 残缺的自定义配置），避免
        // "报错 + 空列表"死胡同。失败只 warn，不影响导入主流程。
        if let Err(e) = state.db.ensure_official_seed_by_id(
            crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID,
            AppType::GrokBuild,
        ) {
            log::warn!("Failed to ensure grokbuild-official seed during import: {e}");
        }
    }

    let imported = ProviderService::import_default_config(state, app_type.clone())?;

    if imported {
        // Extract common config snippet (mirrors old startup logic in lib.rs)
        if state
            .db
            .should_auto_extract_config_snippet(app_type.as_str())?
        {
            match ProviderService::extract_common_config_snippet(state, app_type.clone()) {
                Ok(snippet) if !snippet.is_empty() && snippet != "{}" => {
                    let _ = state
                        .db
                        .set_config_snippet(app_type.as_str(), Some(snippet));
                    let _ = state
                        .db
                        .set_config_snippet_cleared(app_type.as_str(), false);
                }
                _ => {}
            }
        }

        ProviderService::migrate_legacy_common_config_usage_if_needed(state, app_type.clone())?;
    }

    Ok(imported)
}

#[cfg_attr(not(feature = "test-hooks"), doc(hidden))]
pub fn import_default_config_test_hook(
    state: &AppState,
    app_type: AppType,
) -> Result<bool, AppError> {
    import_default_config_internal(state, app_type)
}

#[tauri::command]
pub fn import_default_config(state: State<'_, AppState>, app: String) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    import_default_config_internal(&state, app_type).map_err(Into::into)
}

#[tauri::command]
pub async fn get_claude_desktop_status(
    state: State<'_, AppState>,
) -> Result<crate::claude_desktop_config::ClaudeDesktopStatus, String> {
    let proxy_running = state.proxy_service.is_running().await;
    crate::claude_desktop_config::get_status(state.db.as_ref(), proxy_running)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_claude_desktop_default_routes(
) -> Vec<crate::claude_desktop_config::ClaudeDesktopDefaultRoute> {
    crate::claude_desktop_config::default_proxy_routes()
}

#[tauri::command]
pub fn import_claude_desktop_providers_from_claude(
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let claude_providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .map_err(|e| e.to_string())?;
    let existing_ids = state
        .db
        .get_provider_ids(AppType::ClaudeDesktop.as_str())
        .map_err(|e| e.to_string())?;

    let mut imported = 0usize;
    for provider in claude_providers.values() {
        if existing_ids.contains(&provider.id) {
            continue;
        }

        let mut desktop_provider = provider.clone();
        desktop_provider.in_failover_queue = false;
        let meta = desktop_provider.meta.get_or_insert_with(Default::default);

        if crate::claude_desktop_config::is_compatible_direct_provider(provider)
            && claude_provider_models_are_claude_safe(provider)
        {
            meta.claude_desktop_mode = Some(ClaudeDesktopMode::Direct);
        } else if let Some(routes) = suggested_claude_desktop_routes(provider) {
            meta.claude_desktop_mode = Some(ClaudeDesktopMode::Proxy);
            meta.claude_desktop_model_routes = routes;
        } else {
            continue;
        }

        state
            .db
            .save_provider(AppType::ClaudeDesktop.as_str(), &desktop_provider)
            .map_err(|e| e.to_string())?;
        imported += 1;
    }

    // Safety net: 用户可能手动删除过 claude-desktop-official seed。
    // 用户主动点 import 是"重新整理 ClaudeDesktop 表"的隐式信号，把官方入口补回来。
    // 失败只 warn，不影响 imported 主流程；imported 计数语义保持纯净。
    if let Err(e) = state.db.ensure_official_seed_by_id(
        crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
        AppType::ClaudeDesktop,
    ) {
        log::warn!("Failed to ensure claude-desktop-official seed during import: {e}");
    }

    Ok(imported)
}

#[tauri::command]
pub fn ensure_claude_desktop_official_provider(state: State<'_, AppState>) -> Result<bool, String> {
    state
        .db
        .ensure_official_seed_by_id(
            crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID,
            AppType::ClaudeDesktop,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ensure_codex_official_provider(state: State<'_, AppState>) -> Result<bool, String> {
    state
        .db
        .ensure_official_seed_by_id(crate::database::CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ensure_grokbuild_official_provider(state: State<'_, AppState>) -> Result<bool, String> {
    state
        .db
        .ensure_official_seed_by_id(
            crate::database::GROKBUILD_OFFICIAL_PROVIDER_ID,
            AppType::GrokBuild,
        )
        .map_err(|e| e.to_string())
}

fn claude_provider_models_are_claude_safe(provider: &Provider) -> bool {
    let Some(env) = provider
        .settings_config
        .get("env")
        .and_then(|value| value.as_object())
    else {
        return true;
    };

    [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
    ]
    .into_iter()
    .filter_map(|key| env.get(key).and_then(|value| value.as_str()))
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .all(crate::claude_desktop_config::is_claude_safe_model_id)
}

pub(crate) fn suggested_claude_desktop_routes(
    provider: &Provider,
) -> Option<std::collections::HashMap<String, crate::provider::ClaudeDesktopModelRoute>> {
    let env = provider
        .settings_config
        .get("env")
        .and_then(|value| value.as_object())?;
    let mut routes = std::collections::HashMap::new();
    let supports_1m_default = !matches!(
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref()),
        Some("github_copilot") | Some("codex_oauth") | Some("xai_oauth")
    );

    fn add_route(
        routes: &mut std::collections::HashMap<String, crate::provider::ClaudeDesktopModelRoute>,
        env: &serde_json::Map<String, serde_json::Value>,
        route_key: &str,
        env_key: &str,
        supports_1m_default: bool,
    ) {
        let Some(raw_model) = env
            .get(env_key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return;
        };

        // Claude 端 env 值可能带 [1M] 后缀；Claude Desktop schema 不接受后缀，
        // 改用 supports1m 字段表达 1M 能力。在 import 边界做单向翻译。
        let marker = crate::claude_desktop_config::ONE_M_CONTEXT_MARKER.as_bytes();
        let raw_bytes = raw_model.as_bytes();
        let has_1m_marker = raw_bytes.len() >= marker.len()
            && raw_bytes[raw_bytes.len() - marker.len()..].eq_ignore_ascii_case(marker);
        let stripped_model: &str = if has_1m_marker {
            raw_model[..raw_model.len() - marker.len()].trim_end()
        } else {
            raw_model
        };
        if stripped_model.is_empty() {
            return;
        }
        let effective_supports_1m = supports_1m_default || has_1m_marker;
        let explicit_label_override = env
            .get(&format!("{env_key}_NAME"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let label_override = explicit_label_override.clone().or_else(|| {
            (!crate::claude_desktop_config::is_claude_safe_model_id(stripped_model))
                .then(|| stripped_model.to_string())
        });

        // 何时覆盖既有 label_override：原本为空 / 这次来的是 explicit _NAME /
        // 既有值只是 stripped_model 派生的占位（被 explicit 或更具体的值挤掉）。
        let should_overwrite = |existing: Option<&str>| {
            existing.is_none()
                || explicit_label_override.is_some()
                || existing == Some(stripped_model)
        };

        let merge_into = |existing: &mut crate::provider::ClaudeDesktopModelRoute| {
            let merged = existing.supports_1m.unwrap_or(false) || effective_supports_1m;
            existing.supports_1m = Some(merged);
            if should_overwrite(existing.label_override.as_deref()) {
                existing.label_override = label_override.clone();
            }
        };

        if let Some(existing) = routes
            .values_mut()
            .find(|existing| existing.model == stripped_model)
        {
            merge_into(existing);
            return;
        }

        routes
            .entry(route_key.to_string())
            .and_modify(merge_into)
            .or_insert_with(|| crate::provider::ClaudeDesktopModelRoute {
                model: stripped_model.to_string(),
                label_override,
                supports_1m: Some(effective_supports_1m),
            });
    }

    for spec in crate::claude_desktop_config::DEFAULT_PROXY_ROUTES {
        add_route(
            &mut routes,
            env,
            spec.route_id,
            spec.env_key,
            supports_1m_default,
        );
    }

    // 三个 default env_key 全空时用 ANTHROPIC_MODEL 派生兜底路由。
    if routes.is_empty() {
        let primary_route = crate::claude_desktop_config::DEFAULT_PROXY_ROUTES[0].route_id;
        add_route(
            &mut routes,
            env,
            primary_route,
            "ANTHROPIC_MODEL",
            supports_1m_default,
        );
    }

    (!routes.is_empty()).then_some(routes)
}

#[allow(non_snake_case)]
#[tauri::command]
pub async fn queryProviderUsage(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    xai_state: State<'_, XaiOAuthState>,
    #[allow(non_snake_case)] providerId: String, // 使用 camelCase 匹配前端
    app: String,
) -> Result<crate::provider::UsageResult, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    // inner 可能以两种形式失败：
    //   1) 返回 Ok(UsageResult { success: false, .. }) —— 确定性失败（401、脚本
    //      报错、未知供应商等）。写进 UsageCache 并刷新托盘，让
    //      format_script_summary 的 success 守卫生效、suffix 自然消失。
    //   2) 返回 Err(String) —— 瞬时传输失败（网络/超时）及 DB/Copilot fetch 等。
    //      不写失败快照、不 emit：保留上一份托盘快照，与前端 react-query reject
    //      保留上次 data 的语义一致；否则失败快照会经 useUsageCacheBridge 盲写
    //      回 query 缓存，抹掉 reject 本该保留的旧值。
    let inner = query_provider_usage_inner(
        &state,
        &copilot_state,
        &xai_state,
        app_type.clone(),
        &providerId,
    )
    .await;
    if let Ok(snapshot) = &inner {
        let payload = serde_json::json!({
            "kind": "script",
            "appType": app_type.as_str(),
            "providerId": &providerId,
            "data": snapshot,
        });
        if let Err(e) = app_handle.emit("usage-cache-updated", payload) {
            log::error!("emit usage-cache-updated (script) 失败: {e}");
        }
        state
            .usage_cache
            .put_script(app_type, providerId, snapshot.clone());
        crate::tray::schedule_tray_refresh(&app_handle);
    }
    inner
}

/// Resolve `(base_url, api_key)` for native usage queries, delegating to the
/// per-app resolver on `Provider`. Missing provider → empty credentials.
fn resolve_native_credentials(app_type: &AppType, provider: Option<&Provider>) -> (String, String) {
    provider
        .map(|p| p.resolve_usage_credentials(app_type))
        .unwrap_or_default()
}

fn resolve_coding_plan_credentials(
    app_type: &AppType,
    provider: Option<&Provider>,
    usage_script: Option<&crate::provider::UsageScript>,
) -> (String, String) {
    let is_zenmux = usage_script
        .and_then(|s| s.coding_plan_provider.as_deref())
        .map(|provider| provider.eq_ignore_ascii_case("zenmux"))
        .unwrap_or(false);

    if !is_zenmux {
        return resolve_native_credentials(app_type, provider);
    }

    let script_base_url = usage_script
        .and_then(|s| s.base_url.as_deref())
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let script_api_key = usage_script
        .and_then(|s| s.api_key.as_deref())
        .unwrap_or("")
        .to_string();

    if !script_base_url.is_empty() && !script_api_key.is_empty() {
        return (script_base_url, script_api_key);
    }

    let native = resolve_native_credentials(app_type, provider);
    if !native.0.is_empty() && !native.1.is_empty() {
        native
    } else {
        (script_base_url, script_api_key)
    }
}

async fn query_provider_usage_inner(
    state: &AppState,
    copilot_state: &CopilotAuthState,
    xai_state: &XaiOAuthState,
    app_type: AppType,
    provider_id: &str,
) -> Result<crate::provider::UsageResult, String> {
    // 从数据库读取供应商信息，检查特殊模板类型
    let providers = state
        .db
        .get_all_providers(app_type.as_str())
        .map_err(|e| format!("Failed to get providers: {e}"))?;
    let provider = providers.get(provider_id);
    let usage_script = provider
        .and_then(|p| p.meta.as_ref())
        .and_then(|m| m.usage_script.as_ref());
    let template_type = usage_script
        .and_then(|s| s.template_type.as_deref())
        .unwrap_or("");

    // ── GitHub Copilot 专用路径 ──
    if template_type == TEMPLATE_TYPE_GITHUB_COPILOT {
        let copilot_account_id = provider
            .and_then(|p| p.meta.as_ref())
            .and_then(|m| m.managed_account_id_for(TEMPLATE_TYPE_GITHUB_COPILOT));

        let auth_manager = copilot_state.0.read().await;
        let usage = match copilot_account_id.as_deref() {
            Some(account_id) => auth_manager
                .fetch_usage_for_account(account_id)
                .await
                .map_err(|e| format!("Failed to fetch Copilot usage: {e}"))?,
            None => auth_manager
                .fetch_usage()
                .await
                .map_err(|e| format!("Failed to fetch Copilot usage: {e}"))?,
        };
        let premium = &usage.quota_snapshots.premium_interactions;
        let used = premium.entitlement - premium.remaining;

        return Ok(crate::provider::UsageResult {
            success: true,
            data: Some(vec![crate::provider::UsageData {
                plan_name: Some(usage.copilot_plan),
                remaining: Some(premium.remaining as f64),
                total: Some(premium.entitlement as f64),
                used: Some(used as f64),
                unit: Some(COPILOT_UNIT_PREMIUM.to_string()),
                is_valid: Some(true),
                invalid_message: None,
                extra: Some(format!("Reset: {}", usage.quota_reset_date)),
            }]),
            error: None,
        });
    }

    // ── Coding Plan 专用路径 ──
    if template_type == TEMPLATE_TYPE_TOKEN_PLAN {
        let (base_url, api_key) =
            resolve_coding_plan_credentials(&app_type, provider, usage_script);

        // 火山方舟用账号 AK/SK 签名查询用量（存于 usage_script，与推理 api_key 分离）；
        // 其他供应商为 None，service 层沿用 api_key。
        let access_key_id = usage_script.and_then(|s| s.access_key_id.clone());
        let secret_access_key = usage_script.and_then(|s| s.secret_access_key.clone());
        // 智谱团队版：显式 provider 标识 + 组织/项目 ID（与个人版智谱 base_url 相同，
        // 靠 coding_plan_provider == "zhipu_team" 在 service 层路由）。
        let coding_plan_provider = usage_script.and_then(|s| s.coding_plan_provider.clone());
        let team_organization_id = usage_script.and_then(|s| s.team_organization_id.clone());
        let team_project_id = usage_script.and_then(|s| s.team_project_id.clone());

        let quota = crate::services::coding_plan::get_coding_plan_quota(
            &base_url,
            &api_key,
            access_key_id.as_deref(),
            secret_access_key.as_deref(),
            coding_plan_provider.as_deref(),
            team_organization_id.as_deref(),
            team_project_id.as_deref(),
        )
        .await
        .map_err(|e| format!("Failed to query coding plan: {e}"))?;

        // 将 SubscriptionQuota 转换为 UsageResult
        if !quota.success {
            return Ok(crate::provider::UsageResult {
                success: false,
                data: None,
                error: quota.error,
            });
        }

        // ZenMux 的 tier 携带 USD 额度信息，需要编码为 JSON extra
        let has_usd = quota
            .tiers
            .first()
            .map(|t| t.used_value_usd.is_some())
            .unwrap_or(false);
        let plan_label = quota
            .credential_message
            .as_deref()
            .and_then(|msg| msg.split(' ').next())
            .map(|tier| format!("ZenMux·{}", tier.to_uppercase()));
        let mut first_tier = true;

        let data: Vec<crate::provider::UsageData> = quota
            .tiers
            .iter()
            .map(|tier| {
                let total = 100.0;
                let used = tier.utilization;
                let remaining = total - used;
                let extra = if has_usd {
                    let mut extra_json = serde_json::json!({
                        "resetsAt": tier.resets_at,
                    });
                    if let Some(v) = tier.used_value_usd {
                        extra_json["usedValueUsd"] = serde_json::json!(v);
                    }
                    if let Some(v) = tier.max_value_usd {
                        extra_json["maxValueUsd"] = serde_json::json!(v);
                    }
                    if first_tier {
                        if let Some(ref label) = plan_label {
                            extra_json["planLabel"] = serde_json::json!(label);
                        }
                        first_tier = false;
                    }
                    Some(extra_json.to_string())
                } else {
                    tier.resets_at.clone()
                };
                crate::provider::UsageData {
                    plan_name: Some(tier.name.clone()),
                    remaining: Some(remaining),
                    total: Some(total),
                    used: Some(used),
                    unit: Some("%".to_string()),
                    is_valid: Some(true),
                    invalid_message: None,
                    extra,
                }
            })
            .collect();

        return Ok(crate::provider::UsageResult {
            success: true,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        });
    }

    // ── 官方余额查询路径 ──
    if template_type == TEMPLATE_TYPE_BALANCE {
        // 按 app 区分的凭据存储格式提取 Base URL 与 API Key
        let (base_url, api_key) = resolve_native_credentials(&app_type, provider);

        return crate::services::balance::get_balance(&base_url, &api_key)
            .await
            .map_err(|e| format!("Failed to query balance: {e}"));
    }

    // ── 官方订阅额度查询路径 ──
    if template_type == TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION {
        if !usage_script.map(|s| s.enabled).unwrap_or(false) {
            return Ok(crate::provider::UsageResult {
                success: false,
                data: None,
                error: Some("Usage query is disabled".to_string()),
            });
        }

        // xAI OAuth 托管供应商的额度属绑定的 SuperGrok 账号，而非所在 app 的
        // CLI 凭据（对 codex/claude 而言 CLI 凭据是 ChatGPT/Claude 订阅，跨了
        // 订阅体系，查出来的数字张冠李戴）。
        let quota = if provider.map(Provider::is_xai_oauth).unwrap_or(false) {
            let account_id = provider
                .and_then(|p| p.meta.as_ref())
                .and_then(|m| m.managed_account_id_for("xai_oauth"));
            crate::commands::xai_oauth::query_xai_oauth_quota_for(xai_state, account_id).await?
        } else {
            crate::services::subscription::get_subscription_quota(app_type.as_str())
                .await
                .map_err(|e| format!("Failed to query subscription quota: {e}"))?
        };

        if !quota.success {
            return Ok(crate::provider::UsageResult {
                success: false,
                data: None,
                error: quota.error.or(quota.credential_message),
            });
        }

        let data: Vec<crate::provider::UsageData> = quota
            .tiers
            .iter()
            .map(|tier| crate::provider::UsageData {
                plan_name: Some(tier.name.clone()),
                remaining: Some(100.0 - tier.utilization),
                total: Some(100.0),
                used: Some(tier.utilization),
                unit: Some("%".to_string()),
                is_valid: Some(true),
                invalid_message: None,
                extra: tier.resets_at.clone(),
            })
            .collect();

        return Ok(crate::provider::UsageResult {
            success: true,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        });
    }

    // ── 通用 JS 脚本路径 ──
    ProviderService::query_usage(state, app_type, provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[allow(non_snake_case)]
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn testUsageScript(
    state: State<'_, AppState>,
    #[allow(non_snake_case)] providerId: String,
    app: String,
    #[allow(non_snake_case)] scriptCode: String,
    timeout: Option<u64>,
    #[allow(non_snake_case)] apiKey: Option<String>,
    #[allow(non_snake_case)] baseUrl: Option<String>,
    #[allow(non_snake_case)] accessToken: Option<String>,
    #[allow(non_snake_case)] userId: Option<String>,
    #[allow(non_snake_case)] templateType: Option<String>,
) -> Result<crate::provider::UsageResult, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::test_usage_script(
        state.inner(),
        app_type,
        &providerId,
        &scriptCode,
        timeout.unwrap_or(10),
        apiKey.as_deref(),
        baseUrl.as_deref(),
        accessToken.as_deref(),
        userId.as_deref(),
        templateType.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_live_provider_settings(app: String) -> Result<serde_json::Value, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::read_live_settings(app_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_api_endpoints(
    urls: Vec<String>,
    #[allow(non_snake_case)] timeoutSecs: Option<u64>,
) -> Result<Vec<EndpointLatency>, String> {
    SpeedtestService::test_endpoints(urls, timeoutSecs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_custom_endpoints(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
) -> Result<Vec<crate::settings::CustomEndpoint>, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::get_custom_endpoints(state.inner(), app_type, &providerId)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_custom_endpoint(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
    url: String,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::add_custom_endpoint(state.inner(), app_type, &providerId, url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_custom_endpoint(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
    url: String,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::remove_custom_endpoint(state.inner(), app_type, &providerId, url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_endpoint_last_used(
    state: State<'_, AppState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
    url: String,
) -> Result<(), String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::update_endpoint_last_used(state.inner(), app_type, &providerId, url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_providers_sort_order(
    state: State<'_, AppState>,
    app: String,
    updates: Vec<ProviderSortUpdate>,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    ProviderService::update_sort_order(state.inner(), app_type, updates).map_err(|e| e.to_string())
}

use crate::provider::UniversalProvider;
use std::collections::HashMap;
use tauri::AppHandle;

#[derive(Clone, serde::Serialize)]
pub struct UniversalProviderSyncedEvent {
    pub action: String,
    pub id: String,
}

fn emit_universal_provider_synced(app: &AppHandle, action: &str, id: &str) {
    let _ = app.emit(
        "universal-provider-synced",
        UniversalProviderSyncedEvent {
            action: action.to_string(),
            id: id.to_string(),
        },
    );
}

#[tauri::command]
pub fn get_universal_providers(
    state: State<'_, AppState>,
) -> Result<HashMap<String, UniversalProvider>, String> {
    ProviderService::list_universal(state.inner()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_universal_provider(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<UniversalProvider>, String> {
    ProviderService::get_universal(state.inner(), &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: UniversalProvider,
) -> Result<bool, String> {
    let id = provider.id.clone();
    let result =
        ProviderService::upsert_universal(state.inner(), provider).map_err(|e| e.to_string())?;

    emit_universal_provider_synced(&app, "upsert", &id);

    Ok(result)
}

#[tauri::command]
pub fn delete_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let result =
        ProviderService::delete_universal(state.inner(), &id).map_err(|e| e.to_string())?;

    emit_universal_provider_synced(&app, "delete", &id);

    Ok(result)
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareUniversalProviderSetRequest {
    pub provider: crate::provider::UniversalProvider,
    pub receipt_ids: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UniversalProviderSetPreview {
    pub digest: String,
    pub universal_provider_id: String,
    pub codex: Option<crate::codex_multirouter::provider_set::CodexProviderSetPreview>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitUniversalProviderSetRequest {
    pub provider: crate::provider::UniversalProvider,
    pub receipt_ids: Vec<String>,
    pub digest: String,
    pub intent: crate::commands::protocol_compatibility::CodexProviderSetCommitIntent,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UniversalProviderSetCommitOutcome {
    pub preview: UniversalProviderSetPreview,
}

#[tauri::command]
pub fn prepare_universal_provider_set(
    state: State<'_, AppState>,
    request: PrepareUniversalProviderSetRequest,
) -> Result<UniversalProviderSetPreview, String> {
    prepare_universal_provider_set_internal(state.inner(), request, chrono::Utc::now().timestamp())
}

#[tauri::command]
pub fn commit_universal_provider_set(
    state: State<'_, AppState>,
    request: CommitUniversalProviderSetRequest,
) -> Result<UniversalProviderSetCommitOutcome, String> {
    commit_universal_provider_set_internal(state.inner(), request, chrono::Utc::now().timestamp())
}

fn prepare_universal_provider_set_internal(
    state: &AppState,
    request: PrepareUniversalProviderSetRequest,
    now: i64,
) -> Result<UniversalProviderSetPreview, String> {
    let codex_provider =
        ProviderService::prepare_universal_codex_provider_from_definition(state, &request.provider)
            .map_err(|error| error.to_string())?;
    let codex = match codex_provider {
        Some(provider) => Some(
            crate::commands::protocol_compatibility::prepare_codex_provider_set_internal(
                state,
                crate::commands::protocol_compatibility::PrepareCodexProviderSetRequest {
                    provider,
                    receipt_ids: request.receipt_ids.clone(),
                },
                now,
            )?,
        ),
        None => {
            if !request.receipt_ids.is_empty() {
                return Err("codex_provider_set_probe_target_mismatch".to_string());
            }
            None
        }
    };
    let digest = universal_provider_set_digest(state, &request.provider, codex.as_ref())?;
    Ok(UniversalProviderSetPreview {
        digest,
        universal_provider_id: request.provider.id,
        codex,
    })
}

fn commit_universal_provider_set_internal(
    state: &AppState,
    request: CommitUniversalProviderSetRequest,
    now: i64,
) -> Result<UniversalProviderSetCommitOutcome, String> {
    let preview = prepare_universal_provider_set_internal(
        state,
        PrepareUniversalProviderSetRequest {
            provider: request.provider.clone(),
            receipt_ids: request.receipt_ids.clone(),
        },
        now,
    )?;
    if preview.digest != request.digest {
        return Err("codex_provider_set_dependency_changed".to_string());
    }
    let codex_provider =
        ProviderService::prepare_universal_codex_provider_from_definition(state, &request.provider)
            .map_err(|error| error.to_string())?;
    match (codex_provider.as_ref(), preview.codex.as_ref(), request.intent) {
        (None, None, crate::commands::protocol_compatibility::CodexProviderSetCommitIntent::AcceptSingle) => {}
        (Some(provider), Some(codex), intent) if provider.uses_manual_codex_protocol() => {
            if !matches!(
                (&codex.plan, intent),
                (
                    crate::codex_multirouter::provider_set::CodexProviderSetPlan::Single { .. },
                    crate::commands::protocol_compatibility::CodexProviderSetCommitIntent::ConfirmManual
                )
            ) {
                return Err("codex_provider_set_manual_intent_required".to_string());
            }
        }
        (
            Some(_),
            Some(crate::codex_multirouter::provider_set::CodexProviderSetPreview {
                plan: crate::codex_multirouter::provider_set::CodexProviderSetPlan::Single { .. },
                ..
            }),
            crate::commands::protocol_compatibility::CodexProviderSetCommitIntent::AcceptSingle,
        ) => {}
        (
            Some(_),
            Some(crate::codex_multirouter::provider_set::CodexProviderSetPreview {
                plan: crate::codex_multirouter::provider_set::CodexProviderSetPlan::Split { .. },
                ..
            }),
            crate::commands::protocol_compatibility::CodexProviderSetCommitIntent::ConfirmSplit,
        ) => {}
        (
            Some(_),
            Some(crate::codex_multirouter::provider_set::CodexProviderSetPreview {
                plan: crate::codex_multirouter::provider_set::CodexProviderSetPlan::Split { .. },
                ..
            }),
            _,
        ) => return Err("codex_provider_set_split_confirmation_required".to_string()),
        (
            Some(_),
            Some(crate::codex_multirouter::provider_set::CodexProviderSetPreview {
                plan: crate::codex_multirouter::provider_set::CodexProviderSetPlan::Blocked { .. },
                ..
            }),
            _,
        ) => return Err("codex_provider_set_model_blocked".to_string()),
        (Some(_), Some(_), _) => {
            return Err("codex_provider_set_single_intent_required".to_string())
        }
        _ => return Err("codex_provider_set_dependency_changed".to_string()),
    }

    let records = match codex_provider.as_ref() {
        Some(provider) if !provider.uses_manual_codex_protocol() => {
            state.get_codex_provider_set_probe_receipts(&request.receipt_ids)?
        }
        _ => Vec::new(),
    };
    ProviderService::save_and_sync_universal_to_apps_with_codex_profiles(
        state,
        request.provider,
        codex_provider,
        &records,
    )
    .map_err(|error| error.to_string())?;
    if let Err(error) = state.forget_codex_provider_set_probe_receipts(&request.receipt_ids) {
        log::warn!("Universal Provider Set 已提交，但清理一次性探测 receipt 失败：{error}");
    }
    Ok(UniversalProviderSetCommitOutcome { preview })
}

fn universal_provider_set_digest(
    state: &AppState,
    provider: &crate::provider::UniversalProvider,
    codex: Option<&crate::codex_multirouter::provider_set::CodexProviderSetPreview>,
) -> Result<String, String> {
    let existing_definition = state
        .db
        .get_universal_provider(&provider.id)
        .map_err(|error| error.to_string())?;
    let child_ids = [
        (AppType::Claude, format!("universal-claude-{}", provider.id)),
        (AppType::Codex, format!("universal-codex-{}", provider.id)),
        (AppType::Gemini, format!("universal-gemini-{}", provider.id)),
    ];
    let existing_children = child_ids
        .iter()
        .map(|(app_type, provider_id)| {
            state
                .db
                .get_provider_by_id(provider_id, app_type.as_str())
                .map(|child| (app_type.as_str().to_string(), provider_id.clone(), child))
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let current_codex = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .map_err(|error| error.to_string())?;
    let encoded = serde_json::to_vec(&(
        1_u32,
        provider,
        codex,
        existing_definition,
        existing_children,
        current_codex,
    ))
    .map_err(|error| format!("codex_provider_set_digest_failed: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

async fn sync_universal_provider_internal_with_probe<F, Fut>(
    state: &AppState,
    id: &str,
    probe: F,
) -> Result<bool, AppError>
where
    F: FnOnce(Provider) -> Fut,
    Fut: Future<
        Output = Result<
            (
                Provider,
                Vec<crate::protocol_compatibility::ProtocolCompatibilityRecord>,
            ),
            String,
        >,
    >,
{
    let Some(provider) = ProviderService::prepare_universal_codex_provider(state, id)? else {
        return ProviderService::sync_universal_to_apps(state, id);
    };
    let (provider, profiles) = resolve_automatic_probe_outcome(provider, probe).await?;
    validate_legacy_universal_provider_set_commit(state, &provider, &profiles)?;
    ProviderService::sync_universal_to_apps_with_codex_profiles(
        state,
        id,
        Some(provider),
        &profiles,
    )
}

async fn save_and_sync_universal_provider_internal_with_probe<F, Fut>(
    state: &AppState,
    provider: UniversalProvider,
    probe: F,
) -> Result<bool, AppError>
where
    F: FnOnce(Provider) -> Fut,
    Fut: Future<
        Output = Result<
            (
                Provider,
                Vec<crate::protocol_compatibility::ProtocolCompatibilityRecord>,
            ),
            String,
        >,
    >,
{
    let Some(codex_provider) =
        ProviderService::prepare_universal_codex_provider_from_definition(state, &provider)?
    else {
        return ProviderService::save_and_sync_universal_to_apps_with_codex_profiles(
            state,
            provider,
            None,
            &[],
        );
    };
    let (codex_provider, profiles) = resolve_automatic_probe_outcome(codex_provider, probe).await?;
    validate_legacy_universal_provider_set_commit(state, &codex_provider, &profiles)?;
    ProviderService::save_and_sync_universal_to_apps_with_codex_profiles(
        state,
        provider,
        Some(codex_provider),
        &profiles,
    )
}

fn validate_legacy_universal_provider_set_commit(
    state: &AppState,
    provider: &Provider,
    profiles: &[crate::protocol_compatibility::ProtocolCompatibilityRecord],
) -> Result<(), AppError> {
    if provider.uses_manual_codex_protocol() {
        return Ok(());
    }
    let provider =
        ProviderService::prepare_provider_for_mutation(state, &AppType::Codex, provider.clone())?;
    let existing = state
        .db
        .get_all_providers(AppType::Codex.as_str())?
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    let prepared = crate::codex_multirouter::provider_set::plan_codex_provider_set(
        &provider,
        profiles,
        &existing,
        chrono::Utc::now().timestamp(),
    )
    .map_err(|error| AppError::InvalidInput(error.to_string()))?;
    match prepared.preview.plan {
        crate::codex_multirouter::provider_set::CodexProviderSetPlan::Single { .. } => Ok(()),
        crate::codex_multirouter::provider_set::CodexProviderSetPlan::Split { .. } => Err(
            AppError::InvalidInput("codex_provider_set_split_confirmation_required".to_string()),
        ),
        crate::codex_multirouter::provider_set::CodexProviderSetPlan::Blocked { .. } => Err(
            AppError::InvalidInput("codex_provider_set_model_blocked".to_string()),
        ),
    }
}

#[tauri::command]
pub async fn sync_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<bool, String> {
    let result = sync_universal_provider_internal_with_probe(state.inner(), &id, |provider| {
        crate::commands::protocol_compatibility::automatic_codex_provider_preflight(
            state.inner(),
            provider,
        )
    })
    .await
    .map_err(|e| e.to_string())?;

    emit_universal_provider_synced(&app, "sync", &id);

    Ok(result)
}

#[tauri::command]
pub async fn save_and_sync_universal_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: UniversalProvider,
) -> Result<bool, String> {
    let id = provider.id.clone();
    let result = save_and_sync_universal_provider_internal_with_probe(
        state.inner(),
        provider,
        |codex_provider| {
            crate::commands::protocol_compatibility::automatic_codex_provider_preflight(
                state.inner(),
                codex_provider,
            )
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    emit_universal_provider_synced(&app, "save-and-sync", &id);

    Ok(result)
}

#[tauri::command]
pub fn import_opencode_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_opencode_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_opencode_live_provider_ids() -> Result<Vec<String>, String> {
    crate::opencode_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}

// ============================================================================
// OpenClaw 专属命令 → 已迁移至 commands/openclaw.rs
// ============================================================================

#[cfg(test)]
mod import_claude_desktop_tests {
    use super::suggested_claude_desktop_routes;
    use crate::provider::{Provider, ProviderMeta};
    use serde_json::json;

    fn make_provider(env: serde_json::Value, provider_type: Option<&str>) -> Provider {
        let mut p = Provider::with_id(
            "test-claude".to_string(),
            "Test".to_string(),
            json!({ "env": env }),
            None,
        );
        if let Some(pt) = provider_type {
            p.meta = Some(ProviderMeta {
                provider_type: Some(pt.to_string()),
                ..ProviderMeta::default()
            });
        }
        p
    }

    #[test]
    fn route_strips_1m_suffix_and_sets_supports_1m() {
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-5-20250929[1M]",
            }),
            None,
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        let r = routes.get("claude-sonnet-5").expect("sonnet route present");
        assert_eq!(r.model, "claude-sonnet-4-5-20250929");
        assert!(
            !r.model.to_ascii_lowercase().contains("[1m]"),
            "model must not contain [1m] suffix"
        );
        assert_eq!(r.label_override, None);
        assert_eq!(r.supports_1m, Some(true));
    }

    #[test]
    fn route_preserves_model_without_suffix() {
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-k2",
            }),
            None,
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        let r = routes.get("claude-sonnet-5").expect("sonnet route present");
        assert_eq!(r.model, "kimi-k2");
        assert_eq!(r.label_override.as_deref(), Some("kimi-k2"));
        // 默认 provider_type 缺省 → supports_1m_default = true
        assert_eq!(r.supports_1m, Some(true));
    }

    #[test]
    fn route_uses_claude_code_model_name_as_label_override() {
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-k2",
                "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Kimi K2",
            }),
            None,
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        let r = routes.get("claude-sonnet-5").expect("sonnet route present");
        assert_eq!(r.model, "kimi-k2");
        assert_eq!(r.label_override.as_deref(), Some("Kimi K2"));
    }

    #[test]
    fn route_1m_suffix_overrides_provider_type_default() {
        // github_copilot 默认 supports_1m_default = false，但 [1M] 后缀应强制 true
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5-codex[1M]",
            }),
            Some("github_copilot"),
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        let r = routes.get("claude-sonnet-5").expect("sonnet route present");
        assert_eq!(r.model, "gpt-5-codex");
        assert_eq!(r.label_override.as_deref(), Some("gpt-5-codex"));
        assert_eq!(r.supports_1m, Some(true));
    }

    #[test]
    fn route_github_copilot_without_suffix_keeps_false() {
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "gpt-5-codex",
            }),
            Some("github_copilot"),
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        let r = routes.get("claude-sonnet-5").expect("sonnet route present");
        assert_eq!(r.model, "gpt-5-codex");
        assert_eq!(r.label_override.as_deref(), Some("gpt-5-codex"));
        assert_eq!(r.supports_1m, Some(false));
    }

    #[test]
    fn same_upstream_across_three_aliases_merges_to_one_route() {
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M2",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "MiniMax-M2",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "MiniMax-M2",
            }),
            None,
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        assert_eq!(routes.len(), 1, "three aliases → one merged route");
        let r = routes.get("claude-sonnet-5").expect("merged route present");
        assert_eq!(r.model, "MiniMax-M2");
        assert_eq!(r.label_override.as_deref(), Some("MiniMax-M2"));
    }

    #[test]
    fn same_upstream_with_partial_1m_marker_takes_or_aggregation() {
        // sonnet 带 [1M]，opus/haiku 不带 → 合并后 supports_1m == Some(true)
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "MiniMax-M2[1M]",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "MiniMax-M2",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "MiniMax-M2",
            }),
            None,
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        assert_eq!(routes.len(), 1);
        let r = routes.get("claude-sonnet-5").expect("merged route present");
        assert_eq!(r.supports_1m, Some(true));
    }

    #[test]
    fn different_upstream_models_produce_separate_routes() {
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "GLM-4.6",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "GLM-4-Air",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "GLM-4-Flash",
            }),
            None,
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        assert_eq!(routes.len(), 3);
        assert_eq!(routes.get("claude-sonnet-5").unwrap().model, "GLM-4.6");
        assert_eq!(routes.get("claude-opus-5").unwrap().model, "GLM-4-Air");
        assert_eq!(routes.get("claude-haiku-4-5").unwrap().model, "GLM-4-Flash");
        assert_eq!(
            routes
                .get("claude-sonnet-5")
                .unwrap()
                .label_override
                .as_deref(),
            Some("GLM-4.6")
        );
    }

    #[test]
    fn anthropic_model_fallback_only_triggers_when_empty() {
        // 三个 default env_key 都不填，仅 ANTHROPIC_MODEL
        let p = make_provider(
            json!({
                "ANTHROPIC_MODEL": "kimi-k2",
            }),
            None,
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        assert_eq!(routes.len(), 1);
        let r = routes
            .get("claude-sonnet-5")
            .expect("fallback route present");
        assert_eq!(r.model, "kimi-k2");
        assert_eq!(r.label_override.as_deref(), Some("kimi-k2"));
    }

    #[test]
    fn existing_claude_prefix_not_duplicated() {
        let p = make_provider(
            json!({
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-5-20250929",
            }),
            None,
        );
        let routes = suggested_claude_desktop_routes(&p).expect("routes built");
        assert!(routes.contains_key("claude-sonnet-5"));
        assert!(!routes.contains_key("claude-claude-sonnet-4-5-20250929"));
        assert_eq!(
            routes.get("claude-sonnet-5").expect("route").label_override,
            None
        );
    }
}

#[cfg(test)]
mod native_query_credentials_tests {
    use super::{resolve_coding_plan_credentials, resolve_native_credentials};
    use crate::app_config::AppType;
    use crate::provider::{Provider, UsageScript};
    use serde_json::json;

    fn usage_script(
        coding_plan_provider: Option<&str>,
        base_url: Option<&str>,
        api_key: Option<&str>,
    ) -> UsageScript {
        UsageScript {
            enabled: true,
            language: "javascript".to_string(),
            code: String::new(),
            timeout: Some(10),
            api_key: api_key.map(str::to_string),
            base_url: base_url.map(str::to_string),
            access_token: None,
            user_id: None,
            template_type: Some("token_plan".to_string()),
            auto_query_interval: None,
            coding_plan_provider: coding_plan_provider.map(str::to_string),
            access_key_id: None,
            secret_access_key: None,
            team_organization_id: None,
            team_project_id: None,
        }
    }

    #[test]
    fn delegates_to_provider_for_codex() {
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-codex" },
                "config": "model_provider = \"deepseek\"\n\
                           [model_providers.deepseek]\n\
                           base_url = \"https://api.deepseek.com\"\n",
            }),
            None,
        );
        let (base_url, api_key) = resolve_native_credentials(&AppType::Codex, Some(&provider));
        assert_eq!(base_url, "https://api.deepseek.com");
        assert_eq!(api_key, "sk-codex");
    }

    #[test]
    fn missing_provider_yields_empty() {
        let (base_url, api_key) = resolve_native_credentials(&AppType::Codex, None);
        assert!(base_url.is_empty());
        assert!(api_key.is_empty());
    }

    #[test]
    fn zenmux_coding_plan_uses_script_credentials_first() {
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://provider.zenmux.example/v1",
                    "ANTHROPIC_AUTH_TOKEN": "sk-provider"
                }
            }),
            None,
        );
        let script = usage_script(
            Some("zenmux"),
            Some("https://script.zenmux.example/api/usage/"),
            Some("sk-script"),
        );

        let (base_url, api_key) =
            resolve_coding_plan_credentials(&AppType::Claude, Some(&provider), Some(&script));

        assert_eq!(base_url, "https://script.zenmux.example/api/usage");
        assert_eq!(api_key, "sk-script");
    }

    #[test]
    fn zenmux_coding_plan_falls_back_to_provider_credentials() {
        let provider = Provider::with_id(
            "test".to_string(),
            "Test".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": "https://provider.zenmux.example/v1",
                    "ANTHROPIC_AUTH_TOKEN": "sk-provider"
                }
            }),
            None,
        );
        let script = usage_script(Some("zenmux"), Some("https://script.zenmux.example"), None);

        let (base_url, api_key) =
            resolve_coding_plan_credentials(&AppType::Claude, Some(&provider), Some(&script));

        assert_eq!(base_url, "https://provider.zenmux.example/v1");
        assert_eq!(api_key, "sk-provider");
    }
}

#[cfg(test)]
mod codex_protocol_preflight_save_tests {
    use super::{
        get_codex_logical_provider_for_editing_internal, update_provider_internal_with_probe,
    };
    use crate::{
        app_config::AppType,
        database::Database,
        protocol_compatibility::{
            apply_selected_transport_to_provider, ProbeReadiness, ProbeTargetKey,
            ProtocolCompatibilityProbeResult, ProtocolCompatibilityRecord, TransportKind,
        },
        provider::{CodexProtocolMode, Provider, ProviderMeta},
        services::ProviderService,
        store::AppState,
    };
    use serde_json::json;
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    fn codex_provider() -> Provider {
        Provider {
            id: "qwen-provider".to_string(),
            name: "Qwen".to_string(),
            settings_config: json!({
                "auth": {"OPENAI_API_KEY": "probe-secret"},
                "config": "model = \"qwen-visible\"\nmodel_provider = \"qwen\"\n[model_providers.qwen]\nbase_url = \"https://vllm.example/v1\"\nwire_api = \"responses\"\n",
                "modelCatalog": {"models": [{
                    "model": "qwen-visible",
                    "upstreamModel": "Qwen/Qwen3.8",
                    "apiFormat": "openai_responses"
                }]}
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                api_format: Some("openai_responses".to_string()),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn chat_record() -> ProtocolCompatibilityRecord {
        let target = ProbeTargetKey::new(
            "qwen-provider",
            None::<String>,
            "qwen-visible",
            "Qwen/Qwen3.8",
            TransportKind::OpenAiChat,
            "https://vllm.example/v1/chat/completions",
            "bearer",
        )
        .unwrap()
        .with_credential("probe-secret");
        let now = chrono::Utc::now().timestamp();
        ProtocolCompatibilityRecord::new(
            target,
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiChat),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            now,
            now + 600,
        )
    }

    #[tokio::test]
    async fn ordinary_codex_update_runs_preflight_and_atomically_saves_its_profile() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let provider = codex_provider();
        db.save_provider(AppType::Codex.as_str(), &provider)
            .expect("seed provider");
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_probe = calls.clone();
        let expected_record = chat_record();
        let returned_record = expected_record.clone();

        update_provider_internal_with_probe(
            &state,
            AppType::Codex,
            None,
            provider,
            move |mut candidate| {
                calls_for_probe.fetch_add(1, Ordering::SeqCst);
                apply_selected_transport_to_provider(&mut candidate, TransportKind::OpenAiChat)
                    .expect("apply detected protocol");
                std::future::ready(Ok((candidate, vec![returned_record])))
            },
        )
        .await
        .expect("ordinary update should save");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let saved = db
            .get_provider_by_id("qwen-provider", AppType::Codex.as_str())
            .expect("read provider")
            .expect("provider exists");
        assert_eq!(
            saved
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_chat")
        );
        assert!(saved.settings_config["modelCatalog"]["models"][0]
            .get("apiFormat")
            .is_none());
        let rebound_target =
            crate::protocol_compatibility::compile_provider_probe_candidate_for_model(
                &saved,
                "qwen-visible".to_string(),
                "Qwen/Qwen3.8".to_string(),
            )
            .expect("compile final Provider target")
            .target_key(TransportKind::OpenAiChat)
            .expect("final Chat target");
        let rebound = db
            .get_protocol_compatibility_result(&rebound_target)
            .expect("read rebound profile")
            .expect("rebound profile exists");
        assert_eq!(rebound.result, expected_record.result);
    }

    #[tokio::test]
    async fn invalid_codex_update_is_rejected_before_any_probe_request() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db);
        let mut provider = codex_provider();
        provider.settings_config["auth"] = serde_json::Value::Null;
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_probe = calls.clone();

        let result = update_provider_internal_with_probe(
            &state,
            AppType::Codex,
            None,
            provider,
            move |candidate| {
                calls_for_probe.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Ok((candidate, Vec::new())))
            },
        )
        .await;

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ordinary_codex_update_probe_failure_is_zero_write() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let original = codex_provider();
        db.save_provider(AppType::Codex.as_str(), &original)
            .expect("seed provider");
        let mut changed = original.clone();
        changed.name = "Changed but unverified".to_string();

        let error =
            update_provider_internal_with_probe(&state, AppType::Codex, None, changed, |_| {
                std::future::ready(Err("network_unavailable".to_string()))
            })
            .await
            .expect_err("normal mode must fail closed when probing cannot complete");

        assert!(error.to_string().contains("network_unavailable"));
        let saved = db
            .get_provider_by_id("qwen-provider", AppType::Codex.as_str())
            .expect("read provider")
            .expect("provider exists");
        assert_eq!(saved.name, original.name);
    }

    #[tokio::test]
    async fn advanced_manual_protocol_mode_preserves_user_transport_without_probing() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut provider = codex_provider();
        provider
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .codex_protocol_mode = Some(CodexProtocolMode::Manual);
        provider.settings_config["modelCatalog"]["models"][0]
            .as_object_mut()
            .expect("catalog model")
            .remove("apiFormat");
        db.save_provider(AppType::Codex.as_str(), &provider)
            .expect("seed manual provider");
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_probe = calls.clone();

        update_provider_internal_with_probe(
            &state,
            AppType::Codex,
            None,
            provider,
            move |candidate| {
                calls_for_probe.fetch_add(1, Ordering::SeqCst);
                std::future::ready(Ok((candidate, Vec::new())))
            },
        )
        .await
        .expect("save manual protocol provider");

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let saved = db
            .get_provider_by_id("qwen-provider", AppType::Codex.as_str())
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.meta.and_then(|meta| meta.api_format),
            Some("openai_responses".to_string())
        );
        assert!(saved.settings_config["modelCatalog"]["models"][0]
            .get("apiFormat")
            .is_none());
    }

    #[test]
    fn split_facade_editing_restores_one_logical_provider_draft() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut source = codex_provider();
        source.settings_config["modelCatalog"]["models"]
            .as_array_mut()
            .expect("catalog models")
            .push(json!({
                "model": "qwen-coder-visible",
                "upstreamModel": "Qwen/Qwen3-Coder"
            }));
        let now = chrono::Utc::now().timestamp();
        let record = |public_model: &str, upstream_model: &str, transport: TransportKind| {
            let target = crate::protocol_compatibility::compile_provider_probe_candidate_for_model(
                &source,
                public_model.to_string(),
                upstream_model.to_string(),
            )
            .expect("compile candidate")
            .target_key(transport)
            .expect("target");
            ProtocolCompatibilityRecord::new(
                target,
                ProtocolCompatibilityProbeResult {
                    selected_transport: Some(transport),
                    readiness: ProbeReadiness::Verified,
                    branches: Vec::new(),
                },
                now,
                now + 600,
            )
        };
        let records = vec![
            record(
                "qwen-visible",
                "Qwen/Qwen3.8",
                TransportKind::OpenAiResponses,
            ),
            record(
                "qwen-coder-visible",
                "Qwen/Qwen3-Coder",
                TransportKind::OpenAiChat,
            ),
        ];
        let prepared = crate::codex_multirouter::provider_set::plan_codex_provider_set(
            &source,
            &records,
            &HashMap::new(),
            now,
        )
        .expect("prepare split");
        crate::codex_multirouter::mutation::apply_codex_provider_set_mutation_with_publisher(
            db.as_ref(),
            prepared,
            |artifact| {
                Ok(
                    crate::codex_multirouter::projection::ProjectionReadBack::verified(
                        artifact.dependency_fingerprint.clone(),
                    ),
                )
            },
        )
        .expect("seed split Provider Set");

        let restored = get_codex_logical_provider_for_editing_internal(&state, "qwen-provider")
            .expect("restore logical Provider");

        assert_eq!(restored.id, source.id);
        assert_eq!(restored.name, source.name);
        assert!(restored.settings_config.get("codexRouting").is_none());
        assert!(restored.settings_config.get("codexProtocolSet").is_none());
        assert_eq!(
            restored.settings_config["modelCatalog"]["models"]
                .as_array()
                .expect("logical catalog")
                .len(),
            2
        );
    }

    #[test]
    fn advanced_manual_update_collapses_owned_split_leaves() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut source = codex_provider();
        source.settings_config["modelCatalog"]["models"]
            .as_array_mut()
            .expect("catalog models")
            .push(json!({
                "model": "qwen-coder-visible",
                "upstreamModel": "Qwen/Qwen3-Coder",
                "apiFormat": "openai_responses"
            }));
        let now = chrono::Utc::now().timestamp();
        let record = |public_model: &str, upstream_model: &str, transport: TransportKind| {
            let target = crate::protocol_compatibility::compile_provider_probe_candidate_for_model(
                &source,
                public_model.to_string(),
                upstream_model.to_string(),
            )
            .expect("compile candidate")
            .target_key(transport)
            .expect("target");
            ProtocolCompatibilityRecord::new(
                target,
                ProtocolCompatibilityProbeResult {
                    selected_transport: Some(transport),
                    readiness: ProbeReadiness::Verified,
                    branches: Vec::new(),
                },
                now,
                now + 600,
            )
        };
        let records = vec![
            record(
                "qwen-visible",
                "Qwen/Qwen3.8",
                TransportKind::OpenAiResponses,
            ),
            record(
                "qwen-coder-visible",
                "Qwen/Qwen3-Coder",
                TransportKind::OpenAiChat,
            ),
        ];
        let split = crate::codex_multirouter::provider_set::plan_codex_provider_set(
            &source,
            &records,
            &HashMap::new(),
            now,
        )
        .expect("prepare split");
        crate::codex_multirouter::mutation::apply_codex_provider_set_mutation_with_publisher(
            db.as_ref(),
            split,
            |artifact| {
                Ok(
                    crate::codex_multirouter::projection::ProjectionReadBack::verified(
                        artifact.dependency_fingerprint.clone(),
                    ),
                )
            },
        )
        .expect("seed split Provider Set");
        let existing = db
            .get_all_providers("codex")
            .expect("read split Providers")
            .into_iter()
            .collect::<HashMap<_, _>>();
        let facade = existing.get("qwen-provider").expect("facade");
        let mut restored = crate::codex_multirouter::provider_set::restore_logical_codex_provider(
            facade, &existing,
        )
        .expect("restore logical source");
        restored
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .codex_protocol_mode = Some(CodexProtocolMode::Manual);
        db.save_provider(
            "codex",
            &Provider::with_id(
                "other-provider".to_string(),
                "Other".to_string(),
                json!({"auth": {}}),
                None,
            ),
        )
        .expect("seed other Provider");
        db.set_current_provider("codex", "other-provider")
            .expect("activate other Provider");

        ProviderService::update(&state, AppType::Codex, None, restored)
            .expect("save whole-Provider manual protocol");

        assert!(db
            .get_provider_by_id("qwen-provider--ccsm-responses", "codex")
            .expect("read Responses leaf")
            .is_none());
        assert!(db
            .get_provider_by_id("qwen-provider--ccsm-chat", "codex")
            .expect("read Chat leaf")
            .is_none());
        let saved = db
            .get_provider_by_id("qwen-provider", "codex")
            .expect("read source")
            .expect("source exists");
        assert!(saved.settings_config.get("codexRouting").is_none());
        assert!(saved.settings_config.get("codexProtocolSet").is_none());
    }
}

#[cfg(test)]
mod universal_codex_protocol_preflight_tests {
    use super::{
        commit_universal_provider_set_internal, prepare_universal_provider_set_internal,
        save_and_sync_universal_provider_internal_with_probe,
        sync_universal_provider_internal_with_probe, CommitUniversalProviderSetRequest,
        PrepareUniversalProviderSetRequest,
    };
    use crate::commands::protocol_compatibility::CodexProviderSetCommitIntent;
    use crate::{
        app_config::AppType,
        database::Database,
        error::AppError,
        protocol_compatibility::{
            apply_selected_transport_to_provider, compile_provider_probe_candidate_for_model,
            ProbeReadiness, ProbeTargetKey, ProtocolCompatibilityProbeResult,
            ProtocolCompatibilityRecord, TransportKind,
        },
        provider::{
            CodexModelConfig, CodexProtocolMode, Provider, ProviderMeta, UniversalProvider,
        },
        services::ProviderService,
        store::AppState,
    };
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn mixed_universal_fixture(
        state: &AppState,
        id: &str,
        second_readiness: ProbeReadiness,
    ) -> (UniversalProvider, Vec<String>) {
        let mut universal = UniversalProvider::new(
            id.to_string(),
            format!("Universal {id}"),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.apps.gemini = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        let mut codex_provider = universal
            .to_codex_provider()
            .expect("Codex child is enabled");
        codex_provider.settings_config["modelCatalog"] = json!({
            "models": [
                {"model": "qwen-visible", "upstreamModel": "Qwen/Qwen3.8"},
                {"model": "glm-visible", "upstreamModel": "zai-org/GLM-4.5"}
            ]
        });
        state
            .db
            .save_provider(AppType::Codex.as_str(), &codex_provider)
            .expect("seed authoritative Universal Codex catalog");
        let now = chrono::Utc::now().timestamp();
        let receipt_ids = [
            (
                "qwen-visible",
                "Qwen/Qwen3.8",
                TransportKind::OpenAiResponses,
                ProbeReadiness::Verified,
            ),
            (
                "glm-visible",
                "zai-org/GLM-4.5",
                TransportKind::OpenAiChat,
                second_readiness,
            ),
        ]
        .into_iter()
        .map(|(public_model, upstream_model, transport, readiness)| {
            let target = compile_provider_probe_candidate_for_model(
                &codex_provider,
                public_model.to_string(),
                upstream_model.to_string(),
            )
            .expect("compile Universal Codex candidate")
            .target_key(transport)
            .expect("compile Universal Codex target");
            state
                .remember_codex_provider_set_probe_receipt(ProtocolCompatibilityRecord::new(
                    target,
                    ProtocolCompatibilityProbeResult {
                        selected_transport: Some(transport),
                        readiness,
                        branches: Vec::new(),
                    },
                    now,
                    now + 600,
                ))
                .expect("remember Universal Provider Set receipt")
        })
        .collect::<Vec<_>>();
        (universal, receipt_ids)
    }

    fn assert_universal_digest_change_is_rejected(
        mutate: impl FnOnce(&AppState, &UniversalProvider),
    ) {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let (universal, receipt_ids) =
            mixed_universal_fixture(&state, "universal-digest", ProbeReadiness::Verified);
        let now = chrono::Utc::now().timestamp();
        let preview = prepare_universal_provider_set_internal(
            &state,
            PrepareUniversalProviderSetRequest {
                provider: universal.clone(),
                receipt_ids: receipt_ids.clone(),
            },
            now,
        )
        .expect("prepare Universal Provider Set");

        mutate(&state, &universal);

        let error = commit_universal_provider_set_internal(
            &state,
            CommitUniversalProviderSetRequest {
                provider: universal,
                receipt_ids,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::ConfirmSplit,
            },
            now,
        )
        .expect_err("changed Universal dependency must invalidate the digest");
        assert!(error.contains("codex_provider_set_dependency_changed"));
        for provider_id in [
            "universal-codex-universal-digest--ccsm-responses",
            "universal-codex-universal-digest--ccsm-chat",
        ] {
            assert!(db
                .get_provider_by_id(provider_id, AppType::Codex.as_str())
                .expect("read rejected Universal leaf")
                .is_none());
        }
    }

    #[test]
    fn universal_sync_rejects_mismatched_codex_child_before_writing_other_children() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-atomic".to_string(),
            "Universal Atomic".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        ProviderService::upsert_universal(&state, universal).expect("seed universal provider");

        let mut mismatched_codex =
            ProviderService::prepare_universal_codex_provider(&state, "universal-atomic")
                .expect("prepare Codex child")
                .expect("Codex child enabled");
        mismatched_codex.id = "wrong-codex-child".to_string();

        let error = ProviderService::sync_universal_to_apps_with_codex_profiles(
            &state,
            "universal-atomic",
            Some(mismatched_codex),
            &[],
        )
        .expect_err("mismatched Codex child must reject the sync");

        assert!(error.to_string().contains("Codex"));
        assert!(
            db.get_provider_by_id("universal-claude-universal-atomic", "claude")
                .expect("read Claude child")
                .is_none(),
            "a rejected Universal sync must not leave a partial Claude child"
        );
    }

    #[test]
    fn universal_save_and_sync_rolls_back_definition_and_children_when_child_insert_fails(
    ) -> Result<(), AppError> {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-definition-atomic".to_string(),
            "Universal Definition Atomic".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.gemini = true;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER fail_atomic_universal_gemini_insert
                 BEFORE INSERT ON providers
                 WHEN NEW.app_type = 'gemini'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected atomic universal gemini failure');
                 END;",
            )?;
        }

        ProviderService::save_and_sync_universal_to_apps_with_codex_profiles(
            &state,
            universal,
            None,
            &[],
        )
        .expect_err("the injected child failure must abort the complete save-and-sync");

        assert!(
            db.get_universal_provider("universal-definition-atomic")?
                .is_none(),
            "the Universal definition must roll back with every generated child"
        );
        for (app_type, provider_id) in [
            ("claude", "universal-claude-universal-definition-atomic"),
            ("gemini", "universal-gemini-universal-definition-atomic"),
        ] {
            assert!(
                db.get_provider_by_id(provider_id, app_type)?.is_none(),
                "the {app_type} child must roll back with the Universal definition"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn legacy_universal_save_and_sync_requires_explicit_split_confirmation() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-mixed".to_string(),
            "Universal Mixed".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.apps.gemini = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });

        let mut existing_codex = universal
            .to_codex_provider()
            .expect("Codex child is enabled");
        existing_codex.settings_config["modelCatalog"] = json!({
            "models": [
                {"model": "qwen-visible", "upstreamModel": "Qwen/Qwen3.8"},
                {"model": "glm-visible", "upstreamModel": "zai-org/GLM-4.5"}
            ]
        });
        db.save_provider(AppType::Codex.as_str(), &existing_codex)
            .expect("seed the authoritative Universal Codex catalog");

        let error = save_and_sync_universal_provider_internal_with_probe(
            &state,
            universal,
            move |candidate| {
                let now = chrono::Utc::now().timestamp();
                let record =
                    |public_model: &str, upstream_model: &str, transport: TransportKind| {
                        let target = compile_provider_probe_candidate_for_model(
                            &candidate,
                            public_model.to_string(),
                            upstream_model.to_string(),
                        )
                        .expect("compile Universal Codex model candidate")
                        .target_key(transport)
                        .expect("compile Universal Codex probe target");
                        ProtocolCompatibilityRecord::new(
                            target,
                            ProtocolCompatibilityProbeResult {
                                selected_transport: Some(transport),
                                readiness: ProbeReadiness::Verified,
                                branches: Vec::new(),
                            },
                            now,
                            now + 600,
                        )
                    };
                let records = vec![
                    record(
                        "qwen-visible",
                        "Qwen/Qwen3.8",
                        TransportKind::OpenAiResponses,
                    ),
                    record("glm-visible", "zai-org/GLM-4.5", TransportKind::OpenAiChat),
                ];
                std::future::ready(Ok((candidate, records)))
            },
        )
        .await
        .expect_err("legacy save-and-sync must not commit a split without confirmation");
        assert!(error
            .to_string()
            .contains("codex_provider_set_split_confirmation_required"));

        assert!(db
            .get_universal_provider("universal-mixed")
            .expect("read Universal definition")
            .is_none());
        for (app_type, provider_id) in [
            ("claude", "universal-claude-universal-mixed"),
            ("gemini", "universal-gemini-universal-mixed"),
        ] {
            assert!(
                db.get_provider_by_id(provider_id, app_type)
                    .expect("read generated Universal child")
                    .is_none(),
                "rejected legacy split must not write the {app_type} child"
            );
        }
        for provider_id in [
            "universal-codex-universal-mixed--ccsm-responses",
            "universal-codex-universal-mixed--ccsm-chat",
        ] {
            assert!(db
                .get_provider_by_id(provider_id, AppType::Codex.as_str())
                .expect("read rejected Universal Codex leaf")
                .is_none());
        }
    }

    #[test]
    fn disabling_split_universal_codex_removes_facade_owned_leaves_and_profiles_atomically(
    ) -> Result<(), AppError> {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-disable-split".to_string(),
            "Universal Disable Split".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.apps.gemini = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        let mut codex_provider = universal
            .to_codex_provider()
            .expect("Codex child is enabled");
        codex_provider.settings_config["modelCatalog"] = json!({
            "models": [
                {"model": "qwen-visible", "upstreamModel": "Qwen/Qwen3.8"},
                {"model": "glm-visible", "upstreamModel": "zai-org/GLM-4.5"}
            ]
        });
        let now = chrono::Utc::now().timestamp();
        let record = |public_model: &str, upstream_model: &str, transport: TransportKind| {
            let target = compile_provider_probe_candidate_for_model(
                &codex_provider,
                public_model.to_string(),
                upstream_model.to_string(),
            )
            .expect("compile Universal Codex model candidate")
            .target_key(transport)
            .expect("compile Universal Codex probe target");
            ProtocolCompatibilityRecord::new(
                target,
                ProtocolCompatibilityProbeResult {
                    selected_transport: Some(transport),
                    readiness: ProbeReadiness::Verified,
                    branches: Vec::new(),
                },
                now,
                now + 600,
            )
        };
        let records = vec![
            record(
                "qwen-visible",
                "Qwen/Qwen3.8",
                TransportKind::OpenAiResponses,
            ),
            record("glm-visible", "zai-org/GLM-4.5", TransportKind::OpenAiChat),
        ];
        ProviderService::save_and_sync_universal_to_apps_with_codex_profiles(
            &state,
            universal.clone(),
            Some(codex_provider),
            &records,
        )
        .expect("seed split Universal Provider Set");

        universal.apps.codex = false;
        universal.models.codex = None;
        ProviderService::save_and_sync_universal_to_apps_with_codex_profiles(
            &state,
            universal,
            None,
            &[],
        )
        .expect("disable split Universal Codex child");

        for provider_id in [
            "universal-codex-universal-disable-split",
            "universal-codex-universal-disable-split--ccsm-responses",
            "universal-codex-universal-disable-split--ccsm-chat",
        ] {
            assert!(
                db.get_provider_by_id(provider_id, AppType::Codex.as_str())
                    .expect("read disabled Codex Provider Set member")
                    .is_none(),
                "disabling Universal Codex must remove {provider_id}"
            );
            let conn = crate::database::lock_conn!(db.conn);
            let profile_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM protocol_compatibility_profiles WHERE provider_id = ?1",
                    [provider_id],
                    |row| row.get(0),
                )
                .expect("count disabled Provider Set profiles");
            assert_eq!(
                profile_count, 0,
                "profiles for {provider_id} must be deleted"
            );
        }
        Ok(())
    }

    #[test]
    fn universal_mixed_prepare_is_zero_write_and_commit_requires_split_confirmation() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-preview".to_string(),
            "Universal Preview".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.apps.gemini = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        let mut existing_codex = universal
            .to_codex_provider()
            .expect("Codex child is enabled");
        existing_codex.settings_config["modelCatalog"] = json!({
            "models": [
                {"model": "qwen-visible", "upstreamModel": "Qwen/Qwen3.8"},
                {"model": "glm-visible", "upstreamModel": "zai-org/GLM-4.5"}
            ]
        });
        db.save_provider(AppType::Codex.as_str(), &existing_codex)
            .expect("seed Universal Codex catalog");
        let now = chrono::Utc::now().timestamp();
        let record = |public_model: &str, upstream_model: &str, transport: TransportKind| {
            let target = compile_provider_probe_candidate_for_model(
                &existing_codex,
                public_model.to_string(),
                upstream_model.to_string(),
            )
            .expect("compile Universal Codex model candidate")
            .target_key(transport)
            .expect("compile Universal Codex probe target");
            ProtocolCompatibilityRecord::new(
                target,
                ProtocolCompatibilityProbeResult {
                    selected_transport: Some(transport),
                    readiness: ProbeReadiness::Verified,
                    branches: Vec::new(),
                },
                now,
                now + 600,
            )
        };
        let receipt_ids = [
            record(
                "qwen-visible",
                "Qwen/Qwen3.8",
                TransportKind::OpenAiResponses,
            ),
            record("glm-visible", "zai-org/GLM-4.5", TransportKind::OpenAiChat),
        ]
        .into_iter()
        .map(|record| {
            state
                .remember_codex_provider_set_probe_receipt(record)
                .expect("remember Universal Provider Set receipt")
        })
        .collect::<Vec<_>>();

        let preview = prepare_universal_provider_set_internal(
            &state,
            PrepareUniversalProviderSetRequest {
                provider: universal.clone(),
                receipt_ids: receipt_ids.clone(),
            },
            now,
        )
        .expect("prepare Universal Provider Set");
        assert!(matches!(
            preview.codex.as_ref().expect("Codex preview").plan,
            crate::codex_multirouter::provider_set::CodexProviderSetPlan::Split { .. }
        ));
        assert!(db
            .get_universal_provider("universal-preview")
            .expect("read Universal definition")
            .is_none());
        assert!(db
            .get_provider_by_id("universal-claude-universal-preview", "claude")
            .expect("read Claude child")
            .is_none());
        assert!(db
            .get_provider_by_id("universal-gemini-universal-preview", "gemini")
            .expect("read Gemini child")
            .is_none());

        let error = commit_universal_provider_set_internal(
            &state,
            CommitUniversalProviderSetRequest {
                provider: universal.clone(),
                receipt_ids: receipt_ids.clone(),
                digest: preview.digest.clone(),
                intent: CodexProviderSetCommitIntent::AcceptSingle,
            },
            now,
        )
        .expect_err("mixed Universal Provider Set requires split confirmation");
        assert!(error.contains("codex_provider_set_split_confirmation_required"));
        assert!(db
            .get_universal_provider("universal-preview")
            .expect("read Universal definition after rejected commit")
            .is_none());

        commit_universal_provider_set_internal(
            &state,
            CommitUniversalProviderSetRequest {
                provider: universal,
                receipt_ids,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::ConfirmSplit,
            },
            now,
        )
        .expect("confirm and commit Universal split");
        for provider_id in [
            "universal-codex-universal-preview",
            "universal-codex-universal-preview--ccsm-responses",
            "universal-codex-universal-preview--ccsm-chat",
        ] {
            assert!(db
                .get_provider_by_id(provider_id, "codex")
                .expect("read committed Universal Provider Set member")
                .is_some());
        }
    }

    #[test]
    fn universal_partial_model_is_blocked_and_never_writes_children() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let (universal, receipt_ids) =
            mixed_universal_fixture(&state, "universal-blocked", ProbeReadiness::Partial);
        let now = chrono::Utc::now().timestamp();
        let preview = prepare_universal_provider_set_internal(
            &state,
            PrepareUniversalProviderSetRequest {
                provider: universal.clone(),
                receipt_ids: receipt_ids.clone(),
            },
            now,
        )
        .expect("prepare blocked Universal Provider Set");
        let blocked = match preview.codex.as_ref().map(|preview| &preview.plan) {
            Some(crate::codex_multirouter::provider_set::CodexProviderSetPlan::Blocked {
                models,
            }) => models,
            other => panic!("expected blocked Codex plan, got {other:?}"),
        };
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].model, "glm-visible");
        assert_eq!(blocked[0].reason, "probe_not_verified");

        let error = commit_universal_provider_set_internal(
            &state,
            CommitUniversalProviderSetRequest {
                provider: universal,
                receipt_ids,
                digest: preview.digest,
                intent: CodexProviderSetCommitIntent::AcceptSingle,
            },
            now,
        )
        .expect_err("blocked Universal Provider Set must not commit");
        assert!(error.contains("codex_provider_set_model_blocked"));
        assert!(db
            .get_universal_provider("universal-blocked")
            .expect("read rejected Universal definition")
            .is_none());
        for (app_type, provider_id) in [
            ("claude", "universal-claude-universal-blocked"),
            ("gemini", "universal-gemini-universal-blocked"),
            ("codex", "universal-codex-universal-blocked--ccsm-responses"),
            ("codex", "universal-codex-universal-blocked--ccsm-chat"),
        ] {
            assert!(db
                .get_provider_by_id(provider_id, app_type)
                .expect("read rejected Universal child")
                .is_none());
        }
    }

    #[test]
    fn universal_prepare_digest_binds_definition_children_and_current_provider() {
        assert_universal_digest_change_is_rejected(|state, universal| {
            let mut changed = universal.clone();
            changed.name = "Concurrent definition edit".to_string();
            ProviderService::upsert_universal(state, changed)
                .expect("write concurrent Universal definition");
        });
        assert_universal_digest_change_is_rejected(|state, universal| {
            let mut child = universal
                .to_claude_provider()
                .expect("Claude child is enabled");
            child.name = "Concurrent Claude edit".to_string();
            state
                .db
                .save_provider(AppType::Claude.as_str(), &child)
                .expect("write concurrent Claude child");
        });
        assert_universal_digest_change_is_rejected(|state, universal| {
            let mut child = universal
                .to_gemini_provider()
                .expect("Gemini child is enabled");
            child.name = "Concurrent Gemini edit".to_string();
            state
                .db
                .save_provider(AppType::Gemini.as_str(), &child)
                .expect("write concurrent Gemini child");
        });
        assert_universal_digest_change_is_rejected(|state, _| {
            let other = Provider::with_id(
                "other-codex".to_string(),
                "Other Codex".to_string(),
                json!({}),
                None,
            );
            state
                .db
                .save_provider(AppType::Codex.as_str(), &other)
                .expect("seed concurrent current Provider");
            state
                .db
                .set_current_provider(AppType::Codex.as_str(), &other.id)
                .expect("change current Codex Provider");
        });
    }

    #[tokio::test]
    async fn universal_save_and_sync_commits_definition_all_children_and_profile_together() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-complete".to_string(),
            "Universal Complete".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.apps.gemini = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });

        let target = ProbeTargetKey::new(
            "universal-codex-universal-complete",
            None::<String>,
            "qwen-visible",
            "qwen-visible",
            TransportKind::OpenAiChat,
            "https://gateway.example/v1/chat/completions",
            "bearer",
        )
        .expect("profile target");
        let now = chrono::Utc::now().timestamp();
        let record = ProtocolCompatibilityRecord::new(
            target.clone(),
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiChat),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            now,
            now + 600,
        );
        let returned_record = record.clone();

        save_and_sync_universal_provider_internal_with_probe(
            &state,
            universal,
            move |mut candidate| {
                assert_eq!(candidate.id, "universal-codex-universal-complete");
                apply_selected_transport_to_provider(&mut candidate, TransportKind::OpenAiChat)
                    .expect("apply selected transport");
                std::future::ready(Ok((candidate, vec![returned_record])))
            },
        )
        .await
        .expect("save the complete Universal Provider transaction");

        let saved_definition = db
            .get_universal_provider("universal-complete")
            .expect("read Universal definition")
            .expect("Universal definition exists");
        assert_eq!(saved_definition.name, "Universal Complete");
        for (app_type, provider_id) in [
            ("claude", "universal-claude-universal-complete"),
            ("codex", "universal-codex-universal-complete"),
            ("gemini", "universal-gemini-universal-complete"),
        ] {
            assert!(
                db.get_provider_by_id(provider_id, app_type)
                    .expect("read generated child")
                    .is_some(),
                "{app_type} child must be visible with the committed definition"
            );
        }
        let saved_codex = db
            .get_provider_by_id("universal-codex-universal-complete", "codex")
            .expect("read generated Codex child")
            .expect("generated Codex child exists");
        let rebound_target = compile_provider_probe_candidate_for_model(
            &saved_codex,
            "qwen-visible".to_string(),
            "qwen-visible".to_string(),
        )
        .expect("compile saved Universal Codex target")
        .target_key(TransportKind::OpenAiChat)
        .expect("compile saved Universal Codex Chat target");
        let rebound_record = db
            .get_protocol_compatibility_result(&rebound_target)
            .expect("read rebound protocol profile")
            .expect("rebound protocol profile exists");
        assert_eq!(
            rebound_record.result, record.result,
            "the selected Codex profile must commit with the definition and children"
        );
    }

    #[tokio::test]
    async fn universal_without_codex_skips_probe_and_atomically_saves_other_children() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-no-codex".to_string(),
            "Universal Without Codex".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = false;
        universal.apps.gemini = true;
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_probe = calls.clone();

        save_and_sync_universal_provider_internal_with_probe(&state, universal, move |candidate| {
            calls_for_probe.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok((candidate, Vec::new())))
        })
        .await
        .expect("save Universal Provider without Codex");

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(db
            .get_universal_provider("universal-no-codex")
            .expect("read Universal definition")
            .is_some());
        assert!(db
            .get_provider_by_id(
                "universal-claude-universal-no-codex",
                AppType::Claude.as_str(),
            )
            .expect("read Claude child")
            .is_some());
        assert!(db
            .get_provider_by_id(
                "universal-gemini-universal-no-codex",
                AppType::Gemini.as_str(),
            )
            .expect("read Gemini child")
            .is_some());
        assert!(db
            .get_provider_by_id(
                "universal-codex-universal-no-codex",
                AppType::Codex.as_str(),
            )
            .expect("read Codex child")
            .is_none());
    }

    #[test]
    fn universal_sync_rejects_foreign_probe_profile_before_writing_other_children() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-profile-atomic".to_string(),
            "Universal Profile Atomic".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        ProviderService::upsert_universal(&state, universal).expect("seed universal provider");
        let codex_provider =
            ProviderService::prepare_universal_codex_provider(&state, "universal-profile-atomic")
                .expect("prepare Codex child")
                .expect("Codex child enabled");
        let foreign_target = ProbeTargetKey::new(
            "foreign-provider",
            None::<String>,
            "qwen-visible",
            "qwen-visible",
            TransportKind::OpenAiResponses,
            "https://gateway.example/v1/responses",
            "bearer",
        )
        .expect("foreign target");
        let foreign_profile = ProtocolCompatibilityRecord::new(
            foreign_target,
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiResponses),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            100,
            200,
        );

        ProviderService::sync_universal_to_apps_with_codex_profiles(
            &state,
            "universal-profile-atomic",
            Some(codex_provider),
            &[foreign_profile],
        )
        .expect_err("foreign protocol profile must reject the sync");

        assert!(
            db.get_provider_by_id("universal-claude-universal-profile-atomic", "claude")
                .expect("read Claude child")
                .is_none(),
            "a rejected Universal sync must not leave a partial Claude child"
        );
    }

    #[test]
    fn universal_sync_rolls_back_every_child_and_profile_when_gemini_insert_fails(
    ) -> Result<(), AppError> {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-db-atomic".to_string(),
            "Universal DB Atomic".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.apps.gemini = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        ProviderService::upsert_universal(&state, universal).expect("seed universal provider");
        let codex_provider =
            ProviderService::prepare_universal_codex_provider(&state, "universal-db-atomic")
                .expect("prepare Codex child")
                .expect("Codex child enabled");
        let target = ProbeTargetKey::new(
            codex_provider.id.clone(),
            None::<String>,
            "qwen-visible",
            "qwen-visible",
            TransportKind::OpenAiResponses,
            "https://gateway.example/v1/responses",
            "bearer",
        )
        .expect("profile target");
        let now = chrono::Utc::now().timestamp();
        let profile = ProtocolCompatibilityRecord::new(
            target.clone(),
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiResponses),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            now,
            now + 600,
        );
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER fail_universal_gemini_insert
                 BEFORE INSERT ON providers
                 WHEN NEW.app_type = 'gemini'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected universal gemini failure');
                 END;",
            )
            .expect("install Gemini failure trigger");
        }

        ProviderService::sync_universal_to_apps_with_codex_profiles(
            &state,
            "universal-db-atomic",
            Some(codex_provider),
            &[profile],
        )
        .expect_err("the injected Gemini insert failure must abort Universal sync");

        for (app_type, provider_id) in [
            ("claude", "universal-claude-universal-db-atomic"),
            ("codex", "universal-codex-universal-db-atomic"),
            ("gemini", "universal-gemini-universal-db-atomic"),
        ] {
            assert!(
                db.get_provider_by_id(provider_id, app_type)
                    .expect("read generated child")
                    .is_none(),
                "{app_type} child must roll back with the failed Universal sync"
            );
        }
        assert_eq!(
            db.get_protocol_compatibility_result(&target)
                .expect("read protocol profile"),
            None,
            "the Codex protocol profile must be in the same transaction as every child"
        );
        Ok(())
    }

    #[test]
    fn universal_sync_rolls_back_codex_disable_when_gemini_update_fails() -> Result<(), AppError> {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-disable-atomic".to_string(),
            "Universal Disable Atomic".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.apps.gemini = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        universal.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            codex_protocol_mode: Some(CodexProtocolMode::Manual),
            ..ProviderMeta::default()
        });
        ProviderService::upsert_universal(&state, universal.clone())?;
        ProviderService::sync_universal_to_apps(&state, &universal.id)?;

        universal.apps.codex = false;
        universal.models.codex = None;
        universal.name = "Universal Disable Atomic Updated".to_string();
        ProviderService::upsert_universal(&state, universal.clone())?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute_batch(
                "CREATE TRIGGER fail_universal_gemini_update
                 BEFORE UPDATE ON providers
                 WHEN NEW.app_type = 'gemini'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected universal gemini update failure');
                 END;",
            )?;
        }

        let error = ProviderService::sync_universal_to_apps(&state, &universal.id)
            .expect_err("the injected Gemini update failure must abort Universal sync");
        assert!(
            error
                .to_string()
                .contains("injected universal gemini update failure"),
            "the sync must reach the shared database transaction: {error}"
        );
        assert!(
            db.get_provider_by_id(
                "universal-codex-universal-disable-atomic",
                AppType::Codex.as_str(),
            )?
            .is_some(),
            "the Codex child deletion must roll back with the Gemini update"
        );
        Ok(())
    }

    #[test]
    fn universal_sync_disables_existing_codex_child_without_manual_deletion() -> Result<(), AppError>
    {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-disable".to_string(),
            "Universal Disable".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.claude = true;
        universal.apps.codex = true;
        universal.apps.gemini = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        universal.meta = Some(ProviderMeta {
            api_format: Some("openai_responses".to_string()),
            codex_protocol_mode: Some(CodexProtocolMode::Manual),
            ..ProviderMeta::default()
        });
        ProviderService::upsert_universal(&state, universal.clone())?;
        ProviderService::sync_universal_to_apps(&state, &universal.id)?;

        universal.apps.codex = false;
        universal.models.codex = None;
        ProviderService::upsert_universal(&state, universal.clone())?;
        ProviderService::sync_universal_to_apps(&state, &universal.id)?;

        assert!(
            db.get_provider_by_id("universal-codex-universal-disable", AppType::Codex.as_str(),)?
                .is_none(),
            "disabling Codex must remove the generated child in the same sync"
        );
        assert!(db
            .get_provider_by_id(
                "universal-claude-universal-disable",
                AppType::Claude.as_str(),
            )?
            .is_some());
        assert!(db
            .get_provider_by_id(
                "universal-gemini-universal-disable",
                AppType::Gemini.as_str(),
            )?
            .is_some());
        Ok(())
    }

    #[tokio::test]
    async fn universal_codex_sync_runs_preflight_and_atomically_saves_its_profile() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-probe".to_string(),
            "Universal Probe".to_string(),
            "newapi".to_string(),
            "https://vllm.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.codex = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        ProviderService::upsert_universal(&state, universal).expect("seed universal provider");

        let expected_target = ProbeTargetKey::new(
            "universal-codex-universal-probe",
            None::<String>,
            "qwen-visible",
            "qwen-visible",
            TransportKind::OpenAiChat,
            "https://vllm.example/v1/chat/completions",
            "bearer",
        )
        .expect("target");
        let now = chrono::Utc::now().timestamp();
        let expected_record = ProtocolCompatibilityRecord::new(
            expected_target.clone(),
            ProtocolCompatibilityProbeResult {
                selected_transport: Some(TransportKind::OpenAiChat),
                readiness: ProbeReadiness::Verified,
                branches: Vec::new(),
            },
            now,
            now + 600,
        );
        let returned_record = expected_record.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_probe = calls.clone();

        sync_universal_provider_internal_with_probe(
            &state,
            "universal-probe",
            move |mut candidate| {
                calls_for_probe.fetch_add(1, Ordering::SeqCst);
                assert_eq!(candidate.id, "universal-codex-universal-probe");
                apply_selected_transport_to_provider(&mut candidate, TransportKind::OpenAiChat)
                    .expect("apply detected transport");
                std::future::ready(Ok((candidate, vec![returned_record])))
            },
        )
        .await
        .expect("sync universal provider");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let saved = db
            .get_provider_by_id("universal-codex-universal-probe", "codex")
            .expect("read generated provider")
            .expect("generated provider exists");
        assert_eq!(
            saved
                .meta
                .as_ref()
                .and_then(|meta| meta.api_format.as_deref()),
            Some("openai_chat")
        );
        let rebound_target = compile_provider_probe_candidate_for_model(
            &saved,
            "qwen-visible".to_string(),
            "qwen-visible".to_string(),
        )
        .expect("compile saved Universal Codex target")
        .target_key(TransportKind::OpenAiChat)
        .expect("compile saved Universal Codex Chat target");
        let rebound_record = db
            .get_protocol_compatibility_result(&rebound_target)
            .expect("read saved profile")
            .expect("saved profile exists");
        assert_eq!(rebound_record.result, expected_record.result);
    }

    #[tokio::test]
    async fn legacy_universal_sync_requires_explicit_split_confirmation() {
        let db = Arc::new(Database::memory().expect("memory database"));
        let state = AppState::new(db.clone());
        let mut universal = UniversalProvider::new(
            "universal-sync-mixed".to_string(),
            "Universal Sync Mixed".to_string(),
            "newapi".to_string(),
            "https://gateway.example/v1".to_string(),
            "probe-secret".to_string(),
        );
        universal.apps.codex = true;
        universal.models.codex = Some(CodexModelConfig {
            model: Some("qwen-visible".to_string()),
            reasoning_effort: Some("medium".to_string()),
        });
        ProviderService::upsert_universal(&state, universal.clone())
            .expect("seed Universal definition");
        let mut codex_provider = universal
            .to_codex_provider()
            .expect("Codex child is enabled");
        codex_provider.settings_config["modelCatalog"] = json!({
            "models": [
                {"model": "qwen-visible", "upstreamModel": "Qwen/Qwen3.8"},
                {"model": "glm-visible", "upstreamModel": "zai-org/GLM-4.5"}
            ]
        });
        db.save_provider(AppType::Codex.as_str(), &codex_provider)
            .expect("seed authoritative Universal Codex catalog");

        let error = sync_universal_provider_internal_with_probe(
            &state,
            "universal-sync-mixed",
            move |candidate| {
                let now = chrono::Utc::now().timestamp();
                let record =
                    |public_model: &str, upstream_model: &str, transport: TransportKind| {
                        let target = compile_provider_probe_candidate_for_model(
                            &candidate,
                            public_model.to_string(),
                            upstream_model.to_string(),
                        )
                        .expect("compile Universal Codex model candidate")
                        .target_key(transport)
                        .expect("compile Universal Codex probe target");
                        ProtocolCompatibilityRecord::new(
                            target,
                            ProtocolCompatibilityProbeResult {
                                selected_transport: Some(transport),
                                readiness: ProbeReadiness::Verified,
                                branches: Vec::new(),
                            },
                            now,
                            now + 600,
                        )
                    };
                let records = vec![
                    record(
                        "qwen-visible",
                        "Qwen/Qwen3.8",
                        TransportKind::OpenAiResponses,
                    ),
                    record("glm-visible", "zai-org/GLM-4.5", TransportKind::OpenAiChat),
                ];
                std::future::ready(Ok((candidate, records)))
            },
        )
        .await
        .expect_err("legacy sync must not commit a split without confirmation");

        assert!(error
            .to_string()
            .contains("codex_provider_set_split_confirmation_required"));
        for provider_id in [
            "universal-codex-universal-sync-mixed--ccsm-responses",
            "universal-codex-universal-sync-mixed--ccsm-chat",
        ] {
            assert!(db
                .get_provider_by_id(provider_id, AppType::Codex.as_str())
                .expect("read rejected Universal Codex leaf")
                .is_none());
        }
    }
}
