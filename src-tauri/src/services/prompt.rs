use indexmap::IndexMap;
use std::path::Path;

use crate::app_config::AppType;
use crate::config::write_text_file;
use crate::error::AppError;
use crate::prompt::Prompt;
use crate::prompt_files::prompt_file_path;
use crate::services::pi_prompt_files::PiAgentsFileGuard;
use crate::store::AppState;

/// 安全地获取当前 Unix 时间戳
fn get_unix_timestamp() -> Result<i64, AppError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| AppError::Message(format!("Failed to get system time: {e}")))
}

pub struct PromptService;

fn project_prompt_set_to_path(
    prompts: &IndexMap<String, Prompt>,
    target_path: &Path,
) -> Result<Option<String>, AppError> {
    let enabled = prompts
        .iter()
        .filter(|(_, prompt)| prompt.enabled)
        .collect::<Vec<_>>();

    if let Some((_, prompt)) = enabled.first() {
        write_text_file(target_path, &prompt.content)?;
    }
    // With nothing enabled, preserve the local file: it is not part of the
    // database/sync payload, so restore has no authority to erase it. The UI
    // path for explicitly disabling the final managed prompt still clears it.

    if enabled.len() <= 1 {
        return Ok(None);
    }
    let ids = enabled
        .iter()
        .map(|(id, _)| id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(Some(format!(
        "多个 Prompt 同时启用，已按稳定顺序投影第一个；enabled IDs: {ids}"
    )))
}

impl PromptService {
    pub fn get_prompts(
        state: &AppState,
        app: AppType,
    ) -> Result<IndexMap<String, Prompt>, AppError> {
        if matches!(app, AppType::Pi) {
            return get_pi_prompts(state);
        }
        state.db.get_prompts(app.as_str())
    }

    pub fn upsert_prompt(
        state: &AppState,
        app: AppType,
        _id: &str,
        prompt: Prompt,
    ) -> Result<(), AppError> {
        if matches!(app, AppType::Pi) {
            return upsert_pi_prompt(state, _id, prompt);
        }
        // 检查是否为已启用的提示词
        let is_enabled = prompt.enabled;

        state.db.save_prompt(app.as_str(), &prompt)?;

        if is_enabled {
            // 启用提示词：写入内容到文件
            let target_path = prompt_file_path(&app)?;
            write_text_file(&target_path, &prompt.content)?;
        } else {
            // 禁用提示词：检查是否还有其他已启用的提示词
            let prompts = state.db.get_prompts(app.as_str())?;
            let any_enabled = prompts.values().any(|p| p.enabled);

            if !any_enabled {
                // 所有提示词都已禁用，清空文件
                let target_path = prompt_file_path(&app)?;
                if target_path.exists() {
                    write_text_file(&target_path, "")?;
                }
            }
        }

        Ok(())
    }

    pub fn delete_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        if matches!(app, AppType::Pi) {
            return delete_pi_prompt(state, id);
        }
        let prompts = Self::get_prompts(state, app.clone())?;

        if let Some(prompt) = prompts.get(id) {
            if prompt.enabled {
                return Err(AppError::InvalidInput("无法删除已启用的提示词".to_string()));
            }
        }

        state.db.delete_prompt(app.as_str(), id)?;
        Ok(())
    }

    pub fn enable_prompt(state: &AppState, app: AppType, id: &str) -> Result<(), AppError> {
        if matches!(app, AppType::Pi) {
            return enable_pi_prompt(state, id);
        }
        // 回填当前 live 文件内容到已启用的提示词，或创建备份
        let target_path = prompt_file_path(&app)?;
        if target_path.exists() {
            if let Ok(live_content) = std::fs::read_to_string(&target_path) {
                if !live_content.trim().is_empty() {
                    let mut prompts = state.db.get_prompts(app.as_str())?;

                    // 尝试回填到当前已启用的提示词
                    if let Some((enabled_id, enabled_prompt)) = prompts
                        .iter_mut()
                        .find(|(_, p)| p.enabled)
                        .map(|(id, p)| (id.clone(), p))
                    {
                        let timestamp = get_unix_timestamp()?;
                        enabled_prompt.content = live_content.clone();
                        enabled_prompt.updated_at = Some(timestamp);
                        log::info!("回填 live 提示词内容到已启用项: {enabled_id}");
                        state.db.save_prompt(app.as_str(), enabled_prompt)?;
                    } else {
                        // 没有已启用的提示词，则创建一次备份（避免重复备份）
                        let content_exists = prompts
                            .values()
                            .any(|p| p.content.trim() == live_content.trim());
                        if !content_exists {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64;
                            let backup_id = format!("backup-{timestamp}");
                            let backup_prompt = Prompt {
                                id: backup_id.clone(),
                                name: format!(
                                    "原始提示词 {}",
                                    chrono::Local::now().format("%Y-%m-%d %H:%M")
                                ),
                                content: live_content,
                                description: Some("自动备份的原始提示词".to_string()),
                                enabled: false,
                                created_at: Some(timestamp),
                                updated_at: Some(timestamp),
                            };
                            log::info!("回填 live 提示词内容，创建备份: {backup_id}");
                            state.db.save_prompt(app.as_str(), &backup_prompt)?;
                        }
                    }
                }
            }
        }

        // 启用目标提示词并写入文件
        let mut prompts = state.db.get_prompts(app.as_str())?;

        for prompt in prompts.values_mut() {
            prompt.enabled = false;
        }

        if let Some(prompt) = prompts.get_mut(id) {
            prompt.enabled = true;
            write_text_file(&target_path, &prompt.content)?; // 原子写入
            state.db.save_prompt(app.as_str(), prompt)?;
        } else {
            return Err(AppError::InvalidInput(format!("提示词 {id} 不存在")));
        }

        // Save all prompts to disable others
        for (_, prompt) in prompts.iter() {
            state.db.save_prompt(app.as_str(), prompt)?;
        }

        Ok(())
    }

    pub fn import_from_file(state: &AppState, app: AppType) -> Result<String, AppError> {
        let content = if matches!(app, AppType::Pi) {
            PiAgentsFileGuard::acquire()?
                .read()?
                .content
                .ok_or_else(|| AppError::Message("提示词文件不存在".to_string()))?
        } else {
            let file_path = prompt_file_path(&app)?;
            if !file_path.exists() {
                return Err(AppError::Message("提示词文件不存在".to_string()));
            }
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?
        };
        let timestamp = get_unix_timestamp()?;

        let id = format!("imported-{timestamp}");
        let prompt = Prompt {
            id: id.clone(),
            name: format!(
                "导入的提示词 {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("从现有配置文件导入".to_string()),
            enabled: false,
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        Self::upsert_prompt(state, app, &id, prompt)?;
        Ok(id)
    }

    pub fn get_current_file_content(app: AppType) -> Result<Option<String>, AppError> {
        if matches!(app, AppType::Pi) {
            return Ok(PiAgentsFileGuard::acquire()?.read()?.content);
        }
        let file_path = prompt_file_path(&app)?;
        if !file_path.exists() {
            return Ok(None);
        }
        let content =
            std::fs::read_to_string(&file_path).map_err(|e| AppError::io(&file_path, e))?;
        Ok(Some(content))
    }

    /// Project restored database state to the managed prompt file without
    /// reading stale live content back into the newly imported database.
    pub fn sync_to_live(state: &AppState, app: AppType) -> Result<(), AppError> {
        if matches!(app, AppType::ClaudeDesktop | AppType::Pi) {
            return Ok(());
        }
        let prompts = state.db.get_prompts(app.as_str())?;
        let target_path = prompt_file_path(&app)?;
        if let Some(warning) = project_prompt_set_to_path(&prompts, &target_path)? {
            return Err(AppError::Message(warning));
        }
        Ok(())
    }

    pub fn sync_all_to_live(state: &AppState) -> Result<(), AppError> {
        let mut failures = Vec::new();
        for app in AppType::all() {
            if matches!(app, AppType::ClaudeDesktop | AppType::Pi) {
                continue;
            }
            if let Err(error) = Self::sync_to_live(state, app.clone()) {
                log::warn!("同步 Prompt 到 {app:?} 失败: {error}");
                failures.push(format!("{}: {error}", app.as_str()));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "部分应用 Prompt 同步失败: {}",
                failures.join("; ")
            )))
        }
    }

    /// 首次启动时从现有提示词文件自动导入（如果存在）
    /// 返回导入的数量
    pub fn import_from_file_on_first_launch(
        state: &AppState,
        app: AppType,
    ) -> Result<usize, AppError> {
        // 幂等性保护：该应用已有提示词则跳过
        let existing = state.db.get_prompts(app.as_str())?;
        if !existing.is_empty() {
            return Ok(0);
        }

        let file_path = prompt_file_path(&app)?;
        let content = if matches!(app, AppType::Pi) {
            match PiAgentsFileGuard::acquire().and_then(|guard| guard.read()) {
                Ok(snapshot) => match snapshot.content {
                    Some(content) => content,
                    None => return Ok(0),
                },
                Err(error) => {
                    log::warn!("读取提示词文件失败: {file_path:?}, 错误: {error}");
                    return Ok(0);
                }
            }
        } else {
            if !file_path.exists() {
                return Ok(0);
            }
            match std::fs::read_to_string(&file_path) {
                Ok(content) => content,
                Err(error) => {
                    log::warn!("读取提示词文件失败: {file_path:?}, 错误: {error}");
                    return Ok(0);
                }
            }
        };

        // 检查内容是否为空
        if content.trim().is_empty() {
            return Ok(0);
        }

        log::info!("发现提示词文件，自动导入: {file_path:?}");

        // 创建提示词对象
        let timestamp = get_unix_timestamp()?;
        let id = format!("auto-imported-{timestamp}");
        let prompt = Prompt {
            id: id.clone(),
            name: format!(
                "Auto-imported Prompt {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M")
            ),
            content,
            description: Some("Automatically imported on first launch".to_string()),
            // Pi derives active state from AGENTS.md. Its DB row is a library
            // entry and must never become a second source of truth.
            enabled: !matches!(app, AppType::Pi),
            created_at: Some(timestamp),
            updated_at: Some(timestamp),
        };

        // 保存到数据库
        state.db.save_prompt(app.as_str(), &prompt)?;

        log::info!("自动导入完成: {}", app.as_str());
        Ok(1)
    }
}

