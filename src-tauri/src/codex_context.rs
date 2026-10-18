//! Validate explicit official Provider windows without changing any model defaults.

use crate::{error::AppError, provider::Provider};
use serde_json::Value;

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
    let value = entry.get(camel).or_else(|| entry.get(snake))?;
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
