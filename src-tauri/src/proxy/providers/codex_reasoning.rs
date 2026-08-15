use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    pub supported: bool,
    #[serde(default)]
    pub supported_efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub disable_allowed: bool,
    pub upstream: CodexModelReasoningUpstream,
    pub output_format: Option<String>,
    pub source: Option<String>,
}

impl CodexModelReasoningCapability {
    pub fn validate(&self) -> Result<(), String> {
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
        if !self.supported && !self.supported_efforts.is_empty() {
            return Err("unsupported capability cannot advertise efforts".to_string());
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

    let selectable_set = effort_map
        .keys()
        .copied()
        .chain(
            capability
                .disable_allowed
                .then_some(CodexReasoningEffort::None),
        )
        .collect::<HashSet<_>>();
    let codex_selectable_efforts = CodexReasoningEffort::ORDERED
        .into_iter()
        .filter(|effort| selectable_set.contains(effort))
        .collect();
    let support_kind = if !capability.supported {
        ReasoningSupportKind::Unsupported
    } else if !provider_accepted_efforts.is_empty() {
        ReasoningSupportKind::EffortLevels
    } else if capability.upstream.format == "boolean" {
        ReasoningSupportKind::BooleanOnly
    } else {
        ReasoningSupportKind::Unknown
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
    if !matches!(normalized.as_str(), "deepseek-v4-flash" | "deepseek-v4-pro") {
        return None;
    }
    Some(CodexModelReasoningCapability {
        supported: true,
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
        output_format: Some("reasoning_content".into()),
        source: Some("builtin".into()),
    })
}

pub fn reasoning_capability_from_model_entry(
    model_entry: &Value,
) -> Option<CodexModelReasoningCapability> {
    let value = model_entry.get("reasoning")?;
    let capability: CodexModelReasoningCapability = serde_json::from_value(value.clone()).ok()?;
    capability.validate().ok()?;
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
        assert!(outcome.repaired_models.contains(&"deepseek-v4-flash".to_string()));
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
        assert!(outcome.repaired_models.contains(&"private-model".to_string()));
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
            efforts(&["none", "low", "medium", "high", "xhigh", "max"])
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
}
