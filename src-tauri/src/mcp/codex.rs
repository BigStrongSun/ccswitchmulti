//! Codex MCP 同步和导入模块
//!
//! 包含 Codex 的 MCP 配置管理：
//! - 从 ~/.codex/config.toml 导入
//! - 同步到 ~/.codex/config.toml
//! - JSON 到 TOML 的转换逻辑

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

fn should_sync_codex_mcp() -> bool {
    // Codex 未安装/未初始化时：~/.codex 目录不存在。
    // 按用户偏好：目录缺失时跳过写入/删除，不创建任何文件或目录。
    crate::codex_config::get_codex_config_dir().exists()
}

/// 返回已启用的 MCP 服务器（过滤 enabled==true）
fn collect_enabled_servers(cfg: &McpConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (id, entry) in cfg.servers.iter() {
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        match extract_server_spec(entry) {
            Ok(spec) => {
                out.insert(id.clone(), spec);
            }
            Err(err) => {
                log::warn!("跳过无效的 MCP 条目 '{id}': {err}");
            }
        }
    }
    out
}

const CODEX_MCP_RECONCILE_MAX_ATTEMPTS: usize = 3;

#[cfg(test)]
static TEST_RECONCILE_MUTATIONS: OnceLock<Mutex<Vec<Vec<u8>>>> = OnceLock::new();

#[cfg(test)]
fn set_test_reconcile_mutations(contents: Vec<Vec<u8>>) {
    *TEST_RECONCILE_MUTATIONS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("reconcile mutation lock") = contents;
}

fn read_codex_mcp_bytes(path: &Path) -> Result<Vec<u8>, AppError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn codex_mcp_text(bytes: &[u8], path: &Path) -> Result<String, AppError> {
    String::from_utf8(bytes.to_vec()).map_err(|error| {
        AppError::McpValidation(format!(
            "Codex config.toml 不是有效 UTF-8 ({}): {error}",
            path.display()
        ))
    })
}

/// Apply a document-level MCP projection only when the file has not changed
/// since the snapshot used to build it.  A bounded retry makes concurrent
/// edits visible to the caller instead of silently replacing them.  The final
/// write is atomic, so a failed reconcile never leaves a partial TOML file.
fn write_codex_mcp_reconciled<F>(path: &Path, reconcile: F) -> Result<(), AppError>
where
    F: Fn(&str) -> Result<String, AppError>,
{
    for attempt in 0..CODEX_MCP_RECONCILE_MAX_ATTEMPTS {
        let before = read_codex_mcp_bytes(path)?;
        let base = codex_mcp_text(&before, path)?;
        let candidate = reconcile(&base)?;

        #[cfg(test)]
        if let Some(next) = TEST_RECONCILE_MUTATIONS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .expect("reconcile mutation lock")
            .pop()
        {
            std::fs::write(path, next).map_err(|error| AppError::io(path, error))?;
        }

        let observed = read_codex_mcp_bytes(path)?;
        if observed != before {
            log::debug!(
                "Codex MCP config changed during reconcile attempt {}/{}; retrying",
                attempt + 1,
                CODEX_MCP_RECONCILE_MAX_ATTEMPTS
            );
            continue;
        }

        if candidate.as_bytes() == before.as_slice() {
            return Ok(());
        }

        crate::config::atomic_write(path, candidate.as_bytes())?;
        return Ok(());
    }

    Err(AppError::McpValidation(
        "Codex MCP 配置在有限重试后仍发生并发修改，已跳过写入".to_string(),
    ))
}

