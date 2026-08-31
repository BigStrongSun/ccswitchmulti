//! 环境变量自动注入。
//!
//! 把用户在设置里配置的变量写进各 CLI **自己的配置文件**，从而在不改系统
//! 环境、不启用本地代理的前提下，让 CLI 以指定的环境变量运行。
//!
//! 之所以坚持走 CLI 官方配置而不是 shell rc 或 PATH shim：
//! - shell rc 里的 `export` 是全局的，会污染所有进程，与「只影响这一个
//!   CLI」的目标相悖；
//! - shim 需要改 PATH，侵入性强且容易被 CLI 自更新破坏；
//! - 写进 CLI 自己的配置后，**用户不启用本地代理、直连官方**时同样生效，
//!   因为 CLI 每次启动都会读自己的配置文件。
//!
//! 各 CLI 的官方依据与能力边界：
//! - Claude Code：`~/.claude/settings.json` 的 `env` 对象。官方文档明确
//!   「Claude Code 在启动时直接从文件读取它们，因此无论如何启动 claude，
//!   它们都会生效」——作用于主进程与其子进程。
//! - Codex CLI：`~/.codex/config.toml` 的 `[shell_environment_policy] set`。
//!   官方文档说明该节控制的是「Codex 传给其所派生子进程的环境变量」，
//!   **不含 Codex 主进程自身**。对「让 agent 执行的命令看到某个时区」够用，
//!   但不改变 Codex 主进程自己的行为。
//! - Gemini CLI：官方 `settings.json` 没有进程级 env 字段（只有
//!   `mcpServers.*.env`，且值仅支持 `$VAR` 引用展开），因此**不支持**，
//!   不提供该目标，避免给出「开关开了但没效果」的错觉。

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};
use toml_edit::{DocumentMut, Item, Table};

use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::error::AppError;
use crate::settings::{EnvInjectionSettings, EnvInjectionTarget};

/// 把变量合并进 Claude settings.json 的 `env` 对象。
///
/// **只补缺、不覆盖**：provider `settings_config.env` 里已有的同名键一律保留，
/// 避免全局变量意外顶掉 `ANTHROPIC_BASE_URL` 之类路由变量而打乱供应商切换。
pub fn merge_into_claude_settings(settings: &mut Value, vars: &BTreeMap<String, String>) {
    if vars.is_empty() {
        return;
    }
    let Some(obj) = settings.as_object_mut() else {
        return;
    };
    let env = obj
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(env_map) = env.as_object_mut() else {
        // 用户把 env 写成了非对象类型，交给用户自己处理，不覆盖
        log::warn!("Claude settings.json 的 env 不是对象，跳过环境变量注入");
        return;
    };
    for (key, value) in vars {
        if !env_map.contains_key(key) {
            env_map.insert(key.clone(), Value::String(value.clone()));
        }
    }
}

/// 从 Claude settings.json 的 `env` 中移除注入的变量。
///
/// **只删值完全匹配的条目**——用户如果自己改过值，说明那已经是他的配置，
/// 不再由本功能托管。
pub fn remove_from_claude_settings(settings: &mut Value, vars: &BTreeMap<String, String>) {
    if vars.is_empty() {
        return;
    }
    let Some(obj) = settings.as_object_mut() else {
        return;
    };
    let Some(env_map) = obj.get_mut("env").and_then(Value::as_object_mut) else {
        return;
    };
    for (key, value) in vars {
        let injected = env_map.get(key).and_then(Value::as_str) == Some(value.as_str());
        if injected {
            env_map.remove(key);
        }
    }
}

/// Codex `config.toml` 是否配置了 include 型 allowlist。
///
/// 官方文档写明 include 模式的 allowlist 会过滤掉 `set` 恢复出来的值
/// （"An include-pattern allowlist can still remove that restored value"），
/// 一旦存在，注入的变量可能不生效，需要在 UI 上提示。
pub fn codex_env_policy_has_include_allowlist(config_text: &str) -> bool {
    let Ok(doc) = config_text.parse::<DocumentMut>() else {
        return false;
    };
    doc.get("shell_environment_policy")
        .and_then(Item::as_table)
        .and_then(|table| table.get("include_only"))
        .and_then(Item::as_array)
        .map(|arr| !arr.is_empty())
        .unwrap_or(false)
}