fn pi_active_prompt_id(
    prompts: &IndexMap<String, Prompt>,
    live_content: Option<&str>,
) -> Option<String> {
    let live_content = live_content?;
    prompts
        .iter()
        .find(|(_, prompt)| prompt.content == live_content)
        .map(|(id, _)| id.clone())
}

fn unique_pi_backup_id(prompts: &IndexMap<String, Prompt>, timestamp: i64) -> String {
    let base = format!("backup-{timestamp}");
    if !prompts.contains_key(&base) {
        return base;
    }
    for suffix in 2_u64.. {
        let candidate = format!("{base}-{suffix}");
        if !prompts.contains_key(&candidate) {
            return candidate;
        }
    }
    unreachable!("the backup suffix space is finite only after u64 exhaustion")
}

fn get_pi_prompts(state: &AppState) -> Result<IndexMap<String, Prompt>, AppError> {
    let guard = PiAgentsFileGuard::acquire()?;
    let mut prompts = state.db.get_prompts(AppType::Pi.as_str())?;
    let snapshot = guard.read()?;
    let active_id = pi_active_prompt_id(&prompts, snapshot.content.as_deref());
    for (id, prompt) in &mut prompts {
        prompt.enabled = active_id.as_ref() == Some(id);
    }
    Ok(prompts)
}

