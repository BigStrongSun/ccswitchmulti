//! Resolve official defaults and validate explicit Provider context windows.

use crate::{error::AppError, provider::Provider};
use serde_json::Value;

const OFFICIAL_CONTEXT_DEFAULTS: &str =
    include_str!("../../src/config/codexOfficialContextDefaults.json");

/// Look up a published model default without treating it as an account limit.
pub(crate) fn official_default_context_window(model: &str) -> Option<u64> {
    let catalog: Value = serde_json::from_str(OFFICIAL_CONTEXT_DEFAULTS).ok()?;
    catalog
        .get("models")?
        .as_array()?
        .iter()
        .find(|entry| {
            entry
                .get("model")
                .and_then(Value::as_str)
                .is_some_and(|id| id.eq_ignore_ascii_case(model.trim()))
        })?
        .get("contextWindow")?
        .as_u64()
        .filter(|window| *window > 0)
}

pub(crate) fn resolved_official_context_window(
    provider: &Provider,
    model: &str,
    max_context_window: Option<u64>,
) -> Option<(u64, &'static str)> {
    if !crate::codex_config::codex_provider_record_is_official(provider) {
        return None;
    }

    let account_id = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.codex_official_auth.as_ref())
        .and_then(|auth| auth.account_id.as_deref())
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let account_model = account_id.and_then(|account_id| {
        crate::services::codex_oauth_models::authenticated_model_limits(account_id)
            .into_iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(model.trim()))
    });
    let account_max = account_model
        .as_ref()
        .and_then(|entry| entry.max_context_window);
    let trusted_max = account_max.or_else(|| official_max_context_window(model));
    let max_context_window = match (trusted_max, max_context_window) {
        (Some(trusted), Some(row)) => Some(trusted.min(row)),
        (Some(trusted), None) => Some(trusted),
        (None, row) => row,
    };

    if let Some(context_window) = account_model
        .as_ref()
        .and_then(|entry| entry.context_window)
    {
        return Some((
            max_context_window.map_or(context_window, |limit| context_window.min(limit)),
            "account_directory",
        ));
    }

    let context_window = official_default_context_window(model)?;
    Some((
        max_context_window.map_or(context_window, |limit| context_window.min(limit)),
        "official_default",
    ))
}

fn official_max_context_window(model: &str) -> Option<u64> {
    let models = crate::codex_config::codex_official_models_cache()?;
    let entry = models.into_iter().find(|entry| {
        ["slug", "model", "id"].iter().any(|key| {
            entry
                .get(*key)
                .and_then(Value::as_str)
                .is_some_and(|id| id.eq_ignore_ascii_case(model.trim()))
        })
    })?;
    entry
        .get("max_context_window")
        .or_else(|| entry.get("maxContextWindow"))
        .and_then(Value::as_u64)
}

