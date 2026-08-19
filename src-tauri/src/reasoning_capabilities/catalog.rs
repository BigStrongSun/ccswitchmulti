//! CCSM 维护的版本化能力库（独立 JSON 资源，禁止编译进 Rust，前端不得维护副本）。
//!
//! 匹配键：`平台 + API 格式 + canonical model + revision range`。每条记录来源 URL、
//! 核验日期、库版本和证据等级。第一阶段随应用打包；后续允许用户主动下载签名包。

use crate::proxy::providers::codex_reasoning::CodexModelReasoningCapability;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 能力库文件路径的环境变量覆盖（测试/开发用）。
pub const LIBRARY_PATH_ENV: &str = "CCSM_REASONING_LIBRARY";

/// 随应用打包的库文件名。
pub const PACKAGED_LIBRARY_FILE: &str = "reasoning-capabilities.json";

/// 版本化能力库。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityLibrary {
    pub library_version: u32,
    /// 库整体核验日期（ISO 8601 date）。
    pub verified_at: String,
    pub entries: Vec<LibraryEntry>,
}

/// 能力库条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    /// 平台标识：`openrouter` / `vllm` / `deepseek` / ...；`any` 表示平台无关。
    pub platform: String,
    /// API 格式：`responses` / `chat` / ...；缺省表示不限。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_format: Option<String>,
    /// canonical model id（匹配时做大小写归一化）。
    pub model: String,
    /// 模型/服务 revision 范围（如 `>=0.8.0`）；缺省表示不限。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_range: Option<String>,
    /// 推理能力声明（schema v2）。
    pub reasoning: CodexModelReasoningCapability,
    /// 证据来源 URL。
    pub source_url: String,
    /// 条目核验日期（ISO 8601 date）。
    pub verified_at: String,
    /// 证据等级：`platform_api` / `vendor_docs` / `community`。
    pub evidence_level: String,
}

impl CapabilityLibrary {
    /// 按 `平台 + API 格式 + canonical model + revision` 查找能力。
    ///
    /// 平台精确匹配优先于 `any`；同平台多条目时取 `api_format` 精确匹配优先。
    /// 命中条目的 `reasoning` 必须通过 schema v2 校验，否则视为未命中
    /// （库内容非法不得污染运行路径）。
    pub fn lookup(
        &self,
        platform: Option<&str>,
        api_format: Option<&str>,
        model: &str,
        revision: Option<&str>,
    ) -> Option<CodexModelReasoningCapability> {
        let normalized_model = model.trim().to_ascii_lowercase();
        let candidates: Vec<&LibraryEntry> = self
            .entries
            .iter()
            .filter(|entry| entry.model.trim().eq_ignore_ascii_case(&normalized_model))
            .filter(|entry| platform_matches(entry.platform.as_str(), platform))
            .filter(|entry| api_format_matches(entry.api_format.as_deref(), api_format))
            .filter(|entry| revision_matches(entry.revision_range.as_deref(), revision))
            .collect();

        // 平台精确匹配优先于 any；api_format 精确匹配优先于不限。
        candidates
            .iter()
            .max_by_key(|entry| {
                (
                    entry.platform != "any",
                    entry.api_format.as_deref().is_some_and(|f| {
                        api_format.is_some_and(|actual| f.eq_ignore_ascii_case(actual))
                    }),
                )
            })
            .and_then(|entry| {
                entry
                    .reasoning
                    .validate()
                    .ok()
                    .map(|_| entry.reasoning.clone())
            })
    }
}

fn platform_matches(entry_platform: &str, actual: Option<&str>) -> bool {
    if entry_platform.eq_ignore_ascii_case("any") {
        return true;
    }
    actual.is_some_and(|value| value.eq_ignore_ascii_case(entry_platform))
}

fn api_format_matches(entry_format: Option<&str>, actual: Option<&str>) -> bool {
    match entry_format {
        None => true,
        Some(format) => actual.is_some_and(|value| value.eq_ignore_ascii_case(format)),
    }
}

