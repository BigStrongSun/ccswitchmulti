use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

const VALID_EFFORTS: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

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
        Ok(())
    }
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
}