/// Apply published/account defaults only to the effective settings clone.
pub(crate) fn apply_official_context_defaults(provider: &Provider, settings: &mut Value) {
    if !crate::codex_config::codex_provider_record_is_official(provider) {
        return;
    }
    let catalog_key = if settings.get("modelCatalog").is_some() {
        "modelCatalog"
    } else {
        "model_catalog"
    };
    if positive_integer(settings, "contextWindow", "context_window").is_some()
        || positive_integer(settings, "modelContextWindow", "model_context_window").is_some()
    {
        return;
    }

    let Some(models) = settings
        .get_mut(catalog_key)
        .and_then(|catalog| catalog.get_mut("models"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };

    for model in models {
        if positive_integer(model, "contextWindow", "context_window").is_some() {
            continue;
        }
        let model_id = ["upstreamModel", "upstream_model", "model", "id"]
            .iter()
            .find_map(|key| {
                model
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
            });
        let Some(model_id) = model_id else {
            continue;
        };
        let row_max = model
            .get("maxContextWindow")
            .or_else(|| model.get("max_context_window"))
            .and_then(Value::as_u64);
        let Some((context_window, _source)) =
            resolved_official_context_window(provider, model_id, row_max)
        else {
            continue;
        };
        if let Some(object) = model.as_object_mut() {
            object.insert("contextWindow".to_string(), Value::from(context_window));
        }
    }
}

pub const ONE_MILLION: u64 = 1_000_000;

pub(crate) fn validate_official_context_changes(
    provider: &Provider,
    previous: Option<&Provider>,
    official_models: &[Value],
) -> Result<(), AppError> {
    if !crate::codex_config::codex_provider_record_is_official(provider) {
        return Ok(());
    }
    for entry in models(provider) {
        let Some(window) = positive_integer(entry, "contextWindow", "context_window") else {
            continue;
        };
        let Some(model) = model_id(entry) else {
            continue;
        };
        let upstream = upstream_id(entry).unwrap_or(model);
        // Unrelated edits must not invalidate legacy values. Changing the upstream
        // or authentication binding requires fresh capability validation.
        let unchanged = previous
            .filter(|old| {
                crate::codex_config::codex_provider_record_is_official(old)
                    && old
                        .meta
                        .as_ref()
                        .and_then(|m| m.codex_official_auth.as_ref())
                        == provider
                            .meta
                            .as_ref()
                            .and_then(|m| m.codex_official_auth.as_ref())
            })
            .is_some_and(|old| {
                models(old).iter().any(|prior| {
                    model_id(prior) == Some(model)
                        && upstream_id(prior).unwrap_or(model) == upstream
                        && positive_integer(prior, "contextWindow", "context_window")
                            == Some(window)
                })
            });
        if unchanged {
            continue;
        }
        let maximum = official_models
            .iter()
            .find(|official| {
                ["slug", "model", "id"].iter().any(|key| {
                    official
                        .get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|id| id.trim().eq_ignore_ascii_case(upstream.trim()))
                })
            })
            .and_then(|official| {
                positive_integer(official, "max_context_window", "maxContextWindow")
            });
        if maximum.is_some_and(|max| window > max) || (window >= ONE_MILLION && maximum.is_none()) {
            return Err(AppError::InvalidInput(format!(
                "Official model '{model}' context {window} is not supported by trusted metadata (max: {}). Refresh its official model catalog before changing the context window.",
                maximum.map(|max| max.to_string()).unwrap_or_else(|| "unknown".into())
            )));
        }
    }
    Ok(())
}

