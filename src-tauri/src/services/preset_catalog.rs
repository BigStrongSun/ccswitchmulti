//! 本地预设表（preset-table.json）运行时索引。
//!
//! 打包产物由 `preset-table/tools/build_bundle.py` 生成，随 WebDAV 同步分发到
//! `~/.cc-switch/preset-table.json`。本模块负责加载与解析：
//!
//! - 基线条目（`baseline`）：API 通道事实，来自 models.dev 自动同步。
//! - plan 条目（`plans`）：审核后的订阅/调用策略薄覆盖，只存与基线的差异，用 `base_model`
//!   指向基线继承其余字段。
//! - deployment 条目（`deployments`）：共享的 Provider 部署薄覆盖；端点、凭据、探测和
//!   配额观测不允许进入该层，保持设备私有。
//!
//! 解析优先级：plan 覆盖 > 基线 > `None`（调用方回退到既有硬编码逻辑）。
//! 合并规则：对象递归深合并（plan 字段胜出），标量与列表整体替换。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// 应用配置目录下的预设表文件名（WebDAV artifact 同名）。
pub const PRESET_TABLE_FILE_NAME: &str = "preset-table.json";
/// 当前生成 bundle 的 schema 版本。
pub const PRESET_TABLE_SCHEMA_VERSION: u32 = 2;
/// 升级过程中仍接受的旧 bundle 版本，避免旧 WebDAV 快照导致目录整体失效。
pub const PRESET_TABLE_LEGACY_SCHEMA_VERSION: u32 = 1;
/// 预设表下载/加载上限：远大于当前 ~150KB 的体量，防止异常大文件。
pub const MAX_PRESET_TABLE_BYTES: u64 = 16 * 1024 * 1024;

/// 预设表 bundle（build_bundle.py 的输出）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetTableBundle {
    pub schema_version: u32,
    pub version: String,
    pub generated_at: String,
    /// 编译时固化的外部来源锁与各共享层内容 hash。v1 bundle 没有该字段。
    #[serde(default)]
    pub manifest: serde_json::Value,
    /// 键为 provider 名（如 `openai`），值为官方 API 基址，用于按 endpoint 前缀匹配。
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderMeta>,
    /// 键为 `provider/model`（如 `openai/gpt-5.5`），值为自包含的完整模型条目。
    pub baseline: BTreeMap<String, serde_json::Value>,
    /// 键为 plan 名（如 `openai-codex-plan`），值为 `model -> 薄覆盖条目`。
    #[serde(default)]
    pub plans: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    /// 键为共享部署 id（如 `shared/openai-enterprise`），值为薄覆盖条目。
    #[serde(default)]
    pub deployments: BTreeMap<String, serde_json::Value>,
}

/// provider 的官方 API 基址（endpoint 前缀匹配用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMeta {
    pub api: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PresetCatalogSource {
    /// 命中订阅 plan 覆盖（基线 + 覆盖深合并后的结果）。
    Plan,
    /// 命中 API 基线。
    Baseline,
}

/// 解析后的模型能力条目。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPresetEntry {
    pub model: String,
    pub plan: Option<String>,
    /// 命中的共享部署覆盖；设备私有观察值不在 bundle 中。
    pub deployment: Option<String>,
    pub source: PresetCatalogSource,
    /// 合并后的总上下文窗口（`limit.context`）。
    pub context_window: Option<u64>,
    /// 有效上下文窗口：`context * effective_context_percent / 100`；
    /// 无百分比时等于 `context_window`。
    pub effective_context_window: Option<u64>,
    /// 合并后的完整条目（含 provenance / cost / reasoning_options 等）。
    pub entry: serde_json::Value,
}

/// 预设表在应用配置目录下的路径。
pub fn bundle_path() -> std::path::PathBuf {
    crate::config::get_app_config_dir().join(PRESET_TABLE_FILE_NAME)
}

