use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    str::FromStr,
};

const VALID_EFFORTS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CodexReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
    Ultra,
}

impl CodexReasoningEffort {
    const ORDERED: [Self; 8] = [
        Self::None,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
        Self::Ultra,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }
}

impl fmt::Display for CodexReasoningEffort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CodexReasoningEffort {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ORDERED
            .into_iter()
            .find(|candidate| candidate.as_str() == value)
            .ok_or_else(|| format!("unknown reasoning effort: {value}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSupportKind {
    EffortLevels,
    BooleanOnly,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningConfidence {
    Confirmed,
    Declared,
    Unverified,
}

/// 三态支持状态（模型推理能力 schema v2）。
///
/// 只有明确否定证据才能写 `confirmed_unsupported`；字段缺失、探测失败或
/// 不在维护库中都只能得到 `unknown`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSupportStatus {
    ConfirmedSupported,
    ConfirmedUnsupported,
    Unknown,
}

/// 控制形态（模型推理能力 schema v2）。
///
/// 与支持状态相互独立，不能互相推导：能产生 reasoning、能开关 reasoning、
/// 能分档控制 reasoning 是三个独立能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningControlKind {
    /// 无控制：模型产生 reasoning，但上游未声明开关或档位。
    None,
    /// 仅布尔开关。
    Boolean,
    /// 分档 effort。
    Graded,
    /// token budget 或其他非 effort 控制。
    Budget,
    /// 控制形态未知。
    Unknown,
}

/// 能力声明的证据等级（模型推理能力 schema v2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityConfidence {
    Authoritative,
    Verified,
    Maintained,
    Inferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSubagentReasoningCapability {
    pub support_kind: ReasoningSupportKind,
    pub source: Option<String>,
    pub confidence: ReasoningConfidence,
    pub codex_selectable_efforts: Vec<CodexReasoningEffort>,
    pub provider_accepted_efforts: Vec<CodexReasoningEffort>,
    pub provider_default_effort: Option<CodexReasoningEffort>,
    pub disable_allowed: bool,
    pub effort_map: BTreeMap<CodexReasoningEffort, CodexReasoningEffort>,
    /// 来源能力的稳定指纹；无来源（unknown 兜底）时为空串。
    ///
    /// P2 起 catalog / 请求转换 / Sub-Agent 各层投影必须携带同一指纹，
    /// 任何一层重新按模型名猜测档位都视为实现失败。
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelReasoningUpstream {
    pub format: String,
    pub parameter: String,
    #[serde(default)]
    pub effort_map: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelReasoningCapability {
    /// 能力 schema 版本。缺失表示 legacy v1；新写入固定为 2。
    ///
    /// 注意：这是模型推理能力 schema 的版本，与 Codex Sub-Agent V1/V2 无关，
    /// 代码、错误码与 UI 文案中禁止混用简称。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    /// 三态支持状态（schema v2）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_status: Option<ReasoningSupportStatus>,
    /// 控制形态（schema v2）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_kind: Option<ReasoningControlKind>,
    /// Legacy 字段：仅用于读取旧数据；新写入不得包含。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported: Option<bool>,
    #[serde(default)]
    pub supported_efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub disable_allowed: bool,
    pub upstream: CodexModelReasoningUpstream,
    pub output_format: Option<String>,
    pub source: Option<String>,
    /// 证据等级（schema v2）。易变元数据，不进入能力指纹。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<CapabilityConfidence>,
    /// 检测时间（schema v2）。易变元数据，不进入能力指纹。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<String>,
    /// Provider 身份（schema v2）。易变元数据，不进入能力指纹。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_key: Option<String>,
    /// 模型 revision（schema v2）。易变元数据，不进入能力指纹。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_revision: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningCapabilityRepairOutcome {
    pub repaired_models: Vec<String>,
    pub warnings: Vec<String>,
}

impl CodexModelReasoningCapability {
    /// 生效三态状态：`supportStatus` 优先；legacy `supported` 仅用于读旧数据。
    pub fn effective_support_status(&self) -> ReasoningSupportStatus {
        if let Some(status) = self.support_status {
            return status;
        }
        match self.supported {
            Some(true) => ReasoningSupportStatus::ConfirmedSupported,
            Some(false) => ReasoningSupportStatus::ConfirmedUnsupported,
            None => ReasoningSupportStatus::Unknown,
        }
    }

    /// 生效控制形态：`controlKind` 优先；legacy 数据从声明字段推导。
    pub fn effective_control_kind(&self) -> ReasoningControlKind {
        if let Some(kind) = self.control_kind {
            return kind;
        }
        if !self.supported_efforts.is_empty() {
            return ReasoningControlKind::Graded;
        }
        match self.upstream.format.as_str() {
            "boolean" => ReasoningControlKind::Boolean,
            "none" => ReasoningControlKind::None,
            _ => ReasoningControlKind::Unknown,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if let (Some(status), Some(legacy)) = (self.support_status, self.supported) {
            let legacy_status = if legacy {
                ReasoningSupportStatus::ConfirmedSupported
            } else {
                ReasoningSupportStatus::ConfirmedUnsupported
            };
            if status != legacy_status {
                return Err("supportStatus contradicts legacy supported field".to_string());
            }
        }
        if self
            .supported_efforts
            .iter()
            .any(|effort| !VALID_EFFORTS.contains(&effort.as_str()))
        {
            return Err("supportedEfforts contains an unknown effort".to_string());
        }
        if let Some(default_effort) = self.default_effort.as_deref() {
            if !self
                .supported_efforts
                .iter()
                .any(|item| item == default_effort)
            {
                return Err("defaultEffort must be present in supportedEfforts".to_string());
            }
        }
        if !self.disable_allowed && self.supported_efforts.iter().any(|effort| effort == "none") {
            return Err("none requires disableAllowed=true".to_string());
        }
        match self.effective_control_kind() {
            ReasoningControlKind::Graded if self.supported_efforts.is_empty() => {
                return Err("controlKind graded requires nonempty supportedEfforts".to_string());
            }
            ReasoningControlKind::Boolean | ReasoningControlKind::None
                if !self.supported_efforts.is_empty() =>
            {
                return Err("controlKind boolean/none cannot advertise efforts".to_string());
            }
            _ => {}
        }
        if self.effective_support_status() == ReasoningSupportStatus::ConfirmedUnsupported {
            if !self.supported_efforts.is_empty() {
                return Err("unsupported capability cannot advertise efforts".to_string());
            }
            if self.disable_allowed {
                return Err("unsupported capability cannot allow disabling".to_string());
            }
        }
        if self.upstream.effort_map.iter().any(|(source, target)| {
            !VALID_EFFORTS.contains(&source.as_str()) || !VALID_EFFORTS.contains(&target.as_str())
        }) {
            return Err("effortMap contains an unknown effort".to_string());
        }
        if self.upstream.effort_map.values().any(|target| {
            !self
                .supported_efforts
                .iter()
                .any(|supported| supported == target)
        }) {
            return Err("effortMap target must be present in supportedEfforts".to_string());
        }
        Ok(())
    }
}

/// 稳定能力指纹：只覆盖影响运行的规范化字段。
///
/// 不包含 `fetchedAt`、`source`、`confidence`、`schemaVersion` 等易变元数据；
/// legacy 与 v2 声明只要运行语义相同就产生相同指纹。`supportedEfforts` 按
/// 规范化顺序去重，`effortMap` 按键排序，保证与字段书写顺序无关。
pub fn capability_fingerprint(capability: &CodexModelReasoningCapability) -> String {
    let mut efforts: Vec<&str> = capability
        .supported_efforts
        .iter()
        .map(String::as_str)
        .collect();
    efforts.sort_unstable();
    efforts.dedup();
    let effort_map: BTreeMap<&str, &str> = capability
        .upstream
        .effort_map
        .iter()
        .map(|(source, target)| (source.as_str(), target.as_str()))
        .collect();
    let canonical = serde_json::json!({
        "supportStatus": capability.effective_support_status(),
        "controlKind": capability.effective_control_kind(),
        "supportedEfforts": efforts,
        "defaultEffort": capability.default_effort,
        "disableAllowed": capability.disable_allowed,
        "upstream": {
            "format": capability.upstream.format,
            "parameter": capability.upstream.parameter,
            "effortMap": effort_map,
        },
        "outputFormat": capability.output_format,
    });
    let digest = Sha256::digest(canonical.to_string().as_bytes());
    format!("{digest:x}")
}

/// Repair persisted reasoning metadata that older CCSwitchMulti versions allowed to drift.
///
/// Exact DeepSeek V4 model IDs use the maintained built-in declaration. Unknown models keep
/// their own declared levels and merely choose the first valid level when their default is
/// invalid; they never inherit GPT or DeepSeek capabilities.
pub fn repair_invalid_reasoning_capabilities(
    settings: &mut Value,
) -> ReasoningCapabilityRepairOutcome {
    let mut outcome = ReasoningCapabilityRepairOutcome::default();
    let Some(models) = settings
        .get_mut("modelCatalog")
        .and_then(|catalog| catalog.get_mut("models"))
        .and_then(Value::as_array_mut)
    else {
        return outcome;
    };

    for entry in models {
        let model = ["model", "id", "slug", "upstreamModel", "upstream_model"]
            .into_iter()
            .find_map(|field| entry.get(field).and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_string();
        let Some(reasoning) = entry.get("reasoning") else {
            continue;
        };
        let parsed = serde_json::from_value::<CodexModelReasoningCapability>(reasoning.clone());
        if parsed.as_ref().is_ok_and(|value| value.validate().is_ok()) {
            continue;
        }

        if let Some(builtin) = builtin_reasoning_capability_for_model(&model) {
            entry["reasoning"] = serde_json::to_value(builtin)
                .expect("built-in reasoning capability must be serializable");
            outcome.repaired_models.push(model.clone());
            outcome.warnings.push(format!(
                "Restored maintained reasoning capability for {model}"
            ));
            continue;
        }

        let Some(object) = entry.get_mut("reasoning").and_then(Value::as_object_mut) else {
            continue;
        };
        let supported = object
            .get("supportedEfforts")
            .and_then(Value::as_array)
            .map(|values| {
                let mut seen = HashSet::new();
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|effort| VALID_EFFORTS.contains(effort))
                    .filter(|effort| seen.insert((*effort).to_string()))
                    .map(|effort| Value::String(effort.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let supported_names = supported
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<HashSet<_>>();
        object.insert("supportedEfforts".to_string(), Value::Array(supported));

        let default_is_valid = object
            .get("defaultEffort")
            .and_then(Value::as_str)
            .is_some_and(|effort| supported_names.contains(effort));
        if !default_is_valid {
            if let Some(first) = supported_names
                .iter()
                .min_by_key(|effort| {
                    VALID_EFFORTS
                        .iter()
                        .position(|candidate| candidate == &effort.as_str())
                        .unwrap_or(usize::MAX)
                })
                .cloned()
            {
                object.insert("defaultEffort".to_string(), Value::String(first));
            } else {
                object.remove("defaultEffort");
            }
        }
        if let Some(effort_map) = object
            .get_mut("upstream")
            .and_then(Value::as_object_mut)
            .and_then(|upstream| upstream.get_mut("effortMap"))
            .and_then(Value::as_object_mut)
        {
            effort_map.retain(|source, target| {
                VALID_EFFORTS.contains(&source.as_str())
                    && target
                        .as_str()
                        .is_some_and(|target| supported_names.contains(target))
            });
        }
        outcome.repaired_models.push(model.clone());
        outcome.warnings.push(format!(
            "Repaired invalid reasoning metadata for {model} using only its declared efforts"
        ));
    }

    outcome
}

pub fn resolve_subagent_reasoning_capability(
    capability: Option<&CodexModelReasoningCapability>,
) -> ResolvedSubagentReasoningCapability {
    let Some(capability) = capability.filter(|value| value.validate().is_ok()) else {
        return ResolvedSubagentReasoningCapability {
            support_kind: ReasoningSupportKind::Unknown,
            source: None,
            confidence: ReasoningConfidence::Unverified,
            codex_selectable_efforts: Vec::new(),
            provider_accepted_efforts: Vec::new(),
            provider_default_effort: None,
            disable_allowed: false,
            effort_map: BTreeMap::new(),
            fingerprint: String::new(),
        };
    };

    let provider_accepted_efforts = CodexReasoningEffort::ORDERED
        .into_iter()
        .filter(|effort| {
            capability
                .supported_efforts
                .iter()
                .any(|value| value == effort.as_str())
        })
        .collect::<Vec<_>>();
    let provider_effort_set = provider_accepted_efforts
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut effort_map = provider_accepted_efforts
        .iter()
        .copied()
        .map(|effort| (effort, effort))
        .collect::<BTreeMap<_, _>>();
    for (source, target) in &capability.upstream.effort_map {
        let Ok(source) = source.parse::<CodexReasoningEffort>() else {
            continue;
        };
        let Ok(target) = target.parse::<CodexReasoningEffort>() else {
            continue;
        };
        if provider_effort_set.contains(&target) {
            effort_map.insert(source, target);
        }
    }

    let selectable_set = provider_effort_set.iter().copied().collect::<HashSet<_>>();
    let codex_selectable_efforts = CodexReasoningEffort::ORDERED
        .into_iter()
        .filter(|effort| selectable_set.contains(effort))
        .collect();
    let support_kind = match capability.effective_support_status() {
        ReasoningSupportStatus::ConfirmedUnsupported => ReasoningSupportKind::Unsupported,
        ReasoningSupportStatus::Unknown => ReasoningSupportKind::Unknown,
        ReasoningSupportStatus::ConfirmedSupported => {
            if !provider_accepted_efforts.is_empty() {
                ReasoningSupportKind::EffortLevels
            } else if capability.upstream.format == "boolean" {
                ReasoningSupportKind::BooleanOnly
            } else {
                // 支持已确认但未声明控制形态：按控制未知处理，不暴露任何档位，
                // 也不继承 GPT 通用档位。
                ReasoningSupportKind::Unknown
            }
        }
    };
    let confidence = match capability.source.as_deref() {
        Some("builtin") => ReasoningConfidence::Confirmed,
        Some("user") => ReasoningConfidence::Declared,
        _ => ReasoningConfidence::Unverified,
    };

    ResolvedSubagentReasoningCapability {
        support_kind,
        source: capability.source.clone(),
        confidence,
        codex_selectable_efforts,
        provider_accepted_efforts,
        provider_default_effort: capability
            .default_effort
            .as_deref()
            .and_then(|value| value.parse().ok()),
        disable_allowed: capability.disable_allowed,
        effort_map,
        fingerprint: capability_fingerprint(capability),
    }
}

/// Return CCSwitchMulti's maintained capability for exact, stable model IDs.
///
/// This is a migration fallback for Provider/model-catalog rows saved before reasoning metadata
/// became part of the persisted schema. Explicit row metadata remains authoritative. Keep this
/// list narrow: unknown third-party models must not inherit GPT effort levels.
pub fn builtin_reasoning_capability_for_model(
    model: &str,
) -> Option<CodexModelReasoningCapability> {
    let normalized = model.trim().to_ascii_lowercase();
    // 官方维护清单：DeepSeek V4 与 Kimi K3 均支持 reasoning_effort: low/high/max（默认 high）。
    // 保持精确匹配，未知第三方模型不得继承 GPT 通用档位。
    if !matches!(
        normalized.as_str(),
        "deepseek-v4-flash" | "deepseek-v4-pro" | "k3" | "k3-256k"
    ) {
        return None;
    }
    // DeepSeek Responses 返回 reasoning_content 字段；Kimi 响应字段未确认，
    // 不声明 output_format（代理层按默认行为处理，避免错误字段破坏转换）。
    let output_format = if normalized.starts_with("deepseek") {
        Some("reasoning_content".into())
    } else {
        None
    };
    Some(CodexModelReasoningCapability {
        schema_version: Some(2),
        support_status: Some(ReasoningSupportStatus::ConfirmedSupported),
        control_kind: Some(ReasoningControlKind::Graded),
        supported: None,
        supported_efforts: vec!["low".into(), "high".into(), "max".into()],
        default_effort: Some("high".into()),
        disable_allowed: true,
        upstream: CodexModelReasoningUpstream {
            format: "string".into(),
            parameter: "reasoning_effort".into(),
            effort_map: [
                ("low".into(), "low".into()),
                ("medium".into(), "high".into()),
                ("high".into(), "high".into()),
                ("xhigh".into(), "high".into()),
                ("max".into(), "max".into()),
            ]
            .into_iter()
            .collect(),
        },
        output_format,
        source: Some("builtin".into()),
        confidence: Some(CapabilityConfidence::Maintained),
        fetched_at: None,
        provider_key: None,
        model_revision: None,
    })
}

pub fn reasoning_capability_from_model_entry(
    model_entry: &Value,
) -> Option<CodexModelReasoningCapability> {
    let value = model_entry.get("reasoning")?;
    if value.is_null() {
        // reasoning: null 与缺失等价，均视为"未声明"
        return None;
    }
    let model = model_entry
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let capability: CodexModelReasoningCapability = match serde_json::from_value(value.clone()) {
        Ok(capability) => capability,
        Err(error) => {
            // 声明存在但无法解析：打日志暴露问题，避免用户手动声明
            // 被静默当作"未声明"而清空（v27/v28 回归）。
            log::warn!(
                "Codex reasoning declaration for model {model} is not parseable and will be ignored: {error}"
            );
            return None;
        }
    };
    if let Err(error) = capability.validate() {
        log::warn!(
            "Codex reasoning declaration for model {model} is invalid and will be ignored: {error}"
        );
        return None;
    }
    Some(capability)
}

pub fn resolve_reasoning_capability_from_settings(
    settings: &Value,
    model: &str,
) -> Option<CodexModelReasoningCapability> {
    settings
        .get("modelCatalog")?
        .get("models")?
        .as_array()?
        .iter()
        .find(|entry| {
            ["model", "id", "slug", "upstreamModel", "upstream_model"]
                .into_iter()
                .filter_map(|field| entry.get(field).and_then(Value::as_str))
                .any(|candidate| candidate.trim().eq_ignore_ascii_case(model.trim()))
        })
        .and_then(reasoning_capability_from_model_entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn efforts(values: &[&str]) -> Vec<CodexReasoningEffort> {
        values
            .iter()
            .map(|value| value.parse().expect("valid reasoning effort fixture"))
            .collect()
    }

    #[test]
    fn repair_legacy_deepseek_reasoning_restores_builtin_capability() {
        let mut settings = json!({
            "modelCatalog": {"models": [{
                "model": "deepseek-v4-flash",
                "reasoning": {
                    "supported": true,
                    "supportedEfforts": ["low", "high", "max"],
                    "defaultEffort": "medium",
                    "disableAllowed": true,
                    "upstream": {"format": "string", "parameter": "reasoning_effort"},
                    "source": "builtin"
                }
            }]}
        });

        let outcome = repair_invalid_reasoning_capabilities(&mut settings);
        let repaired = &settings["modelCatalog"]["models"][0]["reasoning"];
        assert_eq!(repaired["supportedEfforts"], json!(["low", "high", "max"]));
        assert_eq!(repaired["defaultEffort"], json!("high"));
        assert!(outcome
            .repaired_models
            .contains(&"deepseek-v4-flash".to_string()));
        assert!(!outcome.warnings.is_empty());
    }

    #[test]
    fn repair_unknown_reasoning_uses_only_declared_supported_efforts() {
        let mut settings = json!({
            "modelCatalog": {"models": [{
                "model": "private-model",
                "reasoning": {
                    "supported": true,
                    "supportedEfforts": ["low"],
                    "defaultEffort": "high",
                    "disableAllowed": false,
                    "upstream": {"format": "string", "parameter": "reasoning_effort"},
                    "source": "user"
                }
            }]}
        });

        let outcome = repair_invalid_reasoning_capabilities(&mut settings);
        let repaired = &settings["modelCatalog"]["models"][0]["reasoning"];
        assert_eq!(repaired["supportedEfforts"], json!(["low"]));
        assert_eq!(repaired["defaultEffort"], json!("low"));
        assert_eq!(repaired["source"], json!("user"));
        assert!(outcome
            .repaired_models
            .contains(&"private-model".to_string()));
    }

    fn deepseek_capability() -> CodexModelReasoningCapability {
        serde_json::from_value(json!({
            "supported": true,
            "supportedEfforts": ["low", "high", "max"],
            "defaultEffort": "high",
            "disableAllowed": true,
            "upstream": {
                "format": "string",
                "parameter": "reasoning_effort",
                "effortMap": {
                    "low": "low",
                    "medium": "high",
                    "high": "high",
                    "xhigh": "high",
                    "max": "max"
                }
            },
            "source": "builtin"
        }))
        .expect("DeepSeek fixture")
    }

    #[test]
    fn deepseek_resolution_separates_provider_and_codex_efforts() {
        let resolved = resolve_subagent_reasoning_capability(Some(&deepseek_capability()));
        assert_eq!(
            resolved.provider_accepted_efforts,
            efforts(&["low", "high", "max"])
        );
        assert_eq!(
            resolved.codex_selectable_efforts,
            efforts(&["low", "high", "max"])
        );
        assert_eq!(
            resolved.effort_map.get(&CodexReasoningEffort::Medium),
            Some(&CodexReasoningEffort::High)
        );
        assert!(!resolved
            .codex_selectable_efforts
            .contains(&CodexReasoningEffort::Ultra));
    }

    #[test]
    fn unknown_capability_does_not_advertise_candidate_efforts() {
        let resolved = resolve_subagent_reasoning_capability(None);
        assert_eq!(resolved.support_kind, ReasoningSupportKind::Unknown);
        assert_eq!(resolved.confidence, ReasoningConfidence::Unverified);
        assert!(resolved.codex_selectable_efforts.is_empty());
        assert!(resolved.provider_accepted_efforts.is_empty());
    }

    #[test]
    fn rejects_mapping_to_effort_the_provider_does_not_accept() {
        let mut capability = deepseek_capability();
        capability
            .upstream
            .effort_map
            .insert("medium".to_string(), "medium".to_string());
        assert_eq!(
            capability.validate(),
            Err("effortMap target must be present in supportedEfforts".to_string())
        );
    }

    #[test]
    fn resolves_visible_or_upstream_model_from_same_catalog_row() {
        let settings = json!({"modelCatalog":{"models":[{
            "model":"visible-glm", "upstreamModel":"glm-5.2",
            "reasoning":{"supported":true,"supportedEfforts":["high","max"],
                "defaultEffort":"max","disableAllowed":false,
                "upstream":{"format":"string","parameter":"reasoning_effort"}}
        }]}});
        assert_eq!(
            resolve_reasoning_capability_from_settings(&settings, "visible-glm")
                .and_then(|capability| capability.default_effort),
            Some("max".to_string())
        );
        assert!(resolve_reasoning_capability_from_settings(&settings, "glm-5.2").is_some());
    }

    #[test]
    fn rejects_invalid_default_instead_of_guessing() {
        let settings = json!({"modelCatalog":{"models":[{
            "model":"broken",
            "reasoning":{"supported":true,"supportedEfforts":["low"],
                "defaultEffort":"high","disableAllowed":false,
                "upstream":{"format":"string","parameter":"reasoning_effort"}}
        }]}});
        assert!(resolve_reasoning_capability_from_settings(&settings, "broken").is_none());
    }

    #[test]
    fn restores_only_exact_builtin_deepseek_v4_capabilities() {
        let flash = builtin_reasoning_capability_for_model("deepseek-v4-flash")
            .expect("known Flash capability");
        let pro = builtin_reasoning_capability_for_model("DEEPSEEK-V4-PRO")
            .expect("known Pro capability");
        assert_eq!(flash.supported_efforts, vec!["low", "high", "max"]);
        assert_eq!(pro.default_effort.as_deref(), Some("high"));
        assert!(builtin_reasoning_capability_for_model("deepseek-v4-flash-preview").is_none());
        assert!(builtin_reasoning_capability_for_model("vendor/deepseek-v4-pro").is_none());
    }

    #[test]
    fn restores_exact_kimi_k3_capabilities() {
        for model in ["k3", "K3-256K", "k3-256k"] {
            let capability = builtin_reasoning_capability_for_model(model)
                .unwrap_or_else(|| panic!("{model} must resolve a Kimi capability"));
            assert_eq!(capability.supported_efforts, vec!["low", "high", "max"]);
            assert_eq!(capability.default_effort.as_deref(), Some("high"));
            // Kimi 响应字段未确认，output_format 保持 None
            assert_eq!(capability.output_format, None);
            assert_eq!(capability.source.as_deref(), Some("builtin"));
        }
        assert!(builtin_reasoning_capability_for_model("k3-ultra").is_none());
        assert!(builtin_reasoning_capability_for_model("vendor/k3").is_none());
    }

    // ===== P0 契约：三态 schema v2 与能力指纹 =====

    #[test]
    fn new_schema_unknown_parses_and_resolves_unknown() {
        // 字段缺失/未声明 = unknown，绝不继承 GPT 档位，也不得被当作解析失败丢弃。
        let capability: CodexModelReasoningCapability = serde_json::from_value(json!({
            "schemaVersion": 2,
            "supportStatus": "unknown",
            "controlKind": "unknown",
            "supportedEfforts": [],
            "disableAllowed": false,
            "upstream": {"format": "none", "parameter": "none"},
            "source": "provider"
        }))
        .expect("schema v2 unknown capability must parse");
        assert_eq!(
            capability.effective_support_status(),
            ReasoningSupportStatus::Unknown
        );
        assert_eq!(
            capability.effective_control_kind(),
            ReasoningControlKind::Unknown
        );
        assert!(capability.validate().is_ok());

        let resolved = resolve_subagent_reasoning_capability(Some(&capability));
        assert_eq!(resolved.support_kind, ReasoningSupportKind::Unknown);
        assert!(resolved.codex_selectable_efforts.is_empty());
        assert!(resolved.provider_accepted_efforts.is_empty());
        assert!(resolved.provider_default_effort.is_none());
        assert!(!resolved.disable_allowed);
        // resolved 结果必须携带来源能力的稳定指纹。
        assert_eq!(resolved.fingerprint, capability_fingerprint(&capability));
        assert!(!resolved.fingerprint.is_empty());
    }

    #[test]
    fn new_schema_confirmed_unsupported_rejects_efforts_and_disable() {
        let with_efforts: CodexModelReasoningCapability = serde_json::from_value(json!({
            "schemaVersion": 2,
            "supportStatus": "confirmed_unsupported",
            "supportedEfforts": ["low"],
            "disableAllowed": false,
            "upstream": {"format": "none", "parameter": "none"}
        }))
        .expect("parse");
        assert_eq!(
            with_efforts.validate(),
            Err("unsupported capability cannot advertise efforts".to_string())
        );

        let with_disable: CodexModelReasoningCapability = serde_json::from_value(json!({
            "schemaVersion": 2,
            "supportStatus": "confirmed_unsupported",
            "supportedEfforts": [],
            "disableAllowed": true,
            "upstream": {"format": "none", "parameter": "none"}
        }))
        .expect("parse");
        assert_eq!(
            with_disable.validate(),
            Err("unsupported capability cannot allow disabling".to_string())
        );

        // 无效声明不得被信任为 confirmed_unsupported（fail-closed 回退 unknown）；
        // 有效的 confirmed_unsupported 才解析为 Unsupported。
        let invalid_resolved = resolve_subagent_reasoning_capability(Some(&with_disable));
        assert_eq!(invalid_resolved.support_kind, ReasoningSupportKind::Unknown);

        let valid_unsupported: CodexModelReasoningCapability = serde_json::from_value(json!({
            "schemaVersion": 2,
            "supportStatus": "confirmed_unsupported",
            "supportedEfforts": [],
            "disableAllowed": false,
            "upstream": {"format": "none", "parameter": "none"}
        }))
        .expect("parse");
        let resolved = resolve_subagent_reasoning_capability(Some(&valid_unsupported));
        assert_eq!(resolved.support_kind, ReasoningSupportKind::Unsupported);
        assert!(resolved.codex_selectable_efforts.is_empty());
    }

    #[test]
    fn explicit_empty_efforts_are_not_filled_by_template() {
        // 明确的 supportedEfforts=[] 是“无分档”声明：任何投影都不得回退到通用档位。
        let capability: CodexModelReasoningCapability = serde_json::from_value(json!({
            "schemaVersion": 2,
            "supportStatus": "confirmed_supported",
            "controlKind": "boolean",
            "supportedEfforts": [],
            "disableAllowed": true,
            "upstream": {"format": "boolean", "parameter": "enable_thinking"},
            "outputFormat": "reasoning_content",
            "source": "user"
        }))
        .expect("schema v2 boolean capability must parse");
        assert!(capability.validate().is_ok());

        let resolved = resolve_subagent_reasoning_capability(Some(&capability));
        assert_eq!(resolved.support_kind, ReasoningSupportKind::BooleanOnly);
        assert!(resolved.codex_selectable_efforts.is_empty());
        assert!(resolved.provider_accepted_efforts.is_empty());
        assert!(resolved.provider_default_effort.is_none());
        assert!(resolved.disable_allowed);
    }

    #[test]
    fn legacy_supported_bool_still_parses_and_derives_status() {
        let legacy_true: CodexModelReasoningCapability = serde_json::from_value(json!({
            "supported": true,
            "supportedEfforts": ["low", "high"],
            "defaultEffort": "high",
            "disableAllowed": false,
            "upstream": {"format": "string", "parameter": "reasoning_effort"}
        }))
        .expect("legacy capability must parse");
        assert_eq!(
            legacy_true.effective_support_status(),
            ReasoningSupportStatus::ConfirmedSupported
        );
        assert_eq!(
            legacy_true.effective_control_kind(),
            ReasoningControlKind::Graded
        );

        let legacy_false: CodexModelReasoningCapability = serde_json::from_value(json!({
            "supported": false,
            "supportedEfforts": [],
            "disableAllowed": false,
            "upstream": {"format": "none", "parameter": "none"}
        }))
        .expect("legacy capability must parse");
        assert_eq!(
            legacy_false.effective_support_status(),
            ReasoningSupportStatus::ConfirmedUnsupported
        );
        assert_eq!(
            legacy_false.effective_control_kind(),
            ReasoningControlKind::None
        );
    }

    #[test]
    fn support_status_contradicting_legacy_supported_is_rejected() {
        let capability: CodexModelReasoningCapability = serde_json::from_value(json!({
            "schemaVersion": 2,
            "supportStatus": "confirmed_supported",
            "supported": false,
            "supportedEfforts": ["low"],
            "disableAllowed": false,
            "upstream": {"format": "string", "parameter": "reasoning_effort"}
        }))
        .expect("parse");
        assert_eq!(
            capability.validate(),
            Err("supportStatus contradicts legacy supported field".to_string())
        );
    }

    #[test]
    fn fingerprint_is_stable_across_volatile_metadata() {
        let base: CodexModelReasoningCapability = serde_json::from_value(json!({
            "schemaVersion": 2,
            "supportStatus": "confirmed_supported",
            "controlKind": "graded",
            "supportedEfforts": ["low", "high", "max"],
            "defaultEffort": "high",
            "disableAllowed": true,
            "upstream": {
                "format": "string",
                "parameter": "reasoning_effort",
                "effortMap": {"medium": "high", "low": "low"}
            },
            "outputFormat": "reasoning_content",
            "source": "builtin",
            "confidence": "maintained",
            "fetchedAt": "2026-08-17T00:00:00Z",
            "providerKey": "deepseek",
            "modelRevision": "v4"
        }))
        .expect("parse");
        let mut volatile = base.clone();
        volatile.fetched_at = Some("2031-01-01T00:00:00Z".into());
        volatile.source = Some("user".into());
        volatile.confidence = Some(CapabilityConfidence::Inferred);
        volatile.provider_key = Some("other".into());
        volatile.model_revision = Some("v9".into());
        volatile.schema_version = None;
        // legacy 形态但运行语义相同：指纹必须一致。
        let mut legacy = base.clone();
        legacy.schema_version = None;
        legacy.support_status = None;
        legacy.control_kind = None;
        legacy.supported = Some(true);
        assert_eq!(
            capability_fingerprint(&base),
            capability_fingerprint(&volatile)
        );
        assert_eq!(
            capability_fingerprint(&base),
            capability_fingerprint(&legacy)
        );
    }

    #[test]
    fn fingerprint_changes_with_execution_fields() {
        let base: CodexModelReasoningCapability = serde_json::from_value(json!({
            "schemaVersion": 2,
            "supportStatus": "confirmed_supported",
            "controlKind": "graded",
            "supportedEfforts": ["low", "high", "max"],
            "defaultEffort": "high",
            "disableAllowed": true,
            "upstream": {"format": "string", "parameter": "reasoning_effort"}
        }))
        .expect("parse");
        let base_fingerprint = capability_fingerprint(&base);

        let mut different_default = base.clone();
        different_default.default_effort = Some("max".into());
        assert_ne!(base_fingerprint, capability_fingerprint(&different_default));

        let mut different_disable = base.clone();
        different_disable.disable_allowed = false;
        assert_ne!(base_fingerprint, capability_fingerprint(&different_disable));

        // 档位顺序不同但集合相同：指纹不变。
        let mut reordered = base.clone();
        reordered.supported_efforts = vec!["max".into(), "low".into(), "high".into()];
        assert_eq!(base_fingerprint, capability_fingerprint(&reordered));

        let mut different_parameter = base.clone();
        different_parameter.upstream.parameter = "thinking".into();
        assert_ne!(
            base_fingerprint,
            capability_fingerprint(&different_parameter)
        );
    }
}