fn models(provider: &Provider) -> &[Value] {
    provider
        .settings_config
        .get("modelCatalog")
        .or_else(|| provider.settings_config.get("model_catalog"))
        .and_then(|catalog| catalog.get("models"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn model_id(entry: &Value) -> Option<&str> {
    entry
        .get("model")
        .or_else(|| entry.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

fn upstream_id(entry: &Value) -> Option<&str> {
    entry
        .get("upstreamModel")
        .or_else(|| entry.get("upstream_model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

fn positive_integer(entry: &Value, camel: &str, snake: &str) -> Option<u64> {
    explicit_context_window(entry, &[camel, snake])
}

pub(crate) fn explicit_context_window(entry: &Value, fields: &[&str]) -> Option<u64> {
    let value = fields.iter().find_map(|field| entry.get(*field))?;
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(rows: Value) -> Provider {
        let mut provider = Provider::with_id(
            "official".into(),
            "Official".into(),
            json!({"modelCatalog":{"models":rows}}),
            None,
        );
        provider.category = Some("official".into());
        provider
    }

    #[test]
    fn published_default_is_versioned_and_model_specific() {
        assert_eq!(
            official_default_context_window("GPT-6.1-Sol"),
            Some(500_000)
        );
        assert_eq!(official_default_context_window("gpt-6.1"), None);
        assert_eq!(official_default_context_window("gpt-6.1-sol-mini"), None);
    }

    #[test]
    fn explicit_context_window_accepts_legacy_numeric_strings() {
        assert_eq!(
            explicit_context_window(&json!({"contextWindow":"272000"}), &["contextWindow"]),
            Some(272_000)
        );
    }

    #[test]
    fn effective_defaults_preserve_explicit_and_non_official_values_and_respect_low_max() {
        let mut official = provider(json!([
            {"model":"gpt-6.1-sol"},
            {"model":"gpt-6.1-sol-capped", "maxContextWindow":400000},
            {"model":"gpt-6.1-sol-custom", "contextWindow":272000}
        ]));
        let mut settings = official.settings_config.clone();
        settings["modelCatalog"]["models"].as_array_mut().unwrap()[1]["upstreamModel"] =
            json!("gpt-6.1-sol");
        settings["modelCatalog"]["models"].as_array_mut().unwrap()[2]["upstreamModel"] =
            json!("gpt-6.1-sol");
        apply_official_context_defaults(&official, &mut settings);
        assert_eq!(
            settings["modelCatalog"]["models"][0]["contextWindow"],
            500000
        );
        assert_eq!(
            settings["modelCatalog"]["models"][1]["contextWindow"],
            400000
        );
        assert_eq!(
            settings["modelCatalog"]["models"][2]["contextWindow"],
            272000
        );

        official.category = None;
        let mut third_party = official.clone();
        third_party.settings_config = json!({"modelCatalog":{"models":[{"model":"gpt-6.1-sol"}]}});
        let mut third_party_settings = third_party.settings_config.clone();
        apply_official_context_defaults(&third_party, &mut third_party_settings);
        assert!(third_party_settings["modelCatalog"]["models"][0]
            .get("contextWindow")
            .is_none());
    }

    #[test]
    fn authenticated_context_default_and_max_override_published_reference() {
        let account_id = format!("context-default-test-{}", uuid::Uuid::new_v4());
        let mut provider = provider(json!([]));
        provider.meta = Some(crate::provider::ProviderMeta {
            codex_official_auth: Some(crate::provider::CodexOfficialAuthConfig {
                mode: crate::provider::CodexOfficialAuthMode::ManagedOauth,
                account_id: Some(account_id.clone()),
            }),
            ..Default::default()
        });
        crate::services::codex_oauth_models::record_authenticated_model_limits(
            &account_id,
            &[crate::services::model_fetch::FetchedModel {
                id: "gpt-6.1-sol".to_string(),
                owned_by: None,
                context_window: Some(600_000),
                max_context_window: Some(450_000),
                max_input_tokens: None,
                max_output_tokens: None,
                input_modalities: None,
                supports_image: None,
                reasoning: None,
            }],
        );

        assert_eq!(
            resolved_official_context_window(&provider, "gpt-6.1-sol", None),
            Some((450_000, "account_directory"))
        );
    }

    #[test]
    fn only_declared_million_capability_allows_million_context() {
        for max in [None, Some(872_000), Some(1_000_000), Some(1_050_000)] {
            let official =
                vec![json!({"slug":"gpt-test","context_window":272000,"max_context_window":max})];
            let candidate = provider(
                json!([{"model":"gpt-test","contextWindow":1000000,"maxContextWindow":9999999}]),
            );
            assert_eq!(
                validate_official_context_changes(&candidate, None, &official).is_ok(),
                max.is_some_and(|m| m >= ONE_MILLION)
            );
            assert_eq!(
                candidate.settings_config["modelCatalog"]["models"][0]["contextWindow"],
                1000000
            );
        }
    }

    #[test]
    fn aliases_use_upstream_and_never_invent_a_max_from_default() {
        let candidate =
            provider(json!([{"model":"alias","upstreamModel":"gpt-test","contextWindow":1000000}]));
        assert!(validate_official_context_changes(
            &candidate,
            None,
            &[json!({"slug":"gpt-test","context_window":1050000})]
        )
        .is_err());
        assert!(validate_official_context_changes(
            &candidate,
            None,
            &[json!({"slug":"gpt-test","max_context_window":1050000})]
        )
        .is_ok());
        assert!(validate_official_context_changes(
            &candidate,
            None,
            &[json!({"slug":"alias","max_context_window":1050000})]
        )
        .is_err());
    }

    #[test]
    fn legacy_window_is_preserved_but_changed_window_or_binding_is_validated() {
        let old = provider(json!([{"model":"gpt-test","contextWindow":1050000}]));
        let mut updated = old.clone();
        updated.name = "Renamed".into();
        assert!(validate_official_context_changes(&updated, Some(&old), &[]).is_ok());
        updated.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(1000000);
        assert!(validate_official_context_changes(&updated, Some(&old), &[]).is_err());
        updated = old.clone();
        updated.settings_config["modelCatalog"]["models"][0]["upstreamModel"] = json!("another");
        assert!(validate_official_context_changes(&updated, Some(&old), &[]).is_err());
        updated = old.clone();
        updated.meta = Some(crate::provider::ProviderMeta {
            codex_official_auth: Some(crate::provider::CodexOfficialAuthConfig {
                mode: crate::provider::CodexOfficialAuthMode::ManagedOauth,
                account_id: Some("different-account".into()),
            }),
            ..Default::default()
        });
        assert!(validate_official_context_changes(&updated, Some(&old), &[]).is_err());
    }

    #[test]
    fn third_party_and_unedited_small_models_are_unchanged() {
        let mut candidate = provider(json!([{"model":"private","contextWindow":2000000}]));
        candidate.category = None;
        assert!(validate_official_context_changes(&candidate, None, &[]).is_ok());
        candidate.category = Some("official".into());
        candidate.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(272000);
        let before = candidate.settings_config.clone();
        assert!(validate_official_context_changes(&candidate, None, &[]).is_ok());
        assert_eq!(candidate.settings_config, before);
    }
}
