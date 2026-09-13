//! Detect and repair Codex plugin marketplace registrations.
//!
//! Codex owns the plugin enablement table in `config.toml` and the marketplace
//! registry in `~/.agents`.  CCSwitchMulti only repairs a missing registry
//! entry when the cached plugin manifest and the enablement flag agree; it
//! never changes the user's `[plugins]` enablement state.

use crate::codex_config::get_codex_config_path;
use crate::config::{atomic_write, get_home_dir, path_is_within};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item};

const MARKETPLACE_RELATIVE_PATH: [&str; 3] = [".agents", "plugins", "marketplace.json"];
const PERSONAL_PLUGIN_NAMESPACE: &str = "personal";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexPluginRepairAction {
    RegisterMarketplace,
    EnableAndRegister,
    Enable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairableCodexPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub manifest_path: String,
    pub source_path: String,
    pub marketplace_path: String,
    pub repair_action: CodexPluginRepairAction,
}

#[derive(Debug, Clone)]
struct PluginManifest {
    name: String,
    version: String,
    manifest_path: PathBuf,
    source_path: PathBuf,
}

fn cache_root() -> PathBuf {
    get_home_dir().join(".codex").join("plugins").join("cache")
}

fn marketplace_path() -> PathBuf {
    get_home_dir()
        .join(MARKETPLACE_RELATIVE_PATH[0])
        .join(MARKETPLACE_RELATIVE_PATH[1])
        .join(MARKETPLACE_RELATIVE_PATH[2])
}

fn canonical_key(path: &Path) -> String {
    let mut key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.make_ascii_lowercase();
    }
    while key.len() > 1 && key.ends_with('/') {
        key.pop();
    }
    key
}

fn discover_manifest_paths(root: &Path, depth: usize, output: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            discover_manifest_paths(&path, depth + 1, output);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("plugin.json") {
            output.push(path);
        }
    }
}

fn manifest_source_path(manifest_path: &Path) -> Option<PathBuf> {
    let parent = manifest_path.parent()?;
    let directory_name = parent.file_name().and_then(|name| name.to_str());
    if matches!(directory_name, Some(".codex-plugin" | ".claude-plugin")) {
        parent.parent().map(Path::to_path_buf)
    } else {
        Some(parent.to_path_buf())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginConfigState {
    Missing,
    Enabled,
    Disabled,
}

#[derive(Debug, Clone)]
struct PluginConfigSnapshot {
    document: DocumentMut,
}

fn read_codex_config_snapshot() -> Result<PluginConfigSnapshot, String> {
    let path = get_codex_config_path();
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("读取 Codex config.toml 失败: {error}")),
    };
    let document = if bytes.is_empty() {
        DocumentMut::new()
    } else {
        String::from_utf8(bytes.clone())
            .map_err(|error| format!("解析 Codex config.toml 编码失败: {error}"))?
            .parse::<DocumentMut>()
            .map_err(|error| format!("解析 Codex config.toml 失败: {error}"))?
    };
    Ok(PluginConfigSnapshot { document })
}

fn plugin_config_key(name: &str) -> String {
    format!("{name}@{PERSONAL_PLUGIN_NAMESPACE}")
}

fn plugin_config_state(
    document: &DocumentMut,
    name: &str,
) -> Result<(String, PluginConfigState), String> {
    let Some(plugins) = document.get("plugins") else {
        return Ok((plugin_config_key(name), PluginConfigState::Missing));
    };
    let Some(plugins) = plugins.as_table_like() else {
        return Err("Codex config.toml 的 [plugins] 不是表结构".to_string());
    };

    let candidate_keys = [plugin_config_key(name), name.to_string()];
    for key in candidate_keys {
        let Some(entry) = plugins.get(&key) else {
            continue;
        };
        let Some(enabled) = entry
            .as_table_like()
            .and_then(|table| table.get("enabled"))
            .and_then(Item::as_bool)
        else {
            return Err(format!("Codex 插件配置 {key} 缺少布尔 enabled 字段"));
        };
        return Ok((
            key,
            if enabled {
                PluginConfigState::Enabled
            } else {
                PluginConfigState::Disabled
            },
        ));
    }
    Ok((plugin_config_key(name), PluginConfigState::Missing))
}