/// 把变量写入 Codex `config.toml` 的 `[shell_environment_policy] set`。
///
/// 同样遵循「只补缺、不覆盖」。
pub fn merge_into_codex_config_text(
    config_text: &str,
    vars: &BTreeMap<String, String>,
) -> Result<String, AppError> {
    if vars.is_empty() {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text.parse::<DocumentMut>().map_err(|error| {
        AppError::Config(format!(
            "Codex config.toml 解析失败，无法注入环境变量: {error}"
        ))
    })?;

    {
        let policy_item = doc
            .entry("shell_environment_policy")
            .or_insert(Item::Table(Table::new()));
        let policy_table = policy_item.as_table_mut().ok_or_else(|| {
            AppError::Config("Codex config.toml 的 shell_environment_policy 不是表".to_string())
        })?;
        let set_item = policy_table
            .entry("set")
            .or_insert(Item::Table(Table::new()));
        let set_table = set_item.as_table_mut().ok_or_else(|| {
            AppError::Config("Codex config.toml 的 shell_environment_policy.set 不是表".to_string())
        })?;
        for (key, value) in vars {
            if !set_table.contains_key(key) {
                set_table.insert(key, toml_edit::value(value.as_str()));
            }
        }
    }

    Ok(doc.to_string())
}

/// 从 Codex `config.toml` 的 `[shell_environment_policy] set` 中移除注入的变量。
///
/// 同样只删值匹配的条目。移除后若 `set` 变空会连带清掉空表，避免留下垃圾配置。
pub fn remove_from_codex_config_text(
    config_text: &str,
    vars: &BTreeMap<String, String>,
) -> Result<String, AppError> {
    if vars.is_empty() {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text.parse::<DocumentMut>().map_err(|error| {
        AppError::Config(format!(
            "Codex config.toml 解析失败，无法移除环境变量: {error}"
        ))
    })?;

    {
        let Some(policy_table) = doc
            .get_mut("shell_environment_policy")
            .and_then(Item::as_table_mut)
        else {
            return Ok(config_text.to_string());
        };
        let Some(set_table) = policy_table.get_mut("set").and_then(Item::as_table_mut) else {
            return Ok(config_text.to_string());
        };
        for (key, value) in vars {
            let injected = set_table.get(key).and_then(Item::as_str) == Some(value.as_str());
            if injected {
                set_table.remove(key);
            }
        }
    }

    // `set` 被清空后连带清理空表，避免留下垃圾配置
    let set_now_empty = doc
        .get("shell_environment_policy")
        .and_then(Item::as_table)
        .and_then(|table| table.get("set"))
        .and_then(Item::as_table)
        .is_some_and(toml_edit::Table::is_empty);

    if set_now_empty {
        if let Some(policy_table) = doc
            .get_mut("shell_environment_policy")
            .and_then(Item::as_table_mut)
        {
            policy_table.remove("set");
        }
    }

    Ok(doc.to_string())
}

/// 对 Claude live 配置做一次「移除旧的 + 注入新的」。
fn apply_claude(
    previous: &EnvInjectionSettings,
    next: &EnvInjectionSettings,
) -> Result<(), AppError> {
    let previous_vars = previous.variables_for(EnvInjectionTarget::Claude);
    let next_vars = next.variables_for(EnvInjectionTarget::Claude);
    if previous_vars.is_empty() && next_vars.is_empty() {
        return Ok(());
    }

    let path = get_claude_settings_path();
    // 文件不存在且这次没有要注入的内容时，不做任何事（不要凭空创建配置）
    if !path.exists() && next_vars.is_empty() {
        return Ok(());
    }

    let mut settings: Value = if path.exists() {
        read_json_file::<Value>(&path).unwrap_or_else(|error| {
            log::warn!("读取 Claude settings.json 失败，按空配置处理: {error}");
            Value::Object(Map::new())
        })
    } else {
        Value::Object(Map::new())
    };

    remove_from_claude_settings(&mut settings, &previous_vars);
    merge_into_claude_settings(&mut settings, &next_vars);
    write_json_file(&path, &settings)?;
    log::info!("已把环境变量注入同步到 Claude settings.json");
    Ok(())
}

/// 对 Codex live 配置做一次「移除旧的 + 注入新的」。
fn apply_codex(
    previous: &EnvInjectionSettings,
    next: &EnvInjectionSettings,
) -> Result<(), AppError> {
    let previous_vars = previous.variables_for(EnvInjectionTarget::Codex);
    let next_vars = next.variables_for(EnvInjectionTarget::Codex);
    if previous_vars.is_empty() && next_vars.is_empty() {
        return Ok(());
    }

    crate::codex_config::reconcile_codex_live_config_atomic(|live| {
        let cleaned = remove_from_codex_config_text(live, &previous_vars)?;
        merge_into_codex_config_text(&cleaned, &next_vars)
    })?;
    log::info!("已把环境变量注入同步到 Codex config.toml");
    Ok(())
}

/// 开关或变量发生变化后，把注入结果同步到所有支持的 CLI 配置。
///
/// 只处理 Claude 与 Codex；Gemini 官方配置没有对应的写入位置。
/// 调用方应忽略返回值里的失败——设置本身已经存盘，不该因为某
/// 一个 CLI 的配置写不动就回滚用户的设置。
pub fn sync_to_live_configs(
    previous: &EnvInjectionSettings,
    next: &EnvInjectionSettings,
) -> Result<(), AppError> {
    let mut first_error: Option<AppError> = None;

    if let Err(error) = apply_claude(previous, next) {
        log::warn!("同步环境变量注入到 Claude 失败: {error}");
        first_error.get_or_insert(error);
    }
    if let Err(error) = apply_codex(previous, next) {
        log::warn!("同步环境变量注入到 Codex 失败: {error}");
        first_error.get_or_insert(error);
    }

    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// 供应商切换写 live 配置时调用：把当前注入的变量补进即将写入的内容。
///
/// Claude 走这条路径，因为 `live.rs` 写 settings.json 前会构造完整对象。
pub fn inject_into_claude_live(settings: &mut Value) {
    let current = crate::settings::get_settings();
    merge_into_claude_settings(
        settings,
        &current
            .env_injection
            .variables_for(EnvInjectionTarget::Claude),
    );
}

/// 把注入过的变量从 Claude settings.json 中摘掉。
///
/// 用于「写 live 配置后再读回、存进供应商记录」的场景：注入是全局的、由设置
/// 单独管理，不能被固化进某一条供应商记录——否则关掉开关后残留值会跟着该
/// 供应商到处跑，还会污染备份与云同步。
pub fn strip_from_claude_settings(settings: &mut Value) {
    let vars = crate::settings::get_settings()
        .env_injection
        .variables_for(EnvInjectionTarget::Claude);
    remove_from_claude_settings(settings, &vars);
}

/// 当前 live 配置里已知的、会让注入失效的冲突。
///
/// 前端据此给出提示，避免用户看到「开关开了但没效果」却不知道原因。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvInjectionConflicts {
    /// Codex 配了 include 型 allowlist，官方会用它过滤掉 `set` 恢复出来的值，
    /// 此时注入的变量不会生效。
    pub codex_include_allowlist: bool,
}

/// 读取当前 live 配置，检查是否存在会让注入失效的冲突。
///
/// 只读，失败一律按「没有冲突」处理——这只是提示信息，不该阻塞 UI。
pub fn inspect_conflicts() -> EnvInjectionConflicts {
    let codex_include_allowlist = crate::codex_config::read_codex_config_text()
        .inspect_err(|error| {
            log::debug!("读取 Codex config.toml 失败，跳过注入冲突检查: {error}");
        })
        .is_ok_and(|text| {
            let conflict = codex_env_policy_has_include_allowlist(&text);
            if conflict {
                log::warn!(
                    "Codex config.toml 存在 shell_environment_policy.include_only，\
                     注入的环境变量可能不会生效"
                );
            }
            conflict
        });

    EnvInjectionConflicts {
        codex_include_allowlist,
    }
}

/// 供应商切换写 live 配置时调用：把当前注入的变量补进即将写入的 TOML 文本。
pub fn inject_into_codex_live(config_text: &str) -> Result<String, AppError> {
    let current = crate::settings::get_settings();
    merge_into_codex_config_text(
        config_text,
        &current
            .env_injection
            .variables_for(EnvInjectionTarget::Codex),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn claude_merge_does_not_overwrite_existing_keys() {
        let mut settings = serde_json::json!({
            "env": { "ANTHROPIC_BASE_URL": "https://provider.example" }
        });
        merge_into_claude_settings(
            &mut settings,
            &vars(&[
                ("TZ", "Asia/Shanghai"),
                ("ANTHROPIC_BASE_URL", "https://evil.example"),
            ]),
        );
        let env = settings["env"].as_object().unwrap();
        assert_eq!(env["TZ"], "Asia/Shanghai");
        // provider 的路由变量绝不能被顶掉
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://provider.example");
    }

    #[test]
    fn claude_remove_only_touches_matching_values() {
        let mut settings = serde_json::json!({
            "env": { "TZ": "Asia/Shanghai", "KEEP": "user-value" }
        });
        remove_from_claude_settings(
            &mut settings,
            &vars(&[("TZ", "Asia/Shanghai"), ("KEEP", "injected-value")]),
        );
        let env = settings["env"].as_object().unwrap();
        assert!(env.get("TZ").is_none());
        // 值不匹配说明是用户自己的配置，必须保留
        assert_eq!(env["KEEP"], "user-value");
    }

    #[test]
    fn codex_merge_writes_shell_environment_policy_set() {
        let merged =
            merge_into_codex_config_text("model = \"gpt-5\"\n", &vars(&[("TZ", "Asia/Shanghai")]))
                .unwrap();
        assert!(merged.contains("[shell_environment_policy.set]"));
        assert!(merged.contains("TZ = \"Asia/Shanghai\""));
        // 原有内容必须保留
        assert!(merged.contains("model = \"gpt-5\""));
        // 结果必须是合法 TOML
        merged.parse::<DocumentMut>().unwrap();
    }

    #[test]
    fn codex_merge_does_not_overwrite_existing_values() {
        let base = "[shell_environment_policy.set]\nTZ = \"UTC\"\n";
        let merged = merge_into_codex_config_text(base, &vars(&[("TZ", "Asia/Shanghai")])).unwrap();
        assert!(merged.contains("TZ = \"UTC\""));
        assert!(!merged.contains("Asia/Shanghai"));
    }

    #[test]
    fn codex_remove_drops_empty_set_table() {
        let base = "model = \"gpt-5\"\n\n[shell_environment_policy.set]\nTZ = \"Asia/Shanghai\"\n";
        let cleaned =
            remove_from_codex_config_text(base, &vars(&[("TZ", "Asia/Shanghai")])).unwrap();
        assert!(!cleaned.contains("Asia/Shanghai"));
        assert!(!cleaned.contains("shell_environment_policy"));
        assert!(cleaned.contains("model = \"gpt-5\""));
    }

    #[test]
    fn codex_detects_include_allowlist_conflict() {
        let with_allowlist = "[shell_environment_policy]\ninclude_only = [\"PATH\", \"HOME\"]\n";
        assert!(codex_env_policy_has_include_allowlist(with_allowlist));
        assert!(!codex_env_policy_has_include_allowlist(
            "[shell_environment_policy.set]\nTZ = \"UTC\"\n"
        ));
    }

    #[test]
    fn empty_variables_leave_config_untouched() {
        let base = "model = \"gpt-5\"\n";
        assert_eq!(
            merge_into_codex_config_text(base, &BTreeMap::new()).unwrap(),
            base
        );
        let mut settings = serde_json::json!({ "env": {} });
        merge_into_claude_settings(&mut settings, &BTreeMap::new());
        assert_eq!(settings, serde_json::json!({ "env": {} }));
    }
}