/// 极简 revision 范围匹配：支持 `>=x.y.z` / `<=x.y.z` / `x.y.z`（精确）/ 缺省（不限）。
///
/// 首版只覆盖能力库实际用到的形态；复杂范围语法留待库更新流程引入。
fn revision_matches(range: Option<&str>, actual: Option<&str>) -> bool {
    let Some(range) = range.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let Some(actual) = actual.map(str::trim).filter(|value| !value.is_empty()) else {
        // 有范围约束但拿不到实际 revision：保守视为不匹配，避免误用。
        return false;
    };
    let actual_tuple = version_tuple(actual);
    if let Some(rest) = range.strip_prefix(">=") {
        return actual_tuple >= version_tuple(rest.trim());
    }
    if let Some(rest) = range.strip_prefix("<=") {
        return actual_tuple <= version_tuple(rest.trim());
    }
    actual_tuple == version_tuple(range)
}

fn version_tuple(value: &str) -> (u64, u64, u64) {
    value
        .trim()
        .trim_start_matches('v')
        .split('.')
        .take(3)
        .map(|part| part.trim().parse::<u64>().unwrap_or(0))
        .enumerate()
        .fold((0, 0, 0), |acc, (index, part)| match index {
            0 => (part, acc.1, acc.2),
            1 => (acc.0, part, acc.2),
            _ => (acc.0, acc.1, part),
        })
}

/// 从 JSON 文本解析能力库。
pub fn load_library_from_str(json: &str) -> Result<CapabilityLibrary, String> {
    let library: CapabilityLibrary = serde_json::from_str(json)
        .map_err(|error| format!("invalid capability library: {error}"))?;
    if library.library_version == 0 {
        return Err("capability library requires a nonzero libraryVersion".to_string());
    }
    for entry in &library.entries {
        entry
            .reasoning
            .validate()
            .map_err(|error| format!("library entry {} is invalid: {error}", entry.model))?;
    }
    Ok(library)
}

/// 从文件路径加载能力库。
pub fn load_library_from_path(path: &Path) -> Result<CapabilityLibrary, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read capability library {path:?}: {error}"))?;
    load_library_from_str(&content)
}