/// Return the enabled Codex plugins as `(config_id, manifest_name)` pairs.
///
/// The config accepts both the current `name@personal` key and the legacy
/// bare-name form.  The registry only needs the manifest name to search the
/// personal cache, while retaining the original key as the stable id used by
/// the repair command.  Missing or disabled entries are intentionally not
/// returned; this detector must never enable a plugin as a side effect.
fn enabled_codex_plugins() -> Result<Vec<(String, String)>, String> {
    let snapshot = read_codex_config_snapshot()?;
    let Some(plugins) = snapshot.document.get("plugins") else {
        return Ok(Vec::new());
    };
    let Some(plugins) = plugins.as_table_like() else {
        return Err("Codex config.toml 的 [plugins] 不是表结构".to_string());
    };

    let mut enabled = Vec::new();
    for (config_id, _entry) in plugins.iter() {
        let name = config_id
            .split_once('@')
            .map(|(name, _)| name)
            .unwrap_or(config_id)
            .trim();
        if name.is_empty() {
            continue;
        }
        let (resolved_id, state) = plugin_config_state(&snapshot.document, name)?;
        if state == PluginConfigState::Enabled {
            enabled.push((resolved_id, name.to_string()));
        }
    }
    Ok(enabled)
}

fn read_manifest(path: &Path) -> Option<PluginManifest> {
    let value: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let name = value.get("name")?.as_str()?.trim().to_string();
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("metadata")
                .and_then(|meta| meta.get("version"))
                .and_then(Value::as_str)
        })?
        .trim()
        .to_string();
    if name.is_empty() || version.is_empty() {
        return None;
    }
    let source_path = manifest_source_path(path)?;
    Some(PluginManifest {
        name,
        version,
        manifest_path: path.to_path_buf(),
        source_path,
    })
}

fn read_marketplace() -> Result<Value, String> {
    let path = marketplace_path();
    if !path.exists() {
        return Ok(json!({ "plugins": [] }));
    }
    serde_json::from_slice(
        &fs::read(&path).map_err(|error| format!("读取 marketplace.json 失败: {error}"))?,
    )
    .map_err(|error| format!("解析 marketplace.json 失败: {error}"))
}

fn plugins_array(value: &Value) -> Option<&Vec<Value>> {
    if let Some(array) = value.as_array() {
        return Some(array);
    }
    if let Some(array) = value.get("plugins").and_then(Value::as_array) {
        return Some(array);
    }
    value
        .get("marketplaces")
        .and_then(Value::as_array)
        .and_then(|marketplaces| {
            marketplaces
                .iter()
                .find_map(|marketplace| marketplace.get("plugins").and_then(Value::as_array))
        })
}

fn plugins_array_mut(value: &mut Value) -> Result<&mut Vec<Value>, String> {
    if value.is_array() {
        return value
            .as_array_mut()
            .ok_or_else(|| "marketplace 根节点不是数组".to_string());
    }
    let object = value
        .as_object_mut()
        .ok_or_else(|| "marketplace 根节点必须是对象或数组".to_string())?;
    if object.contains_key("plugins") {
        let plugins = object
            .get_mut("plugins")
            .expect("plugins key was checked above");
        return plugins
            .as_array_mut()
            .ok_or_else(|| "marketplace.plugins 必须是数组".to_string());
    }

    let nested_target = match object.get("marketplaces") {
        None => (None, false),
        Some(marketplaces) => {
            let marketplaces = marketplaces
                .as_array()
                .ok_or_else(|| "marketplace.marketplaces 必须是数组".to_string())?;
            if let Some(index) = marketplaces
                .iter()
                .position(|marketplace| marketplace.get("plugins").is_some())
            {
                (Some(index), false)
            } else if marketplaces.first().and_then(Value::as_object).is_some() {
                (None, true)
            } else {
                (None, false)
            }
        }
    };

    if let Some(index) = nested_target.0 {
        let marketplaces = object
            .get_mut("marketplaces")
            .and_then(Value::as_array_mut)
            .expect("marketplaces was validated as an array");
        let marketplace = marketplaces
            .get_mut(index)
            .expect("plugin marketplace index came from the same array");
        let plugins = marketplace
            .get_mut("plugins")
            .expect("plugin marketplace index has a plugins key");
        return plugins
            .as_array_mut()
            .ok_or_else(|| "marketplace.plugins 必须是数组".to_string());
    }

    if nested_target.1 {
        let marketplaces = object
            .get_mut("marketplaces")
            .and_then(Value::as_array_mut)
            .expect("marketplaces was validated as an array");
        let first = marketplaces
            .first_mut()
            .and_then(Value::as_object_mut)
            .expect("first marketplace was validated as an object");
        first.insert("plugins".to_string(), Value::Array(Vec::new()));
        return first
            .get_mut("plugins")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "marketplace.plugins 必须是数组".to_string());
    }
    let plugins = object
        .entry("plugins")
        .or_insert_with(|| Value::Array(Vec::new()));
    plugins
        .as_array_mut()
        .ok_or_else(|| "marketplace.plugins 必须是数组".to_string())
}