/// 从 ~/.codex/config.toml 导入 MCP 到统一结构（v3.7.0+）
///
/// 格式支持：
/// - 正确格式：[mcp_servers.*]（Codex 官方标准）
/// - 错误格式：[mcp.servers.*]（容错读取，用于迁移错误写入的配置）
///
/// 已存在的服务器将启用 Codex 应用，不覆盖其他字段和应用状态
pub fn import_from_codex(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let text = crate::codex_config::read_and_validate_codex_config_text()?;
    if text.trim().is_empty() {
        return Ok(0);
    }

    let root: toml::Table = toml::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 ~/.codex/config.toml 失败: {e}")))?;

    // 确保新结构存在
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed_total = 0usize;

    // helper：处理一组 servers 表
    let mut import_servers_tbl = |servers_tbl: &toml::value::Table| {
        let mut changed = 0usize;
        for (id, entry_val) in servers_tbl.iter() {
            let Some(entry_tbl) = entry_val.as_table() else {
                continue;
            };

            // type 缺省为 stdio
            let typ = entry_tbl
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("stdio");

            // 构建 JSON 规范
            let mut spec = serde_json::Map::new();
            spec.insert("type".into(), json!(typ));

            // 核心字段（需要手动处理的字段）
            let core_fields = match typ {
                "stdio" => vec!["type", "command", "args", "env", "cwd"],
                // DB 中的统一规范使用 headers，Codex TOML 使用 http_headers。
                // 两者都必须视为核心字段，避免鉴权值落入通用日志路径。
                "http" | "sse" => vec!["type", "url", "headers", "http_headers"],
                _ => vec!["type"],
            };

            // 1. 处理核心字段（强类型）
            match typ {
                "stdio" => {
                    if let Some(cmd) = entry_tbl.get("command").and_then(|v| v.as_str()) {
                        spec.insert("command".into(), json!(cmd));
                    }
                    if let Some(args) = entry_tbl.get("args").and_then(|v| v.as_array()) {
                        let arr = args
                            .iter()
                            .filter_map(|x| x.as_str())
                            .map(|s| json!(s))
                            .collect::<Vec<_>>();
                        if !arr.is_empty() {
                            spec.insert("args".into(), serde_json::Value::Array(arr));
                        }
                    }
                    if let Some(cwd) = entry_tbl.get("cwd").and_then(|v| v.as_str()) {
                        if !cwd.trim().is_empty() {
                            spec.insert("cwd".into(), json!(cwd));
                        }
                    }
                    if let Some(env_tbl) = entry_tbl.get("env").and_then(|v| v.as_table()) {
                        let mut env_json = serde_json::Map::new();
                        for (k, v) in env_tbl.iter() {
                            if let Some(sv) = v.as_str() {
                                env_json.insert(k.clone(), json!(sv));
                            }
                        }
                        if !env_json.is_empty() {
                            spec.insert("env".into(), serde_json::Value::Object(env_json));
                        }
                    }
                }
                "http" | "sse" => {
                    if let Some(url) = entry_tbl.get("url").and_then(|v| v.as_str()) {
                        spec.insert("url".into(), json!(url));
                    }
                    // Read from http_headers (correct Codex format) or headers (legacy) with priority to http_headers
                    let headers_tbl = entry_tbl
                        .get("http_headers")
                        .and_then(|v| v.as_table())
                        .or_else(|| entry_tbl.get("headers").and_then(|v| v.as_table()));

                    if let Some(headers_tbl) = headers_tbl {
                        let mut headers_json = serde_json::Map::new();
                        for (k, v) in headers_tbl.iter() {
                            if let Some(sv) = v.as_str() {
                                headers_json.insert(k.clone(), json!(sv));
                            }
                        }
                        if !headers_json.is_empty() {
                            spec.insert("headers".into(), serde_json::Value::Object(headers_json));
                        }
                    }
                }
                _ => {
                    log::warn!("跳过未知类型 '{typ}' 的 Codex MCP 项 '{id}'");
                    return changed;
                }
            }

            // 2. 处理扩展字段和其他未知字段（通用 TOML → JSON 转换）
            for (key, toml_val) in entry_tbl.iter() {
                // 跳过已处理的核心字段
                if core_fields.contains(&key.as_str()) {
                    continue;
                }

                // 通用 TOML 值到 JSON 值转换
                let json_val = match toml_val {
                    toml::Value::String(s) => Some(json!(s)),
                    toml::Value::Integer(i) => Some(json!(i)),
                    toml::Value::Float(f) => Some(json!(f)),
                    toml::Value::Boolean(b) => Some(json!(b)),
                    toml::Value::Array(arr) => {
                        // 只支持简单类型数组
                        let json_arr: Vec<serde_json::Value> = arr
                            .iter()
                            .filter_map(|item| match item {
                                toml::Value::String(s) => Some(json!(s)),
                                toml::Value::Integer(i) => Some(json!(i)),
                                toml::Value::Float(f) => Some(json!(f)),
                                toml::Value::Boolean(b) => Some(json!(b)),
                                _ => None,
                            })
                            .collect();
                        if !json_arr.is_empty() {
                            Some(serde_json::Value::Array(json_arr))
                        } else {
                            log::debug!("跳过复杂数组字段 '{key}' (TOML → JSON)");
                            None
                        }
                    }
                    toml::Value::Table(tbl) => {
                        // 浅层表转为 JSON 对象（仅支持字符串值）
                        let mut json_obj = serde_json::Map::new();
                        for (k, v) in tbl.iter() {
                            if let Some(s) = v.as_str() {
                                json_obj.insert(k.clone(), json!(s));
                            }
                        }
                        if !json_obj.is_empty() {
                            Some(serde_json::Value::Object(json_obj))
                        } else {
                            log::debug!("跳过复杂对象字段 '{key}' (TOML → JSON)");
                            None
                        }
                    }
                    toml::Value::Datetime(_) => {
                        log::debug!("跳过日期时间字段 '{key}' (TOML → JSON)");
                        None
                    }
                };

                if let Some(val) = json_val {
                    spec.insert(key.clone(), val);
                    log::debug!("导入扩展字段 '{key}'（值已省略）");
                }
            }

            let spec_v = serde_json::Value::Object(spec);

            // 校验：单项失败继续处理
            if let Err(e) = validate_server_spec(&spec_v) {
                log::warn!("跳过无效 Codex MCP 项 '{id}': {e}");
                continue;
            }

            if let Some(existing) = servers.get_mut(id) {
                // 已存在：仅启用 Codex 应用
                if !existing.apps.codex {
                    existing.apps.codex = true;
                    changed += 1;
                    log::info!("MCP 服务器 '{id}' 已启用 Codex 应用");
                }
            } else {
                // 新建服务器：默认仅启用 Codex
                servers.insert(
                    id.clone(),
                    McpServer {
                        id: id.clone(),
                        name: id.clone(),
                        server: spec_v,
                        apps: McpApps {
                            claude: false,
                            codex: true,
                            gemini: false,
                            grokbuild: false,
                            opencode: false,
                            hermes: false,
                        },
                        description: None,
                        homepage: None,
                        docs: None,
                        tags: Vec::new(),
                    },
                );
                changed += 1;
                log::info!("导入新 MCP 服务器 '{id}'");
            }
        }
        changed
    };

    // 1) 处理 mcp.servers
    if let Some(mcp_val) = root.get("mcp") {
        if let Some(mcp_tbl) = mcp_val.as_table() {
            if let Some(servers_val) = mcp_tbl.get("servers") {
                if let Some(servers_tbl) = servers_val.as_table() {
                    changed_total += import_servers_tbl(servers_tbl);
                }
            }
        }
    }

    // 2) 处理 mcp_servers
    if let Some(servers_val) = root.get("mcp_servers") {
        if let Some(servers_tbl) = servers_val.as_table() {
            changed_total += import_servers_tbl(servers_tbl);
        }
    }

    Ok(changed_total)
}

