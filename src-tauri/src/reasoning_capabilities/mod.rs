//! Codex 第三方模型推理能力：统一来源链（P1）。
//!
//! 单一 resolver 入口 [`resolve_codex_model_capability`]——所有消费者（catalog 投影、
//! 请求转换、Sub-Agent policy、GUI/CLI inspect）必须经由该入口，不得再按模型名
//! 重新猜测档位。任何一层重新按模型名猜测档位都视为实现失败。
//!
//! 来源优先级（高 → 低）：
//! 1. 用户模型级声明（`modelCatalog.models[].reasoning`）——始终最高优先级；
//! 2. 动态检测候选快照（TTL、只读元数据、禁止主动推理试探）；
//! 3. CCSM 维护的版本化能力库（随应用打包的独立 JSON 资源）；
//! 4. 内置清单（deepseek-v4 / k3）；
//! 5. Codex 官方模型缓存（仅未知平台生效；OpenRouter/vLLM 等聚合平台不套用）；
//! 6. Unknown（fail-closed）。
//!
//! 核心原则：缺失证据不是不存在的证据。`NotAdvertised`/`Unavailable`/`Invalid`
//! 以及库/内置未命中，都只能得到 `unknown`，绝不自动生成 `confirmed_unsupported`。

pub mod catalog;
pub mod provider_metadata;

use crate::provider::Provider;
use crate::proxy::providers::codex_reasoning::{
    builtin_reasoning_capability_for_model, capability_fingerprint,
    official_reasoning_capability_for_model, resolve_reasoning_capability_from_settings,
    CapabilityConfidence, CodexModelReasoningCapability, CodexModelReasoningUpstream,
    ReasoningControlKind, ReasoningSupportStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 能力来源（用于指纹溯源与 UI 展示）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource {
    /// 用户模型级显式声明（含 `user_confirmed_detection` 采用后的覆盖）。
    UserConfig,
    /// 动态只读检测（TTL 候选快照）。
    Detection,
    /// CCSM 维护的版本化能力库。
    Library,
    /// 内置清单。
    Builtin,
    /// Codex 官方模型缓存（仅未知平台生效；OpenRouter/vLLM 等聚合平台不套用）。
    Official,
    /// 无来源命中，fail-closed unknown。
    Unknown,
}

impl CapabilitySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserConfig => "user_config",
            Self::Detection => "detection",
            Self::Library => "library",
            Self::Builtin => "builtin",
            Self::Official => "official",
            Self::Unknown => "unknown",
        }
    }
}

/// 通用 Provider 能力快照（可扩展：reasoning / 工具调用 / 结构化输出 / 模态 /
/// 上下文 / 端点协议）。首版只填充 reasoning 子对象，避免以后为其他能力
/// 重新发明探测框架。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilitySnapshot {
    pub provider_key: String,
    pub model: String,
    /// Unix 毫秒时间戳。
    pub fetched_at: i64,
    /// 来源标识：`openrouter_api` / `vllm_server` / ...
    pub source: String,
    pub reasoning: Option<ReasoningCapabilitySnapshot>,
}

/// 快照的 reasoning 子对象（仅 allowlist 字段，不含任何敏感信息）。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningCapabilitySnapshot {
    /// 平台声明的可选 effort（含 `none` 表示支持显式关闭）。
    pub supported_efforts: Vec<String>,
    pub default_effort: Option<String>,
    /// 推理强制开启（不可关闭）。
    pub mandatory: bool,
    /// 服务端默认是否开启推理。
    pub default_enabled: Option<bool>,
    /// 支持 token budget 控制（无分档时的替代控制形态）。
    pub supports_max_tokens: bool,
    /// 上游 wire 形态（平台相关）：`object`（reasoning.effort）/ `string`
    /// （reasoning_effort）/ `boolean`（enable_thinking）等。
    pub upstream_format: Option<String>,
    /// 上游参数名。
    pub upstream_parameter: Option<String>,
    /// reasoning 内容回传字段形态。
    pub output_format: Option<String>,
}

/// 适配器结果。`NotAdvertised`/`Unavailable`/`Invalid` 均不能自动生成
/// `confirmed_unsupported`——缺失证据不是不存在的证据。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryOutcome {
    Found(ProviderCapabilitySnapshot),
    /// 端点可达，但模型/字段未声明。
    NotAdvertised,
    /// 端点不可达 / 404 / 鉴权失败。
    Unavailable,
    /// 响应无法解析 / 结构非法。
    Invalid,
}