/// 从指定路径加载并校验 bundle；任何失败（缺失/超限/坏 JSON/版本不符）返回 `None`。
pub fn load_bundle(path: &Path) -> Option<PresetTableBundle> {
    let bytes = std::fs::read(path).ok()?;
    parse_bundle_bytes(&bytes)
}

/// Parse and validate a bundle payload before any caller persists it.
pub fn parse_bundle_bytes(bytes: &[u8]) -> Option<PresetTableBundle> {
    if bytes.len() as u64 > MAX_PRESET_TABLE_BYTES {
        log::warn!(
            "[PresetCatalog] bundle too large ({} bytes), ignoring",
            bytes.len()
        );
        return None;
    }
    let bundle: PresetTableBundle = serde_json::from_slice(&bytes).ok()?;
    if !matches!(
        bundle.schema_version,
        PRESET_TABLE_LEGACY_SCHEMA_VERSION | PRESET_TABLE_SCHEMA_VERSION
    ) {
        log::warn!(
            "[PresetCatalog] unsupported schema_version {} (supported: {} or {}), ignoring",
            bundle.schema_version,
            PRESET_TABLE_LEGACY_SCHEMA_VERSION,
            PRESET_TABLE_SCHEMA_VERSION,
        );
        return None;
    }
    Some(bundle)
}

/// Atomically persist an already validated bundle at the standard app path.
/// Registry downloads call this only after signature/hash and schema validation.
pub fn write_default_bundle_atomic(bytes: &[u8]) -> Result<(), AppError> {
    parse_bundle_bytes(bytes).ok_or_else(|| {
        AppError::localized(
            "preset.bundle.invalid",
            "预设表格式或 schema 无效，未写入本地文件",
            "Preset bundle format or schema is invalid; it was not written locally.",
        )
    })?;
    let dir = crate::config::get_app_config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    let target = dir.join(PRESET_TABLE_FILE_NAME);
    let tmp = dir.join(format!(".{PRESET_TABLE_FILE_NAME}.tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| AppError::io(&tmp, e))?;
    std::fs::rename(&tmp, &target).map_err(|e| AppError::io(&target, e))
}

/// 加载应用配置目录下的默认预设表。
pub fn load_default_bundle() -> Option<PresetTableBundle> {
    load_bundle(&bundle_path())
}

/// 深合并：对象递归合并（override 字段胜出），其余类型（标量/数组/null）整体替换。
pub fn deep_merge(
    base: &serde_json::Value,
    override_value: &serde_json::Value,
) -> serde_json::Value {
    match (base, override_value) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(override_map)) => {
            let mut merged = base_map.clone();
            for (key, value) in override_map {
                match merged.get(key) {
                    Some(existing) => {
                        merged.insert(key.clone(), deep_merge(existing, value));
                    }
                    None => {
                        merged.insert(key.clone(), value.clone());
                    }
                }
            }
            serde_json::Value::Object(merged)
        }
        (_, other) => other.clone(),
    }
}

fn entry_context_window(entry: &serde_json::Value) -> Option<u64> {
    entry
        .get("limit")?
        .as_object()?
        .get("context")?
        .as_u64()
        .filter(|context| *context > 0)
}

fn entry_effective_percent(entry: &serde_json::Value) -> Option<u64> {
    entry
        .get("limit")?
        .as_object()?
        .get("effective_context_percent")?
        .as_u64()
        .filter(|percent| (1..=100).contains(percent))
}

/// 解析模型条目：plan 覆盖 > 基线。
///
/// `plan` 为 `None` 或空时只查基线；plan 条目缺失时同样回退基线（不报错），
/// 让调用方可以无脑传 plan 名。
pub fn resolve(
    bundle: &PresetTableBundle,
    provider: &str,
    model: &str,
    plan: Option<&str>,
) -> Option<ResolvedPresetEntry> {
    resolve_with_deployment(bundle, provider, model, plan, None)
}