fn entry_name(entry: &Value) -> Option<&str> {
    entry
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| entry.get("id").and_then(Value::as_str))
}

fn entry_source_path(entry: &Value) -> Option<&str> {
    entry
        .get("source")
        .and_then(|source| source.get("path"))
        .and_then(Value::as_str)
        .or_else(|| entry.get("path").and_then(Value::as_str))
}

fn marketplace_source_matches(path: &str, manifest: &PluginManifest) -> bool {
    let raw_path = Path::new(path);
    let resolved_path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        get_home_dir().join(raw_path)
    };
    let candidates = [
        manifest.source_path.clone(),
        get_home_dir()
            .join(".codex")
            .join("plugins")
            .join(&manifest.name),
    ];
    candidates.iter().any(|candidate| {
        let candidate = fs::canonicalize(candidate).unwrap_or_else(|_| candidate.clone());
        let resolved = fs::canonicalize(&resolved_path).unwrap_or_else(|_| resolved_path.clone());
        canonical_key(&candidate) == canonical_key(&resolved)
    })
}

fn marketplace_plugin_state(value: &Value, manifest: &PluginManifest) -> Result<bool, String> {
    let expected_name = manifest.name.as_str();
    let Some(entries) = plugins_array(value) else {
        return Ok(false);
    };
    for entry in entries {
        let name_matches = entry_name(entry).is_some_and(|name| {
            name == expected_name || name.split('@').next().unwrap_or(name) == expected_name
        });
        if !name_matches {
            continue;
        }
        let Some(path) = entry_source_path(entry) else {
            return Err(format!(
                "插件 {expected_name} 的 marketplace 登记缺少 source.path"
            ));
        };
        if marketplace_source_matches(path, manifest) {
            return Ok(true);
        }
        return Err(format!(
            "插件 {expected_name} 的 marketplace source.path 不在可验证的插件目录内"
        ));
    }
    Ok(false)
}

fn marketplace_has_plugin(value: &Value, manifest: &PluginManifest) -> bool {
    marketplace_plugin_state(value, manifest).unwrap_or(false)
}

fn latest_manifest(manifests: impl Iterator<Item = PluginManifest>) -> Option<PluginManifest> {
    manifests.max_by(|left, right| {
        left.version
            .cmp(&right.version)
            .then_with(|| canonical_key(&left.source_path).cmp(&canonical_key(&right.source_path)))
    })
}

pub fn detect_codex_plugin_registration() -> Result<Vec<RepairableCodexPlugin>, String> {
    let enabled = enabled_codex_plugins()?;
    if enabled.is_empty() {
        return Ok(Vec::new());
    }
    let mut manifest_paths = Vec::new();
    discover_manifest_paths(&cache_root(), 0, &mut manifest_paths);
    let marketplace = read_marketplace()?;
    let marketplace_path = marketplace_path();
    let cache_root = fs::canonicalize(cache_root()).unwrap_or_else(|_| cache_root());
    let mut repairable = Vec::new();

    for (id, name) in enabled {
        let manifest = latest_manifest(
            manifest_paths
                .iter()
                .filter_map(|path| read_manifest(path))
                .filter(|manifest| manifest.name == name),
        );
        let Some(manifest) = manifest else {
            continue;
        };
        if marketplace_has_plugin(&marketplace, &manifest) {
            continue;
        }
        let source_path =
            fs::canonicalize(&manifest.source_path).unwrap_or(manifest.source_path.clone());
        if !path_is_within(&cache_root, &source_path) {
            continue;
        }
        repairable.push(RepairableCodexPlugin {
            id,
            name: manifest.name,
            version: manifest.version,
            manifest_path: manifest.manifest_path.to_string_lossy().to_string(),
            source_path: source_path.to_string_lossy().to_string(),
            marketplace_path: marketplace_path.to_string_lossy().to_string(),
            repair_action: CodexPluginRepairAction::RegisterMarketplace,
        });
    }

    Ok(repairable)
}

