use crate::Provider;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub const CODEX_ROUTING_SCHEMA_V2: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum CodexRoutingDocument {
    Legacy(Value),
    V2(CodexRoutingConfigV2),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRoutingParseError {
    pub code: String,
    pub message: String,
}

impl CodexRoutingDocument {
    pub fn parse(value: &Value) -> Result<Self, CodexRoutingParseError> {
        let schema_version = value.get("schemaVersion").and_then(Value::as_u64);
        match schema_version {
            None | Some(1) => Ok(Self::Legacy(value.clone())),
            Some(version) if version != CODEX_ROUTING_SCHEMA_V2 as u64 => {
                Err(CodexRoutingParseError {
                    code: "unsupported_schema_version".to_string(),
                    message: format!("unsupported codexRouting schemaVersion {version}"),
                })
            }
            Some(_) => {
                reject_v2_route_snapshot_fields(value)?;
                serde_json::from_value(value.clone())
                    .map(Self::V2)
                    .map_err(|error| CodexRoutingParseError {
                        code: "invalid_v2_schema".to_string(),
                        message: error.to_string(),
                    })
            }
        }
    }
}

/// schema v2 routes only reference a Provider and selection policy. These v1
/// snapshot fields belong to the referenced Provider and must never persist on
/// a v2 route object.
const V2_ROUTE_FORBIDDEN_INHERITED_FIELDS: &[&str] = &[
    "baseUrl",
    "baseURL",
    "base_url",
    "apiKey",
    "api_key",
    "apiFormat",
    "wireApi",
    "wire_api",
    "capabilities",
    "upstream",
];

fn reject_v2_route_snapshot_fields(value: &Value) -> Result<(), CodexRoutingParseError> {
    let Some(routes) = value.get("routes").and_then(Value::as_array) else {
        return Ok(());
    };
    for (index, route) in routes.iter().enumerate() {
        let Some(route) = route.as_object() else {
            continue;
        };
        if let Some(field) = V2_ROUTE_FORBIDDEN_INHERITED_FIELDS
            .iter()
            .find(|field| route.contains_key(**field))
        {
            return Err(CodexRoutingParseError {
                code: "v2_route_forbidden_field".to_string(),
                message: format!("route[{index}] contains forbidden inherited field `{field}`"),
            });
        }
    }
    Ok(())
}

/// Remove legacy Provider snapshots from schema v2 routes at the common
/// persistence boundary. Parsing remains fail-closed; this narrow migration
/// lets old frontends and external writers converge without changing any v1
/// document, catalog entry, or valid v2 field.
pub(crate) fn strip_v2_route_inherited_fields(settings_config: &mut Value) -> bool {
    if settings_config
        .pointer("/codexRouting/schemaVersion")
        .and_then(Value::as_u64)
        != Some(2)
    {
        return false;
    }
    let Some(routes) = settings_config
        .pointer_mut("/codexRouting/routes")
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    let mut stripped = false;
    for route in routes {
        let Some(route) = route.as_object_mut() else {
            continue;
        };
        for field in V2_ROUTE_FORBIDDEN_INHERITED_FIELDS {
            stripped |= route.remove(*field).is_some();
        }
    }
    stripped
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRoutingConfigV2 {
    pub schema_version: u32,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_route_id: Option<String>,
    #[serde(default)]
    pub routes: Vec<CodexRoutingRouteV2>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_v2: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spawn_agent_models: Vec<String>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexRoutingRouteV2 {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub target_provider_id: String,
    #[serde(default)]
    pub model_selection: CodexModelSelection,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_prefixes: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub auth_policy: CodexRouteAuthPolicy,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CodexModelSelection {
    #[default]
    All,
    Include {
        models: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexRouteAuthPolicy {
    pub source: CodexRouteAuthSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

impl Default for CodexRouteAuthPolicy {
    fn default() -> Self {
        Self {
            source: CodexRouteAuthSource::ProviderConfig,
            account_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexRouteAuthSource {
    ProviderConfig,
    ManagedAccount,
    ManagedCodexOauth,
    NativeCodexAuth,
    AccountPool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRoutingValidationIssue {
    pub code: String,
    pub route_id: Option<String>,
    pub message: String,
}

pub fn validate_v2(
    plan: &CodexRoutingConfigV2,
    providers: &HashMap<String, Provider>,
) -> Result<(), Vec<CodexRoutingValidationIssue>> {
    let mut issues = Vec::new();
    let mut route_ids = HashSet::new();
    for route in &plan.routes {
        let route_id = route.id.trim();
        if route_id.is_empty() {
            push_issue(&mut issues, "route_id_empty", None, "route id is empty");
        } else if !route_ids.insert(route_id.to_ascii_lowercase()) {
            push_issue(
                &mut issues,
                "route_id_duplicate",
                Some(route.id.clone()),
                "route id is duplicated",
            );
        }
        if !route.enabled {
            continue;
        }
        if !providers.contains_key(route.target_provider_id.trim()) {
            push_issue(
                &mut issues,
                "target_provider_missing",
                Some(route.id.clone()),
                "target provider does not exist",
            );
        }
        if let CodexModelSelection::Include { models } = &route.model_selection {
            let selected = models
                .iter()
                .map(|model| model.trim())
                .filter(|model| !model.is_empty())
                .collect::<HashSet<_>>();
            if selected.is_empty() {
                push_issue(
                    &mut issues,
                    "include_models_empty",
                    Some(route.id.clone()),
                    "include selection requires at least one model",
                );
            }
            if selected.len() != models.len() {
                push_issue(
                    &mut issues,
                    "include_models_duplicate_or_empty",
                    Some(route.id.clone()),
                    "include selection contains duplicate or empty models",
                );
            }
            for target in route.aliases.values() {
                let Some(provider) = providers.get(route.target_provider_id.trim()) else {
                    if !selected.contains(target.trim()) {
                        push_issue(
                            &mut issues,
                            "alias_target_not_selected",
                            Some(route.id.clone()),
                            "alias target is outside the include selection",
                        );
                    }
                    continue;
                };
                if !selected.iter().any(|selected_model| {
                    provider_models_equivalent(provider, selected_model, target)
                }) {
                    push_issue(
                        &mut issues,
                        "alias_target_not_selected",
                        Some(route.id.clone()),
                        "alias target is outside the include selection",
                    );
                }
            }
        }
    }

    if let Some(default_route_id) = plan.default_route_id.as_deref() {
        if !plan
            .routes
            .iter()
            .any(|route| route.id.eq_ignore_ascii_case(default_route_id))
        {
            push_issue(
                &mut issues,
                "default_route_missing",
                None,
                "default route does not exist",
            );
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn provider_models_equivalent(provider: &Provider, left: &str, right: &str) -> bool {
    let left = left.trim().to_ascii_lowercase();
    let right = right.trim().to_ascii_lowercase();
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left == right {
        return true;
    }
    let Some(models) = provider
        .settings_config
        .get("modelCatalog")
        .or_else(|| provider.settings_config.get("model_catalog"))
        .and_then(|catalog| catalog.get("models"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    models.iter().any(|model| {
        let identities = ["model", "upstreamModel", "upstream_model"]
            .into_iter()
            .filter_map(|field| model.get(field).and_then(Value::as_str))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<HashSet<_>>();
        identities.contains(&left) && identities.contains(&right)
    })
}

fn push_issue(
    issues: &mut Vec<CodexRoutingValidationIssue>,
    code: &str,
    route_id: Option<String>,
    message: &str,
) {
    issues.push(CodexRoutingValidationIssue {
        code: code.to_string(),
        route_id,
        message: message.to_string(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Provider;
    use serde_json::json;
    use std::collections::HashMap;

    fn provider(id: &str) -> Provider {
        Provider::with_id(id.to_string(), id.to_string(), json!({}), None)
    }

    fn providers() -> HashMap<String, Provider> {
        [("qwen".to_string(), provider("qwen"))]
            .into_iter()
            .collect()
    }

    fn valid_plan(route: serde_json::Value) -> serde_json::Value {
        json!({
            "schemaVersion": 2,
            "enabled": true,
            "defaultRouteId": "router-qwen",
            "routes": [route]
        })
    }

    fn route(selection: serde_json::Value) -> serde_json::Value {
        json!({
            "id": "router-qwen",
            "label": "Qwen",
            "enabled": true,
            "targetProviderId": "qwen",
            "modelSelection": selection,
            "matchPrefixes": ["qwen"],
            "aliases": {"qwen-fast": "qwen3.8"},
            "authPolicy": {"source": "provider_config"}
        })
    }

    #[test]
    fn parses_all_and_include_model_selections() {
        let all = CodexRoutingDocument::parse(&valid_plan(route(json!({"mode": "all"}))))
            .expect("v2 all plan");
        let include = CodexRoutingDocument::parse(&valid_plan(route(json!({
            "mode": "include",
            "models": ["qwen3.8"]
        }))))
        .expect("v2 include plan");

        assert!(matches!(all, CodexRoutingDocument::V2(_)));
        assert!(matches!(include, CodexRoutingDocument::V2(_)));
    }

    #[test]
    fn v2_auth_policy_rejects_legacy_auth_provider_field() {
        let mut route = route(json!({"mode": "all"}));
        route["authPolicy"] = json!({
            "source": "managed_codex_oauth",
            "authProvider": "codex_oauth",
            "accountId": "account-1"
        });

        let error = CodexRoutingDocument::parse(&valid_plan(route))
            .expect_err("schema v2 authPolicy must reject legacy provider fields");

        assert_eq!(error.code, "invalid_v2_schema");
        assert!(error.message.contains("authProvider"));
    }

    #[test]
    fn missing_model_selection_defaults_to_all_in_backend_schema() {
        let value = valid_plan(json!({
            "id": "router-qwen",
            "targetProviderId": "qwen",
            "authPolicy": {"source": "provider_config"}
        }));
        let CodexRoutingDocument::V2(plan) =
            CodexRoutingDocument::parse(&value).expect("v2 plan without modelSelection")
        else {
            panic!("expected v2")
        };

        assert!(matches!(
            plan.routes[0].model_selection,
            CodexModelSelection::All
        ));
    }

    #[test]
    fn missing_schema_version_is_tolerated_as_legacy_without_mutation() {
        let legacy = json!({"routes": [{"id": "legacy"}]});
        let parsed = CodexRoutingDocument::parse(&legacy).expect("legacy plan");
        assert_eq!(parsed, CodexRoutingDocument::Legacy(legacy));
    }

    #[test]
    fn validation_rejects_missing_provider_empty_include_and_bad_alias() {
        let value = valid_plan(json!({
            "id": "router-qwen",
            "targetProviderId": "missing",
            "modelSelection": {"mode": "include", "models": []},
            "aliases": {"qwen-fast": "outside-selection"},
            "authPolicy": {"source": "provider_config"}
        }));
        let CodexRoutingDocument::V2(plan) = CodexRoutingDocument::parse(&value).expect("parse v2")
        else {
            panic!("expected v2")
        };
        let codes = validate_v2(&plan, &providers())
            .expect_err("invalid plan")
            .into_iter()
            .map(|issue| issue.code)
            .collect::<Vec<_>>();

        assert!(codes.contains(&"target_provider_missing".to_string()));
        assert!(codes.contains(&"include_models_empty".to_string()));
        assert!(codes.contains(&"alias_target_not_selected".to_string()));
    }

    #[test]
    fn validation_rejects_duplicate_route_ids_and_unknown_default() {
        let duplicate = route(json!({"mode": "all"}));
        let value = json!({
            "schemaVersion": 2,
            "defaultRouteId": "missing-default",
            "routes": [duplicate.clone(), duplicate]
        });
        let CodexRoutingDocument::V2(plan) = CodexRoutingDocument::parse(&value).expect("parse v2")
        else {
            panic!("expected v2")
        };
        let codes = validate_v2(&plan, &providers())
            .expect_err("invalid plan")
            .into_iter()
            .map(|issue| issue.code)
            .collect::<Vec<_>>();

        assert!(codes.contains(&"route_id_duplicate".to_string()));
        assert!(codes.contains(&"default_route_missing".to_string()));
    }

    #[test]
    fn validation_accepts_visible_selection_with_upstream_alias_target() {
        let relay = Provider::with_id(
            "qwen".to_string(),
            "Qwen".to_string(),
            json!({
                "modelCatalog": {
                    "models": [{
                        "model": "deepseek-v4-flash",
                        "upstreamModel": "deepseek-v4-flash-0731"
                    }]
                }
            }),
            None,
        );
        let providers = [("qwen".to_string(), relay)].into_iter().collect();
        let value = valid_plan(json!({
            "id": "router-qwen",
            "targetProviderId": "qwen",
            "modelSelection": {
                "mode": "include",
                "models": ["deepseek-v4-flash"]
            },
            "aliases": {
                "deepseek-v4-flash-0731": "deepseek-v4-flash-0731"
            },
            "authPolicy": {"source": "provider_config"}
        }));
        let CodexRoutingDocument::V2(plan) = CodexRoutingDocument::parse(&value).expect("parse v2")
        else {
            panic!("expected v2");
        };

        validate_v2(&plan, &providers).expect(
            "visible catalog selection and upstream alias target should refer to the same model",
        );
    }

    #[test]
    fn strips_v2_route_inherited_fields_without_touching_valid_v2_or_model_fields() {
        let mut settings = json!({
            "modelCatalog": {
                "models": [{"model": "deepseek-v4-flash", "apiFormat": "openai_responses"}]
            },
            "codexRouting": {
                "schemaVersion": 2,
                "routes": [{
                    "id": "deepseek",
                    "targetProviderId": "codex-deepseek",
                    "modelSelection": {"mode": "all"},
                    "matchPrefixes": ["deepseek"],
                    "apiFormat": "openai_responses",
                    "upstream": {"apiFormat": "openai_responses"}
                }]
            }
        });

        assert!(strip_v2_route_inherited_fields(&mut settings));
        let route = &settings["codexRouting"]["routes"][0];
        assert!(route.get("apiFormat").is_none());
        assert!(route.get("upstream").is_none());
        assert_eq!(route["targetProviderId"], "codex-deepseek");
        assert_eq!(route["matchPrefixes"][0], "deepseek");
        assert_eq!(
            settings["modelCatalog"]["models"][0]["apiFormat"],
            "openai_responses"
        );

        let mut v1 = json!({
            "codexRouting": {
                "schemaVersion": 1,
                "routes": [{"apiFormat": "openai_responses"}]
            }
        });
        assert!(!strip_v2_route_inherited_fields(&mut v1));
        assert_eq!(
            v1["codexRouting"]["routes"][0]["apiFormat"],
            "openai_responses"
        );
    }

    #[test]
    fn v2_route_rejects_inherited_or_secret_snapshot_fields() {
        for (field, value) in [
            ("baseUrl", json!("https://stale.example")),
            ("apiKey", json!("secret")),
            ("apiFormat", json!("openai_chat")),
            ("capabilities", json!({"textOnly": true})),
            ("upstream", json!({"apiFormat": "openai_chat"})),
        ] {
            let mut route = route(json!({"mode": "all"}));
            route[field] = value;
            let error = CodexRoutingDocument::parse(&valid_plan(route))
                .expect_err("snapshot field must be rejected");
            assert_eq!(error.code, "v2_route_forbidden_field", "field={field}");
        }
    }
}