/// 解析模型条目：共享 deployment 覆盖 > plan/policy 覆盖 > baseline。
///
/// 共享 deployment 必须显式以 `base_model` 绑定当前模型，避免相同短模型名被错误
/// 套用到另一个供应商。该函数只处理已经由编译器审查过的共享数据；设备私有端点和
/// 运行观测不属于 bundle，不能经此路径进入 WebDAV 同步。
pub fn resolve_with_deployment(
    bundle: &PresetTableBundle,
    provider: &str,
    model: &str,
    plan: Option<&str>,
    deployment: Option<&str>,
) -> Option<ResolvedPresetEntry> {
    let base_key = format!("{provider}/{model}");
    let (source, mut entry) = match plan {
        Some(plan_name) if !plan_name.is_empty() => {
            let plan_entry = bundle.plans.get(plan_name)?.get(model)?;
            // plan 条目用 base_model 指向基线；缺省时按请求的 provider/model 找。
            let base_key = plan_entry
                .get("base_model")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| base_key.clone());
            let base = bundle.baseline.get(&base_key)?;
            (PresetCatalogSource::Plan, deep_merge(base, plan_entry))
        }
        _ => {
            let base = bundle.baseline.get(&base_key)?;
            (PresetCatalogSource::Baseline, base.clone())
        }
    };

    if let Some(deployment_id) = deployment.filter(|value| !value.is_empty()) {
        let deployment_entry = bundle.deployments.get(deployment_id)?;
        let deployment_base = deployment_entry
            .get("base_model")
            .and_then(|value| value.as_str())?;
        if deployment_base != base_key {
            return None;
        }
        entry = deep_merge(&entry, deployment_entry);
    }

    let context_window = entry_context_window(&entry);
    let effective_context_window = match (context_window, entry_effective_percent(&entry)) {
        (Some(context), Some(percent)) => Some(context * percent / 100),
        (context, _) => context,
    };

    Some(ResolvedPresetEntry {
        model: model.to_string(),
        plan: plan.filter(|value| !value.is_empty()).map(str::to_string),
        deployment: deployment
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        source,
        context_window,
        effective_context_window,
        entry,
    })
}

/// 仅基线查询（`/models` 补齐用，API 通道不涉及 plan）。
pub fn lookup_baseline<'a>(
    bundle: &'a PresetTableBundle,
    provider: &str,
    model: &str,
) -> Option<&'a serde_json::Value> {
    bundle.baseline.get(&format!("{provider}/{model}"))
}

/// 仅基线查询上下文窗口（`/models` 补齐用，API 通道不涉及 plan）。
pub fn lookup_baseline_context(
    bundle: &PresetTableBundle,
    provider: &str,
    model: &str,
) -> Option<u64> {
    let entry = lookup_baseline(bundle, provider, model)?;
    entry_context_window(entry)
}

/// 归一化 URL 前缀：大小写、尾斜杠和最终 `/models` 不影响 provider 匹配。
/// 与 `model_fetch::normalize_url_prefix` 语义一致（本地目录匹配复用同一规则）。
fn normalize_url_prefix(value: &str) -> String {
    let mut normalized = value.trim().trim_end_matches('/').to_ascii_lowercase();
    if let Some(stripped) = normalized.strip_suffix("/models") {
        normalized = stripped.to_string();
    }
    normalized
}