/// Resolver 输出：能力 + 来源 + 指纹。
#[derive(Clone, Debug)]
pub struct ResolvedModelCapability {
    pub capability: Option<CodexModelReasoningCapability>,
    pub source: CapabilitySource,
    pub fingerprint: String,
}

/// 动态检测候选快照的 TTL。
pub const DETECTION_TTL: Duration = Duration::from_secs(60 * 60);

/// 内存 TTL 候选快照缓存（per provider+model）。
#[derive(Default)]
pub struct DetectionCache {
    entries: HashMap<String, (ProviderCapabilitySnapshot, Instant)>,
}

impl DetectionCache {
    fn key(provider_key: &str, model: &str) -> String {
        format!("{provider_key}\u{1f}{model}")
    }

    /// 读取未过期的候选快照；过期或不存在返回 None。
    pub fn get(&self, provider_key: &str, model: &str) -> Option<&ProviderCapabilitySnapshot> {
        self.entries
            .get(&Self::key(provider_key, model))
            .filter(|(_, inserted_at)| inserted_at.elapsed() < DETECTION_TTL)
            .map(|(snapshot, _)| snapshot)
    }

    pub fn insert(&mut self, snapshot: ProviderCapabilitySnapshot) {
        self.entries.insert(
            Self::key(&snapshot.provider_key, &snapshot.model),
            (snapshot, Instant::now()),
        );
    }

    pub fn prune_expired(&mut self) {
        self.entries
            .retain(|_, (_, inserted_at)| inserted_at.elapsed() < DETECTION_TTL);
    }
}

/// 全局检测缓存（请求路径与检测触发方共享）。
static DETECTION_CACHE: OnceLock<Mutex<DetectionCache>> = OnceLock::new();

pub fn detection_cache() -> &'static Mutex<DetectionCache> {
    DETECTION_CACHE.get_or_init(|| Mutex::new(DetectionCache::default()))
}

/// 读取当前 provider+model 的 TTL 候选快照（无或过期返回 None）。
pub fn current_detection(provider_key: &str, model: &str) -> Option<ProviderCapabilitySnapshot> {
    let cache = detection_cache().lock().expect("detection cache poisoned");
    cache.get(provider_key, model).cloned()
}