/// 解析随应用打包的库路径：
/// 1. 环境变量覆盖（测试/开发）；
/// 2. Tauri 资源目录（setup hook 初始化）；
/// 3. 开发回退（仓库内相对路径）。
pub fn resolve_packaged_library_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(LIBRARY_PATH_ENV) {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Some(dir) = crate::reasoning_capabilities::resource_dir() {
        let candidate = dir.join(PACKAGED_LIBRARY_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for candidate in [
        PathBuf::from("src-tauri/resources/reasoning-capabilities.json"),
        PathBuf::from("resources/reasoning-capabilities.json"),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 全局懒加载能力库（请求路径共享；加载失败保持 None 并降级到内置/unknown）。
static LIBRARY: OnceLock<Mutex<Option<CapabilityLibrary>>> = OnceLock::new();

pub fn global_library() -> Option<CapabilityLibrary> {
    let guard = LIBRARY.get_or_init(|| Mutex::new(None));
    let mut lock = guard.lock().expect("capability library poisoned");
    if lock.is_none() {
        match resolve_packaged_library_path() {
            Some(path) => match load_library_from_path(&path) {
                Ok(library) => *lock = Some(library),
                Err(error) => log::warn!("reasoning capability library load failed: {error}"),
            },
            None => log::debug!(
                "reasoning capability library not found; falling back to builtin/unknown"
            ),
        }
    }
    lock.clone()
}

/// 测试用：重置全局库缓存。
#[cfg(test)]
pub fn reset_global_library_for_tests() {
    // OnceLock 无法重置；测试通过环境变量覆盖路径隔离。
}

/// 供 resolver 使用的查找入口（平台 + 模型；API 格式与 revision 首版不传入）。
pub fn lookup_library_capability(
    library: &CapabilityLibrary,
    platform: Option<&str>,
    model: &str,
) -> Option<CodexModelReasoningCapability> {
    library.lookup(platform, None, model, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(platform: &str, model: &str, efforts: &[&str]) -> LibraryEntry {
        LibraryEntry {
            platform: platform.to_string(),
            api_format: None,
            model: model.to_string(),
            revision_range: None,
            reasoning: CodexModelReasoningCapability {
                schema_version: Some(2),
                support_status: Some(
                    crate::proxy::providers::codex_reasoning::ReasoningSupportStatus::ConfirmedSupported,
                ),
                control_kind: Some(
                    crate::proxy::providers::codex_reasoning::ReasoningControlKind::Graded,
                ),
                supported: None,
                supported_efforts: efforts.iter().map(|value| value.to_string()).collect(),
                default_effort: efforts.first().map(|value| value.to_string()),
                disable_allowed: efforts.iter().any(|value| *value == "none"),
                upstream: crate::proxy::providers::codex_reasoning::CodexModelReasoningUpstream {
                    format: "object".into(),
                    parameter: "reasoning.effort".into(),
                    effort_map: Default::default(),
                },
                output_format: Some("auto".into()),
                source: Some("library".into()),
                confidence: None,
                fetched_at: None,
                provider_key: None,
                model_revision: None,
            },
            source_url: "https://example.com".into(),
            verified_at: "2026-08-18".into(),
            evidence_level: "platform_api".into(),
        }
    }

    #[test]
    fn lookup_matches_platform_and_model_case_insensitively() {
        let library = CapabilityLibrary {
            library_version: 1,
            verified_at: "2026-08-18".into(),
            entries: vec![entry(
                "openrouter",
                "DeepSeek/DeepSeek-V4-Pro",
                &["high", "low"],
            )],
        };
        let hit = library.lookup(Some("OpenRouter"), None, "deepseek/deepseek-v4-pro", None);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().supported_efforts, vec!["high", "low"]);
    }

    #[test]
    fn lookup_platform_specific_beats_any() {
        let mut any_entry = entry("any", "model-x", &["high"]);
        any_entry.reasoning.default_effort = Some("high".into());
        let mut specific = entry("vllm", "model-x", &["low"]);
        specific.reasoning.default_effort = Some("low".into());
        let library = CapabilityLibrary {
            library_version: 1,
            verified_at: "2026-08-18".into(),
            entries: vec![any_entry, specific],
        };
        let hit = library.lookup(Some("vllm"), None, "model-x", None).unwrap();
        assert_eq!(hit.supported_efforts, vec!["low"]);
    }

    #[test]
    fn lookup_revision_range_gates_match() {
        let mut entry = entry("vllm", "qwen3.6-27b", &["none"]);
        entry.reasoning.control_kind =
            Some(crate::proxy::providers::codex_reasoning::ReasoningControlKind::Boolean);
        entry.reasoning.supported_efforts = vec![];
        entry.reasoning.default_effort = None;
        entry.revision_range = Some(">=0.8.0".into());
        let library = CapabilityLibrary {
            library_version: 1,
            verified_at: "2026-08-18".into(),
            entries: vec![entry],
        };
        assert!(library
            .lookup(Some("vllm"), None, "qwen3.6-27b", Some("0.9.2"))
            .is_some());
        assert!(library
            .lookup(Some("vllm"), None, "qwen3.6-27b", Some("0.7.0"))
            .is_none());
        // 拿不到 revision 时保守不匹配。
        assert!(library
            .lookup(Some("vllm"), None, "qwen3.6-27b", None)
            .is_none());
    }

    #[test]
    fn load_rejects_invalid_entry_capability() {
        let json = json!({
            "libraryVersion": 1,
            "verifiedAt": "2026-08-18",
            "entries": [{
                "platform": "any",
                "model": "bad-model",
                "reasoning": {
                    "supportStatus": "confirmed_supported",
                    "controlKind": "graded",
                    "supportedEfforts": [],
                    "upstream": {"format": "object", "parameter": "reasoning.effort"}
                },
                "sourceUrl": "https://example.com",
                "verifiedAt": "2026-08-18",
                "evidenceLevel": "community"
            }]
        })
        .to_string();
        assert!(load_library_from_str(&json).is_err());
    }

    #[test]
    fn version_tuple_orders_numerically() {
        assert!(version_tuple("0.10.0") > version_tuple("0.9.2"));
        assert!(version_tuple("v1.2.3") == version_tuple("1.2.3"));
    }
}