fn build_marketplace_entry(manifest: &PluginManifest, source_path: &Path) -> Value {
    json!({
        "name": manifest.name,
        "version": manifest.version,
        "source": {
            "path": source_path.to_string_lossy().to_string()
        }
    })
}

fn repair_manifest_registration(
    manifest: &PluginManifest,
    marketplace_path: &Path,
    cache_root: &Path,
) -> Result<(), String> {
    let manifest_path = fs::canonicalize(&manifest.manifest_path)
        .map_err(|error| format!("插件 manifest 不可访问: {error}"))?;
    let source_path = fs::canonicalize(&manifest.source_path)
        .map_err(|error| format!("插件目录不可访问: {error}"))?;
    let cache_root =
        fs::canonicalize(cache_root).map_err(|error| format!("插件缓存目录不可访问: {error}"))?;
    if !path_is_within(&cache_root, &source_path) || !path_is_within(&cache_root, &manifest_path) {
        return Err("插件 manifest 路径必须位于 Codex 插件缓存目录内".to_string());
    }

    let mut marketplace = read_marketplace()?;
    let entries = plugins_array_mut(&mut marketplace)?;
    let expected_name = manifest.name.as_str();
    let expected_path = canonical_key(&source_path);
    entries.retain(|entry| {
        let same_name = entry_name(entry).is_some_and(|name| {
            name == expected_name || name.split('@').next().unwrap_or(name) == expected_name
        });
        let same_path = entry_source_path(entry)
            .is_some_and(|path| canonical_key(Path::new(path)) == expected_path);
        !same_name && !same_path
    });
    entries.push(build_marketplace_entry(manifest, &source_path));

    let bytes = serde_json::to_vec_pretty(&marketplace)
        .map_err(|error| format!("序列化 marketplace.json 失败: {error}"))?;
    atomic_write(marketplace_path, &bytes)
        .map_err(|error| format!("写入 marketplace.json 失败: {error}"))
}