fn upsert_pi_prompt(state: &AppState, id: &str, prompt: Prompt) -> Result<(), AppError> {
    if prompt.id != id {
        return Err(AppError::InvalidInput(
            "Pi prompt id does not match the requested id".to_string(),
        ));
    }

    let guard = PiAgentsFileGuard::acquire()?;
    let prompts = state.db.get_prompts(AppType::Pi.as_str())?;
    let snapshot = guard.read()?;
    let was_active =
        pi_active_prompt_id(&prompts, snapshot.content.as_deref()).as_deref() == Some(id);
    let previous = prompts.get(id).cloned();
    let requested_active = prompt.enabled;
    let mut stored = prompt;
    stored.enabled = false;

    if requested_active && !was_active {
        return Err(AppError::Conflict(
            "Pi AGENTS.md changed outside CC Switch; reload before editing it".to_string(),
        ));
    }

    persist_pi_prompt_with_native_update(state, id, &stored, previous.as_ref(), || {
        if requested_active {
            guard.replace(&snapshot.revision, &stored.content)
        } else if was_active {
            guard.delete(&snapshot.revision)
        } else {
            Ok(())
        }
    })
}

fn persist_pi_prompt_with_native_update(
    state: &AppState,
    id: &str,
    stored: &Prompt,
    previous: Option<&Prompt>,
    update_native: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    state.db.save_prompt(AppType::Pi.as_str(), stored)?;
    if let Err(native_error) = update_native() {
        let rollback = match previous {
            Some(previous) => state.db.save_prompt(AppType::Pi.as_str(), previous),
            None => state.db.delete_prompt(AppType::Pi.as_str(), id),
        };
        if let Err(rollback_error) = rollback {
            return Err(AppError::Message(format!(
                "Pi prompt update failed ({native_error}); database rollback also failed: {rollback_error}"
            )));
        }
        return Err(native_error);
    }
    Ok(())
}