fn legacy_codex_servers(
    doc: &toml_edit::DocumentMut,
) -> Result<(bool, Vec<(String, toml_edit::Item)>), AppError> {
    let Some(mcp_item) = doc.get("mcp") else {
        return Ok((false, Vec::new()));
    };
    let Some(mcp_table) = mcp_item.as_table_like() else {
        return Err(AppError::McpValidation(
            "config.toml 的 mcp 不是表结构".to_string(),
        ));
    };
    let Some(servers_item) = mcp_table.get("servers") else {
        return Ok((false, Vec::new()));
    };
    let Some(servers_table) = servers_item.as_table_like() else {
        return Err(AppError::McpValidation(
            "config.toml 的 mcp.servers 不是表结构".to_string(),
        ));
    };
    Ok((
        true,
        servers_table
            .iter()
            .map(|(id, item)| (id.to_string(), item.clone()))
            .collect(),
    ))
}

/// 按数据库 ID 集合对账 Codex 的 live MCP。
///
/// 数据库中的 ID 是唯一的所有权证明。数据库没有拥有的 live-only 条目始终保留；
/// 已拥有条目才会被数据库内容更新或在禁用时删除。旧的 `[mcp.servers]` 会先迁移，
/// 再移除旧容器，避免把用户手工维护的条目一并清空。
fn reconcile_codex_document(
    doc: &mut toml_edit::DocumentMut,
    owned_ids: &HashSet<String>,
    enabled: &HashMap<String, Value>,
) -> Result<(), AppError> {
    let (has_legacy_table, legacy_entries) = legacy_codex_servers(doc)?;

    // 先转换数据库条目，再借用 live 表。转换失败时调用方尚未写入文件，
    // 因而可以保持原始字节不变。
    let mut database_entries = Vec::new();
    let mut enabled_ids: Vec<&String> = enabled.keys().collect();
    enabled_ids.sort();
    for id in enabled_ids {
        let spec = enabled
            .get(id)
            .expect("enabled id must have a corresponding server spec");
        database_entries.push((
            id.clone(),
            toml_edit::Item::Table(json_server_to_toml_table(spec)?),
        ));
    }

    let has_live_table = doc.get("mcp_servers").is_some();
    let needs_target_table = has_live_table || has_legacy_table || !database_entries.is_empty();
    if needs_target_table {
        if !has_live_table {
            doc["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let servers = doc
            .get_mut("mcp_servers")
            .and_then(toml_edit::Item::as_table_like_mut)
            .ok_or_else(|| {
                AppError::McpValidation("config.toml 的 mcp_servers 不是表结构".to_string())
            })?;

        // 迁移旧格式时，新的规范表优先；数据库拥有的 ID 由数据库内容覆盖。
        for (id, item) in legacy_entries {
            if owned_ids.contains(&id) {
                continue;
            }
            if !servers.contains_key(&id) {
                servers.insert(&id, item);
            }
        }

        for (id, item) in database_entries {
            let should_update = servers
                .get(&id)
                .map(|existing| !mcp_items_semantically_equal(existing, &item))
                .unwrap_or(true);
            if should_update {
                servers.insert(&id, item);
            }
        }

        // 只有同时存在于数据库且未启用 Codex 的 ID 才能被自动删除。
        let mut disabled_owned_ids: Vec<&String> = owned_ids
            .iter()
            .filter(|id| !enabled.contains_key(*id))
            .collect();
        disabled_owned_ids.sort();
        for id in disabled_owned_ids {
            servers.remove(id);
        }
    }

    // 删除旧容器，但保留 [mcp] 下的其他用户字段。
    if has_legacy_table {
        if let Some(mcp_table) = doc.get_mut("mcp").and_then(|item| item.as_table_like_mut()) {
            mcp_table.remove("servers");
        }
    }

    Ok(())
}

fn mcp_items_semantically_equal(left: &toml_edit::Item, right: &toml_edit::Item) -> bool {
    let left_text = left.to_string();
    let right_text = right.to_string();
    match (
        left_text.parse::<toml::Value>(),
        right_text.parse::<toml::Value>(),
    ) {
        (Ok(left_value), Ok(right_value)) => left_value == right_value,
        _ => left_text == right_text,
    }
}

/// 将 config.json 中 Codex 的 enabled==true 项以 TOML 形式写入 ~/.codex/config.toml
///
/// 格式策略：
/// - 唯一正确格式：[mcp_servers] 顶层表（Codex 官方标准）
/// - 自动清理错误格式：[mcp.servers]（如果存在）
/// - 读取现有 config.toml；若语法无效则报错，不尝试覆盖
/// - 仅更新 `mcp_servers` 表，保留其它键
/// - 仅写入启用项；无启用项时清理 mcp_servers 表
pub fn sync_enabled_to_codex(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.codex);
    let owned_ids = config.mcp.codex.servers.keys().cloned().collect();
    sync_enabled_to_codex_with_ownership(&owned_ids, &enabled)
}

/// 以数据库完整所有权集合对账 live MCP；空数据库不会删除 live-only 条目。
pub fn sync_enabled_to_codex_with_ownership(
    owned_ids: &HashSet<String>,
    enabled: &HashMap<String, Value>,
) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    let path = crate::codex_config::get_codex_config_path();
    write_codex_mcp_reconciled(&path, |base_text| {
        let mut doc = if base_text.trim().is_empty() {
            toml_edit::DocumentMut::default()
        } else {
            base_text
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| AppError::McpValidation(format!("解析 config.toml 失败: {e}")))?
        };
        reconcile_codex_document(&mut doc, owned_ids, enabled)?;
        Ok(doc.to_string())
    })
}