pub fn repair_codex_plugin_registration(plugin_id: &str) -> Result<RepairableCodexPlugin, String> {
    let candidates = detect_codex_plugin_registration()?;
    let candidate = if let Some(candidate) = candidates
        .into_iter()
        .find(|plugin| plugin.id == plugin_id || plugin.name == plugin_id)
    {
        candidate
    } else {
        // 修复命令保持幂等：第二次点击时登记已经存在，直接返回当前
        // manifest 摘要，不再追加重复条目。
        let enabled = enabled_codex_plugins()?;
        let expected_name = enabled
            .iter()
            .find(|(id, name)| id == plugin_id || name == plugin_id)
            .map(|(_, name)| name.clone())
            .ok_or_else(|| format!("未找到需要修复的 Codex 插件: {plugin_id}"))?;
        let mut manifest_paths = Vec::new();
        discover_manifest_paths(&cache_root(), 0, &mut manifest_paths);
        let manifest = latest_manifest(
            manifest_paths
                .iter()
                .filter_map(|path| read_manifest(path))
                .filter(|manifest| manifest.name == expected_name),
        )
        .ok_or_else(|| format!("未找到 Codex 插件 manifest: {expected_name}"))?;
        let source_path = fs::canonicalize(&manifest.source_path)
            .unwrap_or_else(|_| manifest.source_path.clone());
        if !marketplace_has_plugin(&read_marketplace()?, &manifest) {
            return Err(format!("未找到需要修复的 Codex 插件: {plugin_id}"));
        }
        return Ok(RepairableCodexPlugin {
            id: plugin_id.to_string(),
            name: manifest.name,
            version: manifest.version,
            manifest_path: manifest.manifest_path.to_string_lossy().to_string(),
            source_path: source_path.to_string_lossy().to_string(),
            marketplace_path: marketplace_path().to_string_lossy().to_string(),
            repair_action: CodexPluginRepairAction::RegisterMarketplace,
        });
    };
    let manifest_path = PathBuf::from(&candidate.manifest_path);
    let manifest = read_manifest(&manifest_path)
        .ok_or_else(|| "插件 manifest 缺少合法 name 或 version".to_string())?;
    let result = repair_manifest_registration(
        &manifest,
        &PathBuf::from(&candidate.marketplace_path),
        &cache_root(),
    );
    match result {
        Ok(()) => Ok(candidate),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    struct TempHome {
        _dir: TempDir,
        previous: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp home");
            let previous = std::env::var("CC_SWITCH_TEST_HOME").ok();
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn seed_sdd() {
        let manifest_dir = cache_root()
            .join("personal")
            .join("sdd")
            .join("0.1.0")
            .join(".codex-plugin");
        fs::create_dir_all(&manifest_dir).expect("manifest dir");
        fs::write(
            manifest_dir.join("plugin.json"),
            r#"{"name":"sdd","version":"0.1.0"}"#,
        )
        .expect("manifest");
        let config_path = get_codex_config_path();
        fs::create_dir_all(config_path.parent().expect("config parent")).expect("config dir");
        fs::write(config_path, "[plugins.\"sdd@personal\"]\nenabled = true\n").expect("config");
        fs::create_dir_all(marketplace_path().parent().expect("marketplace parent"))
            .expect("marketplace dir");
    }

    #[test]
    #[serial]
    fn detects_enabled_cached_plugin_missing_marketplace_entry() {
        let _home = TempHome::new();
        seed_sdd();
        let plugins = detect_codex_plugin_registration().expect("detect");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "sdd");
    }

    #[test]
    #[serial]
    fn repairs_registration_preserving_existing_entries_and_is_idempotent() {
        let _home = TempHome::new();
        seed_sdd();
        fs::write(
            marketplace_path(),
            r#"{"plugins":[{"name":"investment-signal-monitor","source":{"path":"C:/existing"}}]}"#,
        )
        .expect("seed marketplace");
        repair_codex_plugin_registration("sdd").expect("repair");
        repair_codex_plugin_registration("sdd").expect("second repair should be idempotent");
        let value: Value =
            serde_json::from_slice(&fs::read(marketplace_path()).expect("read marketplace"))
                .expect("parse marketplace");
        let entries = plugins_array(&value).expect("plugins array");
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|entry| entry_name(entry) == Some("investment-signal-monitor")));
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry_name(entry) == Some("sdd"))
                .count(),
            1
        );
    }

    #[test]
    #[serial]
    fn repairs_nested_marketplace_registration_without_creating_root_plugins() {
        let _home = TempHome::new();
        seed_sdd();
        fs::write(
            marketplace_path(),
            r#"{"marketplaces":[{"name":"personal","plugins":[]},{"name":"shared","plugins":[]}]}"#,
        )
        .expect("seed nested marketplace");

        repair_codex_plugin_registration("sdd").expect("repair nested marketplace");

        let value: Value =
            serde_json::from_slice(&fs::read(marketplace_path()).expect("read marketplace"))
                .expect("parse marketplace");
        assert!(value.get("plugins").is_none());
        let marketplaces = value
            .get("marketplaces")
            .and_then(Value::as_array)
            .expect("marketplaces array");
        let entries = marketplaces[0]
            .get("plugins")
            .and_then(Value::as_array)
            .expect("nested plugins array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entry_name(&entries[0]), Some("sdd"));
    }

    #[test]
    #[serial]
    fn rejects_manifest_outside_plugin_cache() {
        let _home = TempHome::new();
        let outside = tempfile::tempdir().expect("outside dir");
        let manifest_path = outside.path().join("plugin.json");
        fs::write(&manifest_path, r#"{"name":"sdd","version":"0.1.0"}"#).expect("manifest");
        let manifest = read_manifest(&manifest_path).expect("read manifest");
        let result = repair_manifest_registration(&manifest, &marketplace_path(), &cache_root());
        assert!(result.unwrap_err().contains("缓存目录"));
    }
}