fn enable_pi_prompt(state: &AppState, id: &str) -> Result<(), AppError> {
    let guard = PiAgentsFileGuard::acquire()?;
    let prompts = state.db.get_prompts(AppType::Pi.as_str())?;
    let target = prompts
        .get(id)
        .cloned()
        .ok_or_else(|| AppError::InvalidInput(format!("提示词 {id} 不存在")))?;
    let snapshot = guard.read()?;

    if let Some(content) = snapshot.content.as_ref() {
        let already_saved = prompts.values().any(|prompt| prompt.content == *content);
        if !content.trim().is_empty() && !already_saved {
            let timestamp = get_unix_timestamp()?;
            let backup = Prompt {
                id: unique_pi_backup_id(&prompts, timestamp),
                name: format!(
                    "原始提示词 {}",
                    chrono::Local::now().format("%Y-%m-%d %H:%M")
                ),
                content: content.clone(),
                description: Some("自动备份的原始提示词".to_string()),
                enabled: false,
                created_at: Some(timestamp),
                updated_at: Some(timestamp),
            };
            state.db.save_prompt(AppType::Pi.as_str(), &backup)?;
        }
    }

    guard.replace(&snapshot.revision, &target.content)
}

fn delete_pi_prompt(state: &AppState, id: &str) -> Result<(), AppError> {
    let guard = PiAgentsFileGuard::acquire()?;
    let prompts = state.db.get_prompts(AppType::Pi.as_str())?;
    let snapshot = guard.read()?;
    if pi_active_prompt_id(&prompts, snapshot.content.as_deref()).as_deref() == Some(id) {
        return Err(AppError::InvalidInput("无法删除已启用的提示词".to_string()));
    }
    state.db.delete_prompt(AppType::Pi.as_str(), id)?;
    Ok(())
}

#[cfg(test)]
mod upstream_sync_reliability_tests {
    use super::project_prompt_set_to_path;
    use crate::prompt::Prompt;
    use indexmap::IndexMap;

    fn prompt(id: &str, content: &str, enabled: bool) -> Prompt {
        Prompt {
            id: id.to_string(),
            name: id.to_string(),
            content: content.to_string(),
            description: None,
            enabled,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn upstream_sync_reliability_prompt_projection_uses_restored_database_content() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        std::fs::write(&path, "stale live content").expect("seed stale live prompt");
        let mut prompts = IndexMap::new();
        prompts.insert("off".to_string(), prompt("off", "old", false));
        prompts.insert("on".to_string(), prompt("on", "restored", true));

        let warning = project_prompt_set_to_path(&prompts, &path).expect("project prompt");
        assert!(warning.is_none());
        assert_eq!(std::fs::read_to_string(path).expect("read"), "restored");
    }

    #[test]
    fn upstream_sync_reliability_prompt_projection_is_deterministic_on_invalid_duplicates() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        let mut prompts = IndexMap::new();
        prompts.insert("first".to_string(), prompt("first", "first body", true));
        prompts.insert("second".to_string(), prompt("second", "second body", true));

        let warning = project_prompt_set_to_path(&prompts, &path)
            .expect("project prompt")
            .expect("duplicate enabled prompts warn");
        assert!(warning.contains("first, second"));
        assert_eq!(std::fs::read_to_string(path).expect("read"), "first body");
    }