/// Tauri 资源目录（由 setup hook 初始化；测试/开发可缺省）。
static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init_resource_dir(dir: PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

pub fn resource_dir() -> Option<PathBuf> {
    RESOURCE_DIR.get().cloned()
}

/// 单一 resolver 入口（请求路径用；同步、无网络）。
///
/// `detection` 为当前 TTL 候选快照（可为 None）；用户配置始终最高优先级。
/// 能力库读取自全局懒加载缓存，official 来源读取 Codex 官方模型缓存。
pub fn resolve_codex_model_capability(
    provider: &Provider,
    model: &str,
    detection: Option<&ProviderCapabilitySnapshot>,
) -> ResolvedModelCapability {
    let platform = provider_metadata::detect_platform(provider);
    let official_models = crate::codex_config::codex_official_models_cache().unwrap_or_default();
    resolve_codex_model_capability_core(
        &provider.settings_config,
        platform,
        model,
        detection,
        catalog::global_library().as_ref(),
        &official_models,
    )
}

/// 测试用 resolver 入口（可注入能力库；不加载 official 缓存，保持确定性）。
///
/// 来源优先级（高 → 低）：用户配置 > 检测候选 > 能力库 > 内置 > official（仅未知平台）> unknown。
pub fn resolve_codex_model_capability_with_library(
    provider: &Provider,
    model: &str,
    detection: Option<&ProviderCapabilitySnapshot>,
    library: Option<&catalog::CapabilityLibrary>,
) -> ResolvedModelCapability {
    let platform = provider_metadata::detect_platform(provider);
    resolve_codex_model_capability_core(
        &provider.settings_config,
        platform,
        model,
        detection,
        library,
        &[],
    )
}

/// Resolver 核心（settings-based、纯函数、无网络、无全局状态）。
///
/// 所有消费者（catalog 投影、请求转换、Sub-Agent policy、GUI/CLI inspect）
/// 必须经由该核心，保证同一模型四层 fingerprint 一致。任何一层绕过该核心
/// 重新按模型名猜测档位都视为实现失败。
///
/// 来源优先级（高 → 低）：
/// 1. 用户模型级声明（始终最高优先级）；
/// 2. 动态检测候选快照（TTL、只读元数据）；
/// 3. CCSM 维护的版本化能力库；
/// 4. 内置清单（deepseek-v4 / k3）；
/// 5. Codex 官方模型缓存（仅 `platform=None` 即未知平台生效；OpenRouter/vLLM
///    等已知聚合平台有自己的推理接口，不得套用官方 OpenAI 形态）；
/// 6. Unknown（fail-closed）。
pub fn resolve_codex_model_capability_core(
    settings: &Value,
    platform: Option<&str>,
    model: &str,
    detection: Option<&ProviderCapabilitySnapshot>,
    library: Option<&catalog::CapabilityLibrary>,
    official_models: &[Value],
) -> ResolvedModelCapability {
    // 1. 用户模型级声明（始终最高优先级）
    if let Some(capability) = resolve_reasoning_capability_from_settings(settings, model) {
        return ResolvedModelCapability {
            fingerprint: capability_fingerprint(&capability),
            capability: Some(capability),
            source: CapabilitySource::UserConfig,
        };
    }

    // 2. 动态检测候选（只读元数据，TTL）
    if let Some(snapshot) = detection {
        if let Some(capability) = snapshot_to_capability(snapshot) {
            return ResolvedModelCapability {
                fingerprint: capability_fingerprint(&capability),
                capability: Some(capability),
                source: CapabilitySource::Detection,
            };
        }
    }

    // 3. CCSM 维护的版本化能力库
    if let Some(library) = library {
        if let Some(capability) = catalog::lookup_library_capability(library, platform, model) {
            return ResolvedModelCapability {
                fingerprint: capability_fingerprint(&capability),
                capability: Some(capability),
                source: CapabilitySource::Library,
            };
        }
    }

    // 4. 内置清单
    if let Some(capability) = builtin_reasoning_capability_for_model(model) {
        return ResolvedModelCapability {
            fingerprint: capability_fingerprint(&capability),
            capability: Some(capability),
            source: CapabilitySource::Builtin,
        };
    }

    // 5. Codex 官方模型缓存（仅未知平台生效）
    if platform.is_none() {
        if let Some(capability) = official_reasoning_capability_for_model(model, official_models) {
            return ResolvedModelCapability {
                fingerprint: capability_fingerprint(&capability),
                capability: Some(capability),
                source: CapabilitySource::Official,
            };
        }
    }

    // 6. Unknown（fail-closed）
    ResolvedModelCapability {
        capability: None,
        source: CapabilitySource::Unknown,
        fingerprint: String::new(),
    }
}

/// 把检测快照转换为已校验的能力。
///
/// 非法快照（如 `default_effort` 不在 `supported_efforts` 内）返回 None，
/// 继续落到库/内置——绝不信任畸形检测为 confirmed。
pub fn snapshot_to_capability(
    snapshot: &ProviderCapabilitySnapshot,
) -> Option<CodexModelReasoningCapability> {
    let reasoning = snapshot.reasoning.as_ref()?;
    let efforts: Vec<String> = reasoning
        .supported_efforts
        .iter()
        .map(|effort| effort.trim().to_ascii_lowercase())
        .filter(|effort| !effort.is_empty())
        .collect();

    let disable_allowed = efforts.iter().any(|effort| effort == "none");
    let graded_efforts: Vec<String> = efforts
        .iter()
        .filter(|effort| **effort != "none")
        .cloned()
        .collect();

    let control_kind = if !graded_efforts.is_empty() {
        ReasoningControlKind::Graded
    } else if reasoning.supports_max_tokens {
        ReasoningControlKind::Budget
    } else if reasoning.mandatory {
        // 强制开启且无控制：模型产生 reasoning，但上游未声明开关或档位。
        ReasoningControlKind::None
    } else {
        // 无分档、无预算、非强制：无法确认支持，返回 None 落到库/内置。
        return None;
    };

    let mut supported_efforts = graded_efforts.clone();
    if disable_allowed {
        supported_efforts.insert(0, "none".to_string());
    }

    let capability = CodexModelReasoningCapability {
        schema_version: Some(2),
        support_status: Some(ReasoningSupportStatus::ConfirmedSupported),
        control_kind: Some(control_kind),
        supported: None,
        supported_efforts,
        default_effort: reasoning.default_effort.clone(),
        disable_allowed,
        upstream: CodexModelReasoningUpstream {
            format: reasoning
                .upstream_format
                .clone()
                .unwrap_or_else(|| "object".to_string()),
            parameter: reasoning
                .upstream_parameter
                .clone()
                .unwrap_or_else(|| "reasoning.effort".to_string()),
            effort_map: graded_efforts
                .iter()
                .map(|effort| (effort.clone(), effort.clone()))
                .collect(),
        },
        output_format: reasoning.output_format.clone(),
        source: Some("detection".to_string()),
        confidence: Some(CapabilityConfidence::Verified),
        fetched_at: Some(snapshot.fetched_at.to_string()),
        provider_key: Some(snapshot.provider_key.clone()),
        model_revision: None,
        codex_ultra_orchestration: None,
    };

    capability.validate().ok().map(|_| capability)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::codex_reasoning::ReasoningControlKind;
    use serde_json::json;

    fn provider_with_catalog(model: &str, reasoning: Option<serde_json::Value>) -> Provider {
        let mut entry = json!({ "model": model });
        if let Some(reasoning) = reasoning {
            entry["reasoning"] = reasoning;
        }
        Provider {
            id: "test-provider".into(),
            name: "Test Provider".into(),
            settings_config: json!({ "modelCatalog": { "models": [entry] } }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn plain_provider(name: &str, base_url: &str) -> Provider {
        Provider {
            id: "test-provider".into(),
            name: name.into(),
            settings_config: json!({ "base_url": base_url }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    fn user_declared_reasoning() -> serde_json::Value {
        json!({
            "schemaVersion": 2,
            "supportStatus": "confirmed_supported",
            "controlKind": "graded",
            "supportedEfforts": ["low", "high"],
            "defaultEffort": "high",
            "disableAllowed": false,
            "upstream": {"format": "string", "parameter": "reasoning_effort"},
            "source": "user"
        })
    }

    fn openrouter_detection_snapshot() -> ProviderCapabilitySnapshot {
        ProviderCapabilitySnapshot {
            provider_key: "test-provider".into(),
            model: "deepseek/deepseek-v4-pro".into(),
            fetched_at: 1_700_000_000_000,
            source: "openrouter_api".into(),
            reasoning: Some(ReasoningCapabilitySnapshot {
                supported_efforts: vec!["max".into(), "high".into(), "low".into()],
                default_effort: Some("high".into()),
                mandatory: false,
                default_enabled: Some(true),
                supports_max_tokens: false,
                upstream_format: Some("object".into()),
                upstream_parameter: Some("reasoning.effort".into()),
                output_format: Some("auto".into()),
            }),
        }
    }

    fn library_with_openrouter_deepseek() -> catalog::CapabilityLibrary {
        catalog::load_library_from_str(
            r#"{
                "libraryVersion": 1,
                "verifiedAt": "2026-08-18",
                "entries": [{
                    "platform": "openrouter",
                    "model": "deepseek/deepseek-v4-pro",
                    "reasoning": {
                        "schemaVersion": 2,
                        "supportStatus": "confirmed_supported",
                        "controlKind": "graded",
                        "supportedEfforts": ["max", "high", "low"],
                        "defaultEffort": "high",
                        "disableAllowed": false,
                        "upstream": {"format": "object", "parameter": "reasoning.effort"},
                        "outputFormat": "auto",
                        "source": "library"
                    },
                    "sourceUrl": "https://openrouter.ai/api/v1/models",
                    "verifiedAt": "2026-08-18",
                    "evidenceLevel": "platform_api"
                }]
            }"#,
        )
        .expect("valid test library")
    }

    #[test]
    fn user_config_beats_detection_and_library() {
        let provider =
            provider_with_catalog("deepseek/deepseek-v4-pro", Some(user_declared_reasoning()));
        let detection = openrouter_detection_snapshot();
        let library = library_with_openrouter_deepseek();
        let resolved = resolve_codex_model_capability_with_library(
            &provider,
            "deepseek/deepseek-v4-pro",
            Some(&detection),
            Some(&library),
        );
        assert_eq!(resolved.source, CapabilitySource::UserConfig);
        assert_eq!(
            resolved.capability.as_ref().unwrap().supported_efforts,
            vec!["low", "high"]
        );
    }

    #[test]
    fn detection_beats_library_when_user_absent() {
        let provider = plain_provider("OpenRouter", "https://openrouter.ai/api/v1");
        let detection = openrouter_detection_snapshot();
        let library = library_with_openrouter_deepseek();
        let resolved = resolve_codex_model_capability_with_library(
            &provider,
            "deepseek/deepseek-v4-pro",
            Some(&detection),
            Some(&library),
        );
        assert_eq!(resolved.source, CapabilitySource::Detection);
        assert_eq!(
            resolved.capability.as_ref().unwrap().supported_efforts,
            vec!["max", "high", "low"]
        );
    }

    #[test]
    fn library_beats_builtin_when_detection_absent() {
        let provider = plain_provider("OpenRouter", "https://openrouter.ai/api/v1");
        let library = library_with_openrouter_deepseek();
        let resolved = resolve_codex_model_capability_with_library(
            &provider,
            "deepseek/deepseek-v4-pro",
            None,
            Some(&library),
        );
        assert_eq!(resolved.source, CapabilitySource::Library);
        // 指纹为 64 位十六进制 sha256（无前缀）。
        assert_eq!(resolved.fingerprint.len(), 64);
        assert!(resolved.fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn builtin_beats_unknown_when_library_misses() {
        // deepseek-v4-flash 在内置清单中；库未命中时落到内置。
        let provider = plain_provider("Some Gateway", "https://example.com/v1");
        let resolved =
            resolve_codex_model_capability_with_library(&provider, "deepseek-v4-flash", None, None);
        assert_eq!(resolved.source, CapabilitySource::Builtin);
        assert!(resolved.capability.is_some());
    }

    #[test]
    fn unknown_when_no_source_hits() {
        let provider = plain_provider("Some Gateway", "https://example.com/v1");
        let resolved =
            resolve_codex_model_capability_with_library(&provider, "mystery-model-xyz", None, None);
        assert_eq!(resolved.source, CapabilitySource::Unknown);
        assert!(resolved.capability.is_none());
        assert_eq!(resolved.fingerprint, "");
    }

    #[test]
    fn invalid_detection_snapshot_falls_through_to_library() {
        // default_effort 不在 supported_efforts 内 → 快照非法 → 落到库。
        let mut detection = openrouter_detection_snapshot();
        detection.reasoning.as_mut().unwrap().default_effort = Some("ultra".into());
        let provider = plain_provider("OpenRouter", "https://openrouter.ai/api/v1");
        let library = library_with_openrouter_deepseek();
        let resolved = resolve_codex_model_capability_with_library(
            &provider,
            "deepseek/deepseek-v4-pro",
            Some(&detection),
            Some(&library),
        );
        assert_eq!(resolved.source, CapabilitySource::Library);
    }

    #[test]
    fn snapshot_to_capability_graded_with_none_is_disable_allowed() {
        let mut snapshot = openrouter_detection_snapshot();
        snapshot.reasoning.as_mut().unwrap().supported_efforts = vec!["high".into(), "none".into()];
        snapshot.reasoning.as_mut().unwrap().default_effort = Some("high".into());
        let capability = snapshot_to_capability(&snapshot).expect("valid snapshot");
        assert!(capability.disable_allowed);
        assert_eq!(capability.supported_efforts, vec!["none", "high"]);
        assert_eq!(capability.control_kind, Some(ReasoningControlKind::Graded));
    }

    #[test]
    fn snapshot_to_capability_budget_when_only_max_tokens() {
        let snapshot = ProviderCapabilitySnapshot {
            provider_key: "p".into(),
            model: "m".into(),
            fetched_at: 0,
            source: "openrouter_api".into(),
            reasoning: Some(ReasoningCapabilitySnapshot {
                supported_efforts: vec![],
                default_effort: None,
                mandatory: false,
                default_enabled: Some(true),
                supports_max_tokens: true,
                upstream_format: None,
                upstream_parameter: None,
                output_format: None,
            }),
        };
        let capability = snapshot_to_capability(&snapshot).expect("budget snapshot");
        assert_eq!(capability.control_kind, Some(ReasoningControlKind::Budget));
        assert!(capability.supported_efforts.is_empty());
    }

    #[test]
    fn snapshot_to_capability_none_when_no_evidence() {
        // 无分档、无预算、非强制：无法确认支持 → None。
        let snapshot = ProviderCapabilitySnapshot {
            provider_key: "p".into(),
            model: "m".into(),
            fetched_at: 0,
            source: "openrouter_api".into(),
            reasoning: Some(ReasoningCapabilitySnapshot::default()),
        };
        assert!(snapshot_to_capability(&snapshot).is_none());
    }

    #[test]
    fn detection_cache_ttl_expiry() {
        let mut cache = DetectionCache::default();
        let snapshot = openrouter_detection_snapshot();
        cache.insert(snapshot.clone());
        assert!(cache
            .get("test-provider", "deepseek/deepseek-v4-pro")
            .is_some());
        // 未知 key 不命中。
        assert!(cache.get("other", "deepseek/deepseek-v4-pro").is_none());
        // 空缓存不命中。
        let empty = DetectionCache::default();
        assert!(empty
            .get("test-provider", "deepseek/deepseek-v4-pro")
            .is_none());
    }

    #[test]
    fn fingerprint_stable_across_sources_with_same_semantics() {
        // 同一运行语义（efforts/default/upstream）在不同来源下指纹一致。
        let detection = openrouter_detection_snapshot();
        let library = library_with_openrouter_deepseek();
        let provider = plain_provider("OpenRouter", "https://openrouter.ai/api/v1");
        let from_detection = resolve_codex_model_capability_with_library(
            &provider,
            "deepseek/deepseek-v4-pro",
            Some(&detection),
            None,
        );
        let from_library = resolve_codex_model_capability_with_library(
            &provider,
            "deepseek/deepseek-v4-pro",
            None,
            Some(&library),
        );
        assert_eq!(
            from_detection.fingerprint, from_library.fingerprint,
            "same execution semantics must produce the same fingerprint"
        );
    }

    fn official_models_fixture() -> Vec<serde_json::Value> {
        vec![json!({
            "slug": "gpt-5.4",
            "supported_reasoning_levels": ["low", "medium", "high", "xhigh", "max"],
            "default_reasoning_level": "medium"
        })]
    }

    #[test]
    fn resolver_core_official_source_for_unknown_platform() {
        // platform=None（未知平台，含 OpenAI 直连与 catalog 投影）命中 official 来源。
        let settings = json!({});
        let official = official_models_fixture();
        let resolved =
            resolve_codex_model_capability_core(&settings, None, "gpt-5.4", None, None, &official);
        assert_eq!(resolved.source, CapabilitySource::Official);
        assert!(!resolved.fingerprint.is_empty());
        let capability = resolved.capability.expect("official capability");
        assert_eq!(capability.source.as_deref(), Some("official"));
        assert_eq!(
            capability.supported_efforts,
            vec!["low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(capability.default_effort.as_deref(), Some("medium"));
        // 官方 GPT 走 OpenAI 顶层 reasoning_effort 字段，effort_map 用 identity。
        assert_eq!(capability.upstream.format, "string");
        assert_eq!(capability.upstream.parameter, "reasoning_effort");
    }

    #[test]
    fn resolver_core_official_source_skipped_for_known_platform() {
        // platform=openrouter（已知聚合平台）不套用官方 OpenAI 形态，落到 unknown。
        let settings = json!({});
        let official = official_models_fixture();
        let resolved = resolve_codex_model_capability_core(
            &settings,
            Some("openrouter"),
            "gpt-5.4",
            None,
            None,
            &official,
        );
        assert_eq!(resolved.source, CapabilitySource::Unknown);
        assert!(resolved.capability.is_none());
    }

    #[test]
    fn resolver_core_user_config_beats_official() {
        // 用户模型级声明始终最高优先级，覆盖 official 来源。
        let settings = json!({
            "modelCatalog": {"models": [{
                "model": "gpt-5.4",
                "reasoning": {
                    "schemaVersion": 2,
                    "supportStatus": "confirmed_supported",
                    "controlKind": "graded",
                    "supportedEfforts": ["low", "high"],
                    "defaultEffort": "high",
                    "disableAllowed": false,
                    "upstream": {"format": "string", "parameter": "reasoning_effort"},
                    "source": "user"
                }
            }]}
        });
        let official = official_models_fixture();
        let resolved =
            resolve_codex_model_capability_core(&settings, None, "gpt-5.4", None, None, &official);
        assert_eq!(resolved.source, CapabilitySource::UserConfig);
        let capability = resolved.capability.expect("user capability");
        assert_eq!(capability.supported_efforts, vec!["low", "high"]);
    }

    #[test]
    fn resolver_core_official_empty_cache_falls_to_unknown() {
        // 官方缓存为空（fresh install）时，GPT 模型落到 unknown（fail-closed）。
        let settings = json!({});
        let resolved =
            resolve_codex_model_capability_core(&settings, None, "gpt-5.4", None, None, &[]);
        assert_eq!(resolved.source, CapabilitySource::Unknown);
        assert!(resolved.capability.is_none());
    }
}
