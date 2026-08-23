//! 预设注册表 Tauri 命令：设备级源配置管理 + 检查更新（复用 WebDAV 传输）。
//!
//! 本文件是 WebDAV 同步 × 预设注册表联调的前端入口（见
//! `docs/superpowers/plans/2026-08-23-webdav-preset-registry-integration.md`）。
//! 只做“获取 + 校验”，不落地应用预设；应用与三方合并是 P1 后续工作。

use serde_json::{json, Value};

use crate::error::AppError;
use crate::services::preset_registry as preset_registry_service;
use crate::services::preset_registry::PresetRegistrySettings;
use crate::settings;

/// 读取设备级预设注册表设置。
#[tauri::command]
pub fn preset_registry_get_settings() -> Result<Option<PresetRegistrySettings>, String> {
    Ok(settings::get_preset_registry_settings())
}

/// 保存设备级预设注册表设置（含 WebDAV 凭据，属设备私有，不跨设备同步）。
#[tauri::command]
pub fn preset_registry_save_settings(
    settings: Option<PresetRegistrySettings>,
) -> Result<(), String> {
    settings::set_preset_registry_settings(settings).map_err(|e| e.to_string())
}

/// 检查指定预设源是否有可用更新（拉取并校验 manifest，不应用）。
///
/// 返回 manifest 元数据（版本、发布时间、过期时间、变更摘要）；校验失败返回错误。
#[tauri::command]
pub async fn preset_registry_check_update(
    source_id: String,
) -> Result<Value, String> {
    let registry = settings::get_preset_registry_settings().ok_or_else(|| {
        AppError::localized(
            "preset.registry.not_configured",
            "未配置预设注册表",
            "Preset registry is not configured.",
        )
        .to_string()
    })?;
    let source = registry
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| {
            AppError::localized(
                "preset.source.not_found",
                "预设源不存在",
                "Preset source not found.",
            )
            .to_string()
        })?
        .clone();

    let now_unix = chrono::Utc::now().timestamp();
    let manifest = match source.kind {
        preset_registry_service::PresetSourceKind::WebDav => {
            preset_registry_service::fetch_preset_manifest_from_webdav(&source, now_unix).await?
        }
        preset_registry_service::PresetSourceKind::Https => {
            return Err(AppError::localized(
                "preset.source.https_not_implemented",
                "HTTPS 预设源尚未实现（本次联调仅交付 WebDAV 传输）",
                "HTTPS preset source not implemented (WebDAV transport only this round).",
            )
            .to_string());
        }
    };

    Ok(json!({
        "sourceId": source.id,
        "version": manifest.version,
        "publishedAt": manifest.published_at,
        "expiresAt": manifest.expires_at,
        "target": manifest.target,
        "size": manifest.size,
        "changelog": manifest.changelog,
        "lastAcceptedVersion": source.last_accepted_version,
        "hasUpdate": source.last_accepted_version.is_empty()
            || !preset_registry_service::is_newer(
                &source.last_accepted_version,
                &manifest.version,
            ),
    }))
}