    #[test]
    fn upstream_sync_reliability_prompt_projection_preserves_unmanaged_live_content() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("AGENTS.md");
        std::fs::write(&path, "local unmanaged content").expect("seed live prompt");
        let mut prompts = IndexMap::new();
        prompts.insert("off".to_string(), prompt("off", "managed", false));

        let warning = project_prompt_set_to_path(&prompts, &path).expect("project prompt");
        assert!(warning.is_none());
        assert_eq!(
            std::fs::read_to_string(path).expect("read"),
            "local unmanaged content"
        );
    }
}

#[cfg(test)]
mod upstream_pi_prompt_ownership_tests {
    use super::*;
    use crate::database::Database;
    use crate::services::pi_prompt_files::{
        PiPromptFileKind, PiPromptFileService, PiPromptTemplateService,
    };
    use serial_test::serial;
    use std::sync::Arc;

    fn prompt(id: &str, content: &str, enabled: bool) -> Prompt {
        Prompt {
            id: id.to_string(),
            name: id.to_string(),
            content: content.to_string(),
            description: None,
            enabled,
            created_at: Some(1),
            updated_at: Some(1),
        }
    }

    fn with_pi_dir<T>(test: impl FnOnce(&std::path::Path) -> Result<T, AppError>) -> T {
        assert!(crate::settings::get_pi_override_dir().is_none());
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let previous = std::env::var_os("PI_CODING_AGENT_DIR");
        unsafe { std::env::set_var("PI_CODING_AGENT_DIR", dir.path()) };
        let result = test(dir.path());
        match previous {
            Some(value) => unsafe { std::env::set_var("PI_CODING_AGENT_DIR", value) },
            None => unsafe { std::env::remove_var("PI_CODING_AGENT_DIR") },
        }
        result.expect("Pi prompt ownership test")
    }