/// 按 endpoint URL 前缀找到匹配的 provider 名；无匹配返回 `None`。
///
/// 这是跨供应商防误配的关键：只有 endpoint 落在官方 API 基址前缀内才使用
/// 本地目录，第三方聚合站（OpenRouter 等）不走本地目录。
pub fn find_provider_for_endpoint<'a>(
    bundle: &'a PresetTableBundle,
    endpoint_url: &str,
) -> Option<&'a str> {
    let endpoint = normalize_url_prefix(endpoint_url);
    if endpoint.is_empty() {
        return None;
    }
    bundle
        .providers
        .iter()
        .find(|(_, meta)| {
            let api = normalize_url_prefix(&meta.api);
            !api.is_empty() && endpoint.starts_with(&api)
        })
        .map(|(name, _)| name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bundle() -> PresetTableBundle {
        let baseline = serde_json::json!({
            "openai/gpt-5.5": {
                "name": "GPT-5.5",
                "reasoning": true,
                "limit": { "context": 1050000, "input": 922000, "output": 128000 },
                "cost": { "input": 5.0, "output": 30.0, "cache_read": 0.5 },
                "reasoning_options": [{ "type": "effort", "values": ["none", "low", "high"] }],
            },
            "anthropic/claude-sonnet-4-5": {
                "name": "Claude Sonnet 4.5",
                "limit": { "context": 200000, "output": 64000 },
            },
        });
        let plans = serde_json::json!({
            "openai-codex-plan": {
                "gpt-5.5": {
                    "base_model": "openai/gpt-5.5",
                    "plan": "openai-codex-plan",
                    "plan_type": "subscription",
                    "limit": { "context": 272000, "effective_context_percent": 95 },
                    "cost": { "input": 0.0, "output": 0.0, "cache_read": 0.0 },
                }
            }
        });
        PresetTableBundle {
            schema_version: PRESET_TABLE_SCHEMA_VERSION,
            version: "2026.08.23".to_string(),
            generated_at: "2026-08-23T00:00:00Z".to_string(),
            manifest: serde_json::Value::Null,
            providers: BTreeMap::new(),
            baseline: baseline
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            plans: plans
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        v.as_object()
                            .unwrap()
                            .iter()
                            .map(|(mk, mv)| (mk.clone(), mv.clone()))
                            .collect(),
                    )
                })
                .collect(),
            deployments: BTreeMap::new(),
        }
    }

    #[test]
    fn resolve_baseline_only() {
        let bundle = test_bundle();
        let resolved = resolve(&bundle, "openai", "gpt-5.5", None).unwrap();
        assert_eq!(resolved.source, PresetCatalogSource::Baseline);
        assert_eq!(resolved.context_window, Some(1_050_000));
        assert_eq!(resolved.effective_context_window, Some(1_050_000));
        assert!(resolved.plan.is_none());
    }

    #[test]
    fn resolve_plan_override_wins_and_inherits() {
        let bundle = test_bundle();
        let resolved = resolve(&bundle, "openai", "gpt-5.5", Some("openai-codex-plan")).unwrap();
        assert_eq!(resolved.source, PresetCatalogSource::Plan);
        assert_eq!(resolved.context_window, Some(272_000));
        // 272000 * 95 / 100 = 258400
        assert_eq!(resolved.effective_context_window, Some(258_400));
        // 继承基线字段
        assert_eq!(resolved.entry["name"], "GPT-5.5");
        assert_eq!(resolved.entry["reasoning"], true);
        assert_eq!(resolved.entry["limit"]["input"], 922_000);
        // plan 覆盖字段
        assert_eq!(resolved.entry["cost"]["input"], 0.0);
        assert_eq!(resolved.entry["limit"]["context"], 272_000);
        assert_eq!(resolved.entry["plan_type"], "subscription");
    }

    #[test]
    fn deployment_override_wins_after_policy_and_keeps_baseline_facts() {
        let mut bundle = test_bundle();
        bundle.deployments.insert(
            "shared/openai-enterprise".to_string(),
            serde_json::json!({
                "base_model": "openai/gpt-5.5",
                "model_alias": "gpt-5.5-enterprise",
                "transport": { "api_format": "openai_responses" },
                "capabilities": { "web_search": true }
            }),
        );

        let resolved = resolve_with_deployment(
            &bundle,
            "openai",
            "gpt-5.5",
            Some("openai-codex-plan"),
            Some("shared/openai-enterprise"),
        )
        .unwrap();

        assert_eq!(resolved.source, PresetCatalogSource::Plan);
        assert_eq!(
            resolved.deployment.as_deref(),
            Some("shared/openai-enterprise")
        );
        assert_eq!(resolved.entry["limit"]["context"], 272_000);
        assert_eq!(
            resolved.entry["transport"]["api_format"],
            "openai_responses"
        );
        assert_eq!(resolved.entry["capabilities"]["web_search"], true);
        assert_eq!(resolved.entry["reasoning"], true);
    }

    #[test]
    fn resolve_missing_plan_falls_back_to_baseline() {
        let bundle = test_bundle();
        let resolved = resolve(&bundle, "openai", "gpt-5.5", Some("no-such-plan"));
        assert!(resolved.is_none());
        // 空 plan 名等价于不传
        let resolved = resolve(&bundle, "openai", "gpt-5.5", Some("")).unwrap();
        assert_eq!(resolved.source, PresetCatalogSource::Baseline);
    }

    #[test]
    fn resolve_unknown_model_returns_none() {
        let bundle = test_bundle();
        assert!(resolve(&bundle, "openai", "gpt-9.9", None).is_none());
        assert!(resolve(&bundle, "openai", "gpt-9.9", Some("openai-codex-plan")).is_none());
    }

    #[test]
    fn deep_merge_replaces_lists_and_scalars() {
        let base = serde_json::json!({ "a": 1, "list": [1, 2, 3], "obj": { "x": 1, "y": 2 } });
        let override_value = serde_json::json!({ "list": [9], "obj": { "y": 20, "z": 30 } });
        let merged = deep_merge(&base, &override_value);
        assert_eq!(merged["a"], 1);
        assert_eq!(merged["list"], serde_json::json!([9]));
        assert_eq!(
            merged["obj"],
            serde_json::json!({ "x": 1, "y": 20, "z": 30 })
        );
    }

    #[test]
    fn lookup_baseline_context_ignores_plans() {
        let bundle = test_bundle();
        assert_eq!(
            lookup_baseline_context(&bundle, "openai", "gpt-5.5"),
            Some(1_050_000)
        );
        assert_eq!(lookup_baseline_context(&bundle, "openai", "gpt-9.9"), None);
    }

    #[test]
    fn find_provider_for_endpoint_matches_prefix_only() {
        let mut bundle = test_bundle();
        bundle.providers = BTreeMap::from([
            (
                "openai".to_string(),
                ProviderMeta {
                    api: "https://api.openai.com/v1".to_string(),
                },
            ),
            (
                "anthropic".to_string(),
                ProviderMeta {
                    api: "https://api.anthropic.com/v1".to_string(),
                },
            ),
        ]);
        assert_eq!(
            find_provider_for_endpoint(&bundle, "https://api.openai.com/v1/models"),
            Some("openai")
        );
        assert_eq!(
            find_provider_for_endpoint(&bundle, "https://api.anthropic.com/v1"),
            Some("anthropic")
        );
        // 第三方聚合站不匹配任何官方前缀
        assert_eq!(
            find_provider_for_endpoint(&bundle, "https://openrouter.ai/api/v1/models"),
            None
        );
        // 前缀相似但不同域不匹配
        assert_eq!(
            find_provider_for_endpoint(&bundle, "https://evil-api.openai.com.evil.com/v1"),
            None
        );
    }

    #[test]
    fn load_bundle_rejects_bad_schema_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PRESET_TABLE_FILE_NAME);
        assert!(load_bundle(&path).is_none());

        let mut bundle = test_bundle();
        bundle.schema_version = 99;
        std::fs::write(&path, serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert!(load_bundle(&path).is_none());

        bundle.schema_version = PRESET_TABLE_SCHEMA_VERSION;
        std::fs::write(&path, serde_json::to_vec(&bundle).unwrap()).unwrap();
        let loaded = load_bundle(&path).unwrap();
        assert_eq!(loaded.baseline.len(), 2);
        assert_eq!(loaded.plans["openai-codex-plan"].len(), 1);
    }

    #[test]
    fn load_bundle_accepts_v1_for_sync_backward_compatibility() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PRESET_TABLE_FILE_NAME);
        let mut bundle = test_bundle();
        bundle.schema_version = 1;
        std::fs::write(&path, serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert_eq!(load_bundle(&path).unwrap().schema_version, 1);
    }
}