/// 将单个 MCP 服务器同步到 Codex live 配置
/// 始终使用 Codex 官方格式 [mcp_servers]，并清理可能存在的错误格式 [mcp.servers]
/// 把单个 MCP server 表写入 `[mcp_servers]`，并保证该键是「表」。
///
/// `~/.codex/config.toml` 是用户可手改的：若 `mcp_servers` 存在但不是表
/// （如 `mcp_servers = "x"` / `[]`），仅判 `contains_key` 会跳过重建，随后的
/// `doc["mcp_servers"][id] = …` 会触发 toml_edit 的 `IndexMut` panic
/// （panic 发生在 Tauri command 内、跨 FFI 展开）。这里统一归一化后再插入。
#[cfg(test)]
fn upsert_mcp_server_table(
    doc: &mut toml_edit::DocumentMut,
    id: &str,
    table: toml_edit::Table,
) -> Result<(), AppError> {
    if doc
        .get_mut("mcp_servers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .is_none()
    {
        // 键存在但不是表时，归一化会丢掉用户手写的那个值——必须留痕，
        // 否则用户只会看到自己的改动凭空消失。
        if doc.get("mcp_servers").is_some_and(|item| !item.is_none()) {
            log::warn!("config.toml 的 mcp_servers 不是表，已重置为空表");
        }
        doc["mcp_servers"] = toml_edit::table();
    }
    let servers = doc
        .get_mut("mcp_servers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| AppError::McpValidation("config.toml 的 mcp_servers 不是表".to_string()))?;
    servers.insert(id, toml_edit::Item::Table(table));
    Ok(())
}

/// 从 `[mcp_servers]`（以及历史错误格式 `[mcp.servers]`）中删除单个 MCP server。
///
/// 与 `upsert_mcp_server_table` 对称地使用 `as_table_like_mut`：用户若把配置写成
/// inline table（`mcp_servers = { foo = {...} }`，TOML 合法），`as_table_mut` 会返回
/// None 导致删除**静默失效**——界面提示已移除，条目却还在文件里，Codex 下次启动照样
/// 加载。这比 panic 更隐蔽，因为用户往往正是发现某个 MCP 有问题才来关它的。
///
/// 与写入分离成纯 doc 级函数，使守卫可脱离真实 `~/.codex/config.toml` 单测。
fn remove_mcp_server_from_doc(doc: &mut toml_edit::DocumentMut, id: &str) {
    if let Some(item) = doc.get_mut("mcp_servers") {
        // `Item::None` 是 toml_edit 的占位形态，不是用户写下的值——对它告警是噪音。
        // 必须在取可变借用之前算出来。
        let user_authored = !item.is_none();
        match item.as_table_like_mut() {
            Some(mcp_servers) => {
                mcp_servers.remove(id);
            }
            None if user_authored => {
                log::warn!("config.toml 的 mcp_servers 不是表，无法删除服务器 '{id}'");
            }
            None => {}
        }
    }

    // 同时清理可能存在于错误位置的数据：[mcp.servers]（如果存在）
    if let Some(mcp_table) = doc.get_mut("mcp").and_then(|t| t.as_table_like_mut()) {
        if let Some(servers) = mcp_table
            .get_mut("servers")
            .and_then(|s| s.as_table_like_mut())
        {
            if servers.remove(id).is_some() {
                log::warn!("从错误的 MCP 格式 [mcp.servers] 中清理了服务器 '{id}'");
            }
        }
    }
}

pub fn sync_single_server_to_codex(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }

    let config_path = crate::codex_config::get_codex_config_path();
    let owned_ids = HashSet::from([id.to_string()]);
    let enabled = HashMap::from([(id.to_string(), server_spec.clone())]);
    write_codex_mcp_reconciled(&config_path, |base_text| {
        let mut doc = if base_text.trim().is_empty() {
            toml_edit::DocumentMut::default()
        } else {
            base_text
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| AppError::McpValidation(format!("解析 config.toml 失败: {e}")))?
        };
        reconcile_codex_document(&mut doc, &owned_ids, &enabled)?;
        Ok(doc.to_string())
    })
}