    #[test]
    #[serial]
    fn upstream_pi_generic_restore_preserves_native_agents_file() {
        assert!(crate::settings::get_pi_override_dir().is_none());
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let previous = std::env::var_os("PI_CODING_AGENT_DIR");
        unsafe { std::env::set_var("PI_CODING_AGENT_DIR", dir.path()) };
        let result = (|| {
            let state = AppState::new(Arc::new(Database::memory()?));
            let path = dir.path().join("AGENTS.md");
            std::fs::write(&path, "native instructions").map_err(|e| AppError::io(&path, e))?;

            PromptService::sync_to_live(&state, AppType::Pi)?;

            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                "native instructions"
            );
            Ok::<(), AppError>(())
        })();
        match previous {
            Some(value) => unsafe { std::env::set_var("PI_CODING_AGENT_DIR", value) },
            None => unsafe { std::env::remove_var("PI_CODING_AGENT_DIR") },
        }
        result.expect("preserve Pi AGENTS.md");
    }

    #[test]
    #[serial]
    fn upstream_pi_native_prompt_files_use_revision_cas_and_portable_template_slugs() {
        assert!(crate::settings::get_pi_override_dir().is_none());
        let dir = tempfile::tempdir().expect("Pi agent directory");
        let previous = std::env::var_os("PI_CODING_AGENT_DIR");
        unsafe { std::env::set_var("PI_CODING_AGENT_DIR", dir.path()) };
        let result = (|| {
            let initial = PiPromptFileService::read(PiPromptFileKind::SystemOverride)?;
            assert!(!initial.exists);
            std::fs::write(dir.path().join("SYSTEM.md"), "external")
                .map_err(|e| AppError::io(dir.path().join("SYSTEM.md"), e))?;
            assert!(matches!(
                PiPromptFileService::replace(
                    PiPromptFileKind::SystemOverride,
                    &initial.revision,
                    "managed"
                ),
                Err(AppError::Conflict(_))
            ));
            assert!(PiPromptTemplateService::upsert("../escape", None, "missing", "body").is_err());
            assert!(!dir.path().join("escape.md").exists());
            Ok::<(), AppError>(())
        })();
        match previous {
            Some(value) => unsafe { std::env::set_var("PI_CODING_AGENT_DIR", value) },
            None => unsafe { std::env::remove_var("PI_CODING_AGENT_DIR") },
        }
        result.expect("Pi native prompt ownership");
    }

    #[test]
    #[serial]
    fn upstream_pi_active_prompt_is_derived_from_agents_file() {
        with_pi_dir(|dir| {
            let state = AppState::new(Arc::new(Database::memory()?));
            state.db.save_prompt(
                AppType::Pi.as_str(),
                &prompt("managed", "managed content", true),
            )?;

            let saved = PromptService::get_prompts(&state, AppType::Pi)?;
            assert!(!saved["managed"].enabled);

            let path = dir.join("AGENTS.md");
            std::fs::write(&path, "managed content").map_err(|e| AppError::io(&path, e))?;
            let active = PromptService::get_prompts(&state, AppType::Pi)?;
            assert!(active["managed"].enabled);

            std::fs::write(&path, "external edit").map_err(|e| AppError::io(&path, e))?;
            assert!(matches!(
                PromptService::upsert_prompt(
                    &state,
                    AppType::Pi,
                    "managed",
                    prompt("managed", "edited content", true),
                ),
                Err(AppError::Conflict(_))
            ));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "external edit");
            Ok(())
        });
    }

    #[test]
    #[serial]
    fn upstream_pi_inactive_duplicate_edit_preserves_agents_file() {
        with_pi_dir(|dir| {
            let state = AppState::new(Arc::new(Database::memory()?));
            let active = prompt("active", "same content", false);
            let duplicate = prompt("duplicate", "same content", false);
            state.db.save_prompt(AppType::Pi.as_str(), &active)?;
            state.db.save_prompt(AppType::Pi.as_str(), &duplicate)?;
            let path = dir.join("AGENTS.md");
            std::fs::write(&path, "same content").map_err(|e| AppError::io(&path, e))?;

            let hydrated = PromptService::get_prompts(&state, AppType::Pi)?;
            assert!(hydrated["active"].enabled);
            assert!(!hydrated["duplicate"].enabled);

            PromptService::upsert_prompt(
                &state,
                AppType::Pi,
                "duplicate",
                prompt("duplicate", "edited duplicate", false),
            )?;
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "same content");
            Ok(())
        });
    }

    #[test]
    #[serial]
    fn upstream_pi_disabling_active_prompt_deletes_agents_file() {
        with_pi_dir(|dir| {
            let state = AppState::new(Arc::new(Database::memory()?));
            let managed = prompt("managed", "managed content", false);
            state.db.save_prompt(AppType::Pi.as_str(), &managed)?;
            let path = dir.join("AGENTS.md");
            std::fs::write(&path, "managed content").map_err(|e| AppError::io(&path, e))?;

            PromptService::upsert_prompt(&state, AppType::Pi, "managed", managed)?;
            assert!(!path.exists());
            Ok(())
        });
    }

    #[test]
    #[serial]
    fn upstream_pi_enable_backs_up_unmanaged_agents_content_once() {
        with_pi_dir(|dir| {
            let state = AppState::new(Arc::new(Database::memory()?));
            state.db.save_prompt(
                AppType::Pi.as_str(),
                &prompt("managed", "managed content", false),
            )?;
            let path = dir.join("AGENTS.md");
            std::fs::write(&path, "unmanaged content").map_err(|e| AppError::io(&path, e))?;

            PromptService::enable_prompt(&state, AppType::Pi, "managed")?;
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "managed content");
            PromptService::enable_prompt(&state, AppType::Pi, "managed")?;
            let prompts = state.db.get_prompts(AppType::Pi.as_str())?;
            assert_eq!(
                prompts
                    .values()
                    .filter(|value| value.content == "unmanaged content")
                    .count(),
                1
            );
            assert!(prompts.values().all(|value| !value.enabled));
            Ok(())
        });
    }

    #[test]
    #[serial]
    fn upstream_pi_native_write_failure_restores_database_prompt() {
        with_pi_dir(|_| {
            let state = AppState::new(Arc::new(Database::memory()?));
            let previous = prompt("managed", "before", false);
            state.db.save_prompt(AppType::Pi.as_str(), &previous)?;
            let edited = prompt("managed", "after", false);

            let error = persist_pi_prompt_with_native_update(
                &state,
                "managed",
                &edited,
                Some(&previous),
                || Err(AppError::Message("native write failed".to_string())),
            )
            .expect_err("native failure must be returned");
            assert!(error.to_string().contains("native write failed"));
            let prompts = state.db.get_prompts(AppType::Pi.as_str())?;
            assert_eq!(prompts["managed"].content, "before");
            Ok(())
        });
    }
}