/// 从 Codex live 配置中移除单个 MCP 服务器
/// 从正确的 [mcp_servers] 表中删除，同时清理可能存在于错误位置 [mcp.servers] 的数据
pub fn remove_server_from_codex(id: &str) -> Result<(), AppError> {
    if !should_sync_codex_mcp() {
        return Ok(());
    }
    let config_path = crate::codex_config::get_codex_config_path();

    if !config_path.exists() {
        return Ok(()); // 文件不存在，无需删除
    }

    let no_owned_ids = HashSet::new();
    let no_enabled = HashMap::new();
    write_codex_mcp_reconciled(&config_path, |base_text| {
        let mut doc = if base_text.trim().is_empty() {
            toml_edit::DocumentMut::default()
        } else {
            base_text
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| AppError::McpValidation(format!("解析 config.toml 失败: {e}")))?
        };
        reconcile_codex_document(&mut doc, &no_owned_ids, &no_enabled)?;
        remove_mcp_server_from_doc(&mut doc, id);
        Ok(doc.to_string())
    })
}

// ============================================================================
// TOML 转换辅助函数
// ============================================================================

/// 通用 JSON 值到 TOML 值转换器（支持简单类型和浅层嵌套）
///
/// 支持的类型转换：
/// - String → TOML String
/// - Number (i64) → TOML Integer
/// - Number (f64) → TOML Float
/// - Boolean → TOML Boolean
/// - Array[简单类型] → TOML Array
/// - Object → TOML Inline Table (仅字符串值)
///
/// 不支持的类型（返回 None）：
/// - null
/// - 深度嵌套对象
/// - 混合类型数组
fn json_value_to_toml_item(value: &Value, field_name: &str) -> Option<toml_edit::Item> {
    use toml_edit::{Array, InlineTable, Item};

    match value {
        Value::String(s) => Some(toml_edit::value(s.as_str())),

        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(toml_edit::value(i))
            } else if let Some(f) = n.as_f64() {
                Some(toml_edit::value(f))
            } else {
                log::warn!("跳过字段 '{field_name}': 无法转换的数字类型 {n}");
                None
            }
        }

        Value::Bool(b) => Some(toml_edit::value(*b)),

        Value::Array(arr) => {
            // 只支持简单类型的数组（字符串、数字、布尔）
            let mut toml_arr = Array::default();
            let mut all_same_type = true;

            for item in arr {
                match item {
                    Value::String(s) => toml_arr.push(s.as_str()),
                    Value::Number(n) if n.is_i64() => {
                        if let Some(i) = n.as_i64() {
                            toml_arr.push(i);
                        } else {
                            all_same_type = false;
                            break;
                        }
                    }
                    Value::Number(n) if n.is_f64() => {
                        if let Some(f) = n.as_f64() {
                            toml_arr.push(f);
                        } else {
                            all_same_type = false;
                            break;
                        }
                    }
                    Value::Bool(b) => toml_arr.push(*b),
                    _ => {
                        all_same_type = false;
                        break;
                    }
                }
            }

            if all_same_type && !toml_arr.is_empty() {
                Some(Item::Value(toml_edit::Value::Array(toml_arr)))
            } else {
                log::warn!("跳过字段 '{field_name}': 不支持的数组类型（混合类型或嵌套结构）");
                None
            }
        }

        Value::Object(obj) => {
            // 只支持浅层对象（所有值都是字符串）→ TOML Inline Table
            let mut inline_table = InlineTable::new();
            let mut all_strings = true;

            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    // InlineTable 需要 Value 类型，toml_edit::value() 返回 Item，需要提取内部的 Value
                    inline_table.insert(k, s.into());
                } else {
                    all_strings = false;
                    break;
                }
            }

            if all_strings && !inline_table.is_empty() {
                Some(Item::Value(toml_edit::Value::InlineTable(inline_table)))
            } else {
                log::warn!("跳过字段 '{field_name}': 对象值包含非字符串类型，建议使用子表语法");
                None
            }
        }

        Value::Null => {
            log::debug!("跳过字段 '{field_name}': TOML 不支持 null 值");
            None
        }
    }
}

/// Helper: 将 JSON MCP 服务器规范转换为 toml_edit::Table
///
/// 策略：
/// 1. 核心字段（type, command, args, url, headers, env, cwd）使用强类型处理
/// 2. 扩展字段（timeout、retry 等）通过白名单列表自动转换
/// 3. 其他未知字段使用通用转换器尝试转换
pub(super) fn json_server_to_toml_table(spec: &Value) -> Result<toml_edit::Table, AppError> {
    use toml_edit::{Array, Item, Table};

    let mut t = Table::new();
    let typ = spec.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");
    t["type"] = toml_edit::value(typ);

    // 定义核心字段（已在下方处理，跳过通用转换）
    let core_fields = match typ {
        "stdio" => vec!["type", "command", "args", "env", "cwd"],
        "http" | "sse" => vec!["type", "url", "headers", "http_headers"],
        _ => vec!["type"],
    };

    // 定义扩展字段白名单（Codex 常见可选字段）
    let extended_fields = [
        // 通用字段
        "timeout",
        "timeout_ms",
        "startup_timeout_ms",
        "startup_timeout_sec",
        "connection_timeout",
        "read_timeout",
        "debug",
        "log_level",
        "disabled",
        // stdio 特有
        "shell",
        "encoding",
        "working_dir",
        "restart_on_exit",
        "max_restart_count",
        // http/sse 特有
        "retry_count",
        "max_retry_attempts",
        "retry_delay",
        "cache_tools_list",
        "verify_ssl",
        "insecure",
        "proxy",
    ];

    // 1. 处理核心字段（强类型）
    match typ {
        "stdio" => {
            let cmd = spec.get("command").and_then(|v| v.as_str()).unwrap_or("");
            t["command"] = toml_edit::value(cmd);

            if let Some(args) = spec.get("args").and_then(|v| v.as_array()) {
                let mut arr_v = Array::default();
                for a in args.iter().filter_map(|x| x.as_str()) {
                    arr_v.push(a);
                }
                if !arr_v.is_empty() {
                    t["args"] = Item::Value(toml_edit::Value::Array(arr_v));
                }
            }

            if let Some(cwd) = spec.get("cwd").and_then(|v| v.as_str()) {
                if !cwd.trim().is_empty() {
                    t["cwd"] = toml_edit::value(cwd);
                }
            }

            if let Some(env) = spec.get("env").and_then(|v| v.as_object()) {
                let mut env_tbl = Table::new();
                for (k, v) in env.iter() {
                    if let Some(s) = v.as_str() {
                        env_tbl[&k[..]] = toml_edit::value(s);
                    }
                }
                if !env_tbl.is_empty() {
                    t["env"] = Item::Table(env_tbl);
                }
            }
        }
        "http" | "sse" => {
            let url = spec.get("url").and_then(|v| v.as_str()).unwrap_or("");
            t["url"] = toml_edit::value(url);

            if let Some(headers) = spec.get("headers").and_then(|v| v.as_object()) {
                let mut h_tbl = Table::new();
                for (k, v) in headers.iter() {
                    if let Some(s) = v.as_str() {
                        h_tbl[&k[..]] = toml_edit::value(s);
                    }
                }
                if !h_tbl.is_empty() {
                    t["http_headers"] = Item::Table(h_tbl);
                }
            }
        }
        _ => {}
    }

    // 2. 处理扩展字段和其他未知字段
    if let Some(obj) = spec.as_object() {
        for (key, value) in obj {
            // 跳过已处理的核心字段
            if core_fields.contains(&key.as_str()) {
                continue;
            }

            // 尝试使用通用转换器
            if let Some(toml_item) = json_value_to_toml_item(value, key) {
                t[&key[..]] = toml_item;

                // 只记录字段名：未知字段同样可能携带 token / secret。
                if extended_fields.contains(&key.as_str()) {
                    log::debug!("已转换扩展字段 '{key}'（值已省略）");
                } else {
                    log::debug!("已转换自定义字段 '{key}'（值已省略）");
                }
            }
        }
    }

    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        _dir: TempDir,
        original_test_home: Option<String>,
    }

    impl TempHome {
        /// 创建隔离的 Codex home，避免测试读写真实用户配置。
        fn new() -> Self {
            let dir = TempDir::new().expect("create temp home");
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                _dir: dir,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        /// 恢复测试前的 home 环境变量，避免污染其它测试。
        fn drop(&mut self) {
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            crate::settings::reload_settings().expect("reload settings");
        }
    }

    #[test]
    fn upsert_normalizes_non_table_mcp_servers_without_panicking() {
        // 用户手改过的 config.toml：mcp_servers 是字符串而不是表。
        // 修复前 `doc["mcp_servers"][id] = …` 会 panic。
        for malformed in [
            "mcp_servers = \"x\"\n",
            "mcp_servers = []\n",
            "mcp_servers = 42\n",
        ] {
            let mut doc = malformed
                .parse::<toml_edit::DocumentMut>()
                .expect("fixture parses");
            let table = json_server_to_toml_table(&json!({
                "type": "stdio",
                "command": "npx"
            }))
            .expect("server table");

            upsert_mcp_server_table(&mut doc, "echo", table)
                .unwrap_or_else(|e| panic!("upsert must not fail for {malformed:?}: {e}"));

            let servers = doc
                .get("mcp_servers")
                .and_then(|item| item.as_table_like())
                .unwrap_or_else(|| panic!("mcp_servers must be normalized to a table"));
            assert!(servers.contains_key("echo"));
        }
    }

    #[test]
    #[serial]
    fn sync_single_server_to_codex_does_not_overwrite_invalid_config() {
        let _home = TempHome::new();
        let codex_dir = crate::codex_config::get_codex_config_dir();
        std::fs::create_dir_all(&codex_dir).expect("create codex dir");
        let config_path = crate::codex_config::get_codex_config_path();
        let original = r#"approval_policy = "on-request"
sandbox_mode = "workspace-write"
broken = [
"#;
        std::fs::write(&config_path, original).expect("seed invalid codex config");

        let err = sync_single_server_to_codex(
            &MultiAppConfig::default(),
            "echo",
            &json!({
                "type": "stdio",
                "command": "node",
                "args": ["server.js"]
            }),
        )
        .expect_err("invalid config.toml must abort single MCP sync");

        assert!(
            err.to_string().contains("config.toml"),
            "error should name config.toml, got: {err}"
        );
        let after = std::fs::read_to_string(&config_path).expect("read config after failed sync");
        assert_eq!(
            after, original,
            "single MCP sync must not replace invalid config.toml with a partial document"
        );
    }

    #[test]
    #[serial]
    fn optimistic_writer_retries_after_external_edit() {
        let _home = TempHome::new();
        let codex_dir = crate::codex_config::get_codex_config_dir();
        std::fs::create_dir_all(&codex_dir).expect("create codex dir");
        let config_path = crate::codex_config::get_codex_config_path();
        std::fs::write(&config_path, "[desktop]\nkeep = true\n").expect("seed config.toml");

        set_test_reconcile_mutations(vec![b"[external]\nchanged = true\n".to_vec()]);
        sync_single_server_to_codex(
            &MultiAppConfig::default(),
            "echo",
            &json!({"type": "stdio", "command": "node"}),
        )
        .expect("writer should retry after external edit");

        let text = std::fs::read_to_string(&config_path).expect("read reconciled config");
        assert!(text.contains("[external]"));
        assert!(text.contains("[mcp_servers.echo]"));
    }

    #[test]
    #[serial]
    fn optimistic_writer_stops_after_bounded_conflicts() {
        let _home = TempHome::new();
        let codex_dir = crate::codex_config::get_codex_config_dir();
        std::fs::create_dir_all(&codex_dir).expect("create codex dir");
        let config_path = crate::codex_config::get_codex_config_path();
        std::fs::write(&config_path, "[desktop]\nkeep = true\n").expect("seed config.toml");

        set_test_reconcile_mutations(vec![
            b"[external]\nversion = 1\n".to_vec(),
            b"[external]\nversion = 2\n".to_vec(),
            b"[external]\nversion = 3\n".to_vec(),
        ]);
        let err = sync_single_server_to_codex(
            &MultiAppConfig::default(),
            "echo",
            &json!({"type": "stdio", "command": "node"}),
        )
        .expect_err("repeated external edits must fail closed");
        assert!(err.to_string().contains("有限重试"));

        let text = std::fs::read_to_string(&config_path).expect("read untouched external edit");
        assert!(text.contains("version = 1"));
        assert!(!text.contains("mcp_servers.echo"));
    }

    #[test]
    fn upsert_preserves_existing_servers_in_a_valid_table() {
        let mut doc = "[mcp_servers.keep]\ncommand = \"keep\"\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("fixture parses");
        let table = json_server_to_toml_table(&json!({
            "type": "stdio",
            "command": "npx"
        }))
        .expect("server table");

        upsert_mcp_server_table(&mut doc, "added", table).expect("upsert");

        let servers = doc
            .get("mcp_servers")
            .and_then(|item| item.as_table_like())
            .expect("table");
        assert!(servers.contains_key("keep"), "existing server must survive");
        assert!(servers.contains_key("added"));
    }

    #[test]
    fn remove_deletes_from_inline_table_form_too() {
        // inline table 是合法 TOML，但 as_table_mut() 对它返回 None——用它做守卫
        // 会让删除静默失效：界面说移除成功，条目却还在，Codex 下次启动照样加载。
        let mut doc = "mcp_servers = { drop = { command = \"x\" }, keep = { command = \"y\" } }\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("fixture parses");

        remove_mcp_server_from_doc(&mut doc, "drop");

        let servers = doc
            .get("mcp_servers")
            .and_then(|item| item.as_table_like())
            .expect("mcp_servers must still be table-like");
        assert!(
            !servers.contains_key("drop"),
            "removal must work on the inline-table form"
        );
        assert!(servers.contains_key("keep"), "siblings must survive");
    }

    #[test]
    fn remove_is_a_noop_on_non_table_mcp_servers() {
        // 既不能 panic，也不能把用户手写的值悄悄抹掉
        let mut doc = "mcp_servers = 42\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("fixture parses");

        remove_mcp_server_from_doc(&mut doc, "whatever");

        assert_eq!(doc.to_string(), "mcp_servers = 42\n");
    }

    #[test]
    fn http_headers_are_only_written_to_codex_http_headers() {
        let table = json_server_to_toml_table(&json!({
            "type": "http",
            "url": "https://mcp.example.com",
            "headers": {
                "Authorization": "Bearer top-secret",
                "X-Api-Key": "also-secret"
            },
            "timeout": 30
        }))
        .unwrap();

        let headers = table
            .get("http_headers")
            .and_then(|item| item.as_table())
            .expect("Codex http_headers table should be written");
        assert_eq!(
            headers.get("Authorization").and_then(|item| item.as_str()),
            Some("Bearer top-secret")
        );
        assert!(
            table.get("headers").is_none(),
            "legacy headers must not be emitted a second time"
        );
        assert_eq!(
            table.get("timeout").and_then(|item| item.as_integer()),
            Some(30)
        );
    }
}
