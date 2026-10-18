//! Pure helpers for projecting a bounded Codex context-window preference.
//!
//! The preference requests at most one million tokens; effective values always
//! remain bounded by trusted official model metadata. A request is not evidence
//! that the service has unlocked the requested window.

use crate::provider::Provider;
use serde::Serialize;
use serde_json::{json, Value};
use toml_edit::{value as toml_value, DocumentMut};

pub const REQUESTED_CONTEXT_WINDOW: u64 = 1_000_000;
const NATIVE_CONTEXT_MARKER: &str = "CCSM_OFFICIAL_CONTEXT_1M_V1:";
const NATIVE_CONTEXT_MARKER_FAMILY: &str = "CCSM_OFFICIAL_CONTEXT_";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexContextWindowStatus {
    pub model: String,
    pub default_context_window: Option<u64>,
    pub max_context_window: Option<u64>,
    pub requested_context_window: Option<u64>,
    pub effective_context_window: Option<u64>,
    pub auto_compact_token_limit: Option<u64>,
}

/// Resolve a context preference from caller-verified official model metadata.
///
/// If the official max is missing, the declared default is the only safe cap;
/// no capability is inferred from the requested preference. With the feature
/// disabled, the effective window remains the official default.
pub fn codex_context_window_status(
    model: &str,
    official_model: &Value,
    enabled: bool,
) -> CodexContextWindowStatus {
    let default_context_window = positive_u64(official_model.get("context_window"));
    let declared_max = positive_u64(official_model.get("max_context_window"));
    let max_context_window = declared_max;
    let safe_cap = declared_max.or(default_context_window);
    let requested_context_window = enabled.then_some(REQUESTED_CONTEXT_WINDOW);
    let effective_context_window = if enabled {
        safe_cap.map(|maximum| maximum.min(REQUESTED_CONTEXT_WINDOW))
    } else {
        default_context_window
    };
    let auto_compact_cap = effective_context_window.map(|window| window.saturating_mul(90) / 100);
    let official_auto_compact = positive_u64(official_model.get("auto_compact_token_limit"));
    let auto_compact_token_limit = match (official_auto_compact, auto_compact_cap) {
        (Some(official), Some(cap)) => Some(official.min(cap)),
        (None, Some(cap)) => Some(cap),
        (Some(official), None) => Some(official),
        (None, None) => None,
    };

    CodexContextWindowStatus {
        model: model.to_string(),
        default_context_window,
        max_context_window,
        requested_context_window,
        effective_context_window,
        auto_compact_token_limit,
    }
}

/// Return a Provider clone with mapped Codex catalog entries capped to the
/// official effective context window. `is_official` must be established by the
/// caller; this helper performs no provider classification or I/O.
///
/// Existing model entries and IDs are preserved; this never adds models. Only
/// `contextWindow` is written, so the official `maxContextWindow` capability
/// and all unrelated metadata remain unchanged. External catalog files are
/// outside this Provider projection and are never read or written here.
pub fn provider_with_codex_context_preference(
    provider: &Provider,
    official_models: &[Value],
    is_official: bool,
    enabled: bool,
) -> Provider {
    let mut projected = provider.clone();
    if !is_official || !enabled {
        return projected;
    }

    let catalog_key = if projected.settings_config.get("modelCatalog").is_some() {
        "modelCatalog"
    } else {
        "model_catalog"
    };
    let Some(models) = projected
        .settings_config
        .get_mut(catalog_key)
        .and_then(|catalog| catalog.get_mut("models"))
        .and_then(Value::as_array_mut)
    else {
        return projected;
    };

    for model_entry in models {
        let Some(entry_id) = model_entry
            .get("model")
            .and_then(Value::as_str)
            .or_else(|| model_entry.get("id").and_then(Value::as_str))
        else {
            continue;
        };
        let upstream_id = model_entry
            .get("upstreamModel")
            .or_else(|| model_entry.get("upstream_model"))
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty());
        let official = match upstream_id {
            Some(upstream_id) => find_official_model(official_models, upstream_id),
            None => find_official_model(official_models, entry_id),
        };
        let Some(official) = official else {
            continue;
        };
        let status = codex_context_window_status(entry_id, official, true);
        if let Some(effective) = status.effective_context_window {
            if let Some(entry) = model_entry.as_object_mut() {
                entry.insert("contextWindow".to_string(), json!(effective));
            }
        }
    }

    projected
}

fn find_official_model<'a>(official_models: &'a [Value], model: &str) -> Option<&'a Value> {
    let normalized = normalize_model_id(model)?;
    official_models.iter().find(|entry| {
        ["slug", "model", "id"].iter().any(|key| {
            entry
                .get(*key)
                .and_then(Value::as_str)
                .and_then(normalize_model_id)
                .is_some_and(|candidate| candidate == normalized)
        })
    })
}

fn normalize_model_id(model: &str) -> Option<String> {
    let normalized = model.trim().to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

fn positive_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(number)) => number.as_u64().filter(|value| *value > 0),
        Some(Value::String(value)) => value.trim().parse::<u64>().ok().filter(|value| *value > 0),
        _ => None,
    }
}

/// Set the native official Codex context preference to one million tokens,
/// recording the original TOML item in a same-line marker for exact restoration.
pub fn apply_native_context_preference(config: &str) -> Result<String, crate::error::AppError> {
    let restored = restore_native_context_preference(config)?;
    let mut document = parse_native_config(&restored)?;
    let original = document
        .get("model_context_window")
        .map(ToString::to_string);
    let payload = serde_json::to_string(&original)
        .map_err(|error| crate::error::AppError::Config(error.to_string()))?;

    document["model_context_window"] = toml_value(REQUESTED_CONTEXT_WINDOW as i64);
    let item = document["model_context_window"]
        .as_value_mut()
        .ok_or_else(|| {
            crate::error::AppError::Config("model_context_window is not a TOML value".into())
        })?;
    item.decor_mut()
        .set_suffix(format!(" # {NATIVE_CONTEXT_MARKER}{payload}"));
    Ok(document.to_string())
}

/// Restore the native TOML item recorded by [`apply_native_context_preference`].
/// A user-edited value is preserved while only the marker is removed.
pub fn restore_native_context_preference(config: &str) -> Result<String, crate::error::AppError> {
    let mut document = parse_native_config(config)?;
    let Some(item) = document.get("model_context_window") else {
        return Ok(document.to_string());
    };
    let suffix = item
        .as_value()
        .and_then(|value| value.decor().suffix())
        .and_then(|suffix| suffix.as_str())
        .unwrap_or_default();
    let Some(marker_offset) = suffix.find(NATIVE_CONTEXT_MARKER) else {
        if suffix.contains(NATIVE_CONTEXT_MARKER_FAMILY) {
            return Err(crate::error::AppError::Config(
                "unrecognized native context preference marker".into(),
            ));
        }
        return Ok(document.to_string());
    };
    let payload = suffix[marker_offset + NATIVE_CONTEXT_MARKER.len()..].trim();
    let original: Option<String> = serde_json::from_str(payload).map_err(|_| {
        crate::error::AppError::Config("invalid native context preference marker".into())
    })?;

    let is_requested_value = item
        .as_value()
        .and_then(toml_edit::Value::as_integer)
        .is_some_and(|value| value == REQUESTED_CONTEXT_WINDOW as i64);
    if is_requested_value {
        if let Some(original) = original {
            let wrapped = format!("value ={original}");
            let mut parsed = parse_native_config(&wrapped)?;
            let original_item = parsed
                .remove("value")
                .ok_or_else(|| crate::error::AppError::Config("invalid saved TOML item".into()))?;
            document["model_context_window"] = original_item;
        } else {
            document.remove("model_context_window");
        }
    } else {
        let value = document["model_context_window"]
            .as_value_mut()
            .ok_or_else(|| {
                crate::error::AppError::Config("model_context_window is not a TOML value".into())
            })?;
        value.decor_mut().set_suffix("");
    }
    Ok(document.to_string())
}

fn parse_native_config(config: &str) -> Result<DocumentMut, crate::error::AppError> {
    config
        .parse::<DocumentMut>()
        .map_err(|error| crate::error::AppError::Config(format!("invalid Codex TOML: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(settings_config: Value) -> Provider {
        Provider::with_id(
            "official-row".into(),
            "Official".into(),
            settings_config,
            None,
        )
    }

    #[test]
    fn clamps_requested_window_to_declared_official_max_and_auto_compact() {
        let official = json!({
            "slug": "gpt-5.6-sol",
            "context_window": 272000,
            "max_context_window": 872000,
            "auto_compact_token_limit": 850000
        });
        let status = codex_context_window_status("gpt-5.6-sol", &official, true);

        assert_eq!(
            status.requested_context_window,
            Some(REQUESTED_CONTEXT_WINDOW)
        );
        assert_eq!(status.default_context_window, Some(272_000));
        assert_eq!(status.max_context_window, Some(872_000));
        assert_eq!(status.effective_context_window, Some(872_000));
        assert_eq!(status.auto_compact_token_limit, Some(784_800));
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            json!({
                "model":"gpt-5.6-sol",
                "defaultContextWindow":272000,
                "maxContextWindow":872000,
                "requestedContextWindow":1000000,
                "effectiveContextWindow":872000,
                "autoCompactTokenLimit":784800
            })
        );
    }

    #[test]
    fn caps_to_default_when_official_max_is_missing_and_does_not_invent_capacity() {
        let official = json!({"slug":"gpt-5.4","context_window":272000});
        let status = codex_context_window_status("gpt-5.4", &official, true);

        assert_eq!(status.max_context_window, None);
        assert_eq!(status.effective_context_window, Some(272_000));
        assert_ne!(
            status.effective_context_window,
            Some(REQUESTED_CONTEXT_WINDOW)
        );
        assert_eq!(status.auto_compact_token_limit, Some(244_800));
    }

    #[test]
    fn requests_one_million_but_respects_real_official_max_above_one_million() {
        let official = json!({
            "slug":"gpt-5.6-sol",
            "context_window":272000,
            "max_context_window":1050000
        });
        let status = codex_context_window_status("gpt-5.6-sol", &official, true);

        assert_eq!(status.max_context_window, Some(1_050_000));
        assert_eq!(status.requested_context_window, Some(1_000_000));
        assert_eq!(status.effective_context_window, Some(1_000_000));
        assert_eq!(status.auto_compact_token_limit, Some(900_000));
    }

    #[test]
    fn disabled_status_uses_declared_default_and_official_auto_compact_limit() {
        let official = json!({
            "context_window":272000,
            "max_context_window":872000,
            "auto_compact_token_limit":200000
        });
        let status = codex_context_window_status("gpt-5.6-sol", &official, false);

        assert_eq!(status.requested_context_window, None);
        assert_eq!(status.effective_context_window, Some(272_000));
        assert_eq!(status.auto_compact_token_limit, Some(200_000));
    }

    #[test]
    fn maps_visible_and_upstream_ids_without_rewriting_ids_or_max_capabilities() {
        let mut provider = provider(json!({"modelCatalog":{"models":[
            {"model":" GPT-5.6-SOL-visible ","upstreamModel":"gpt-5.6-sol","contextWindow":272000,"maxContextWindow":872000,"label":"keep"},
            {"model":"gpt-5.4","contextWindow":272000,"custom":{"keep":true}},
            {"model":"private-model","contextWindow":12345}
        ]}}));
        let official = vec![json!({
            "slug":"gpt-5.6-sol","context_window":272000,"max_context_window":872000
        })];

        let projected = provider_with_codex_context_preference(&provider, &official, true, true);
        let models = projected.settings_config["modelCatalog"]["models"]
            .as_array()
            .unwrap();
        assert_eq!(models[0]["model"], " GPT-5.6-SOL-visible ");
        assert_eq!(models[0]["contextWindow"], 872000);
        assert_eq!(models[0]["maxContextWindow"], 872000);
        assert_eq!(models[0]["label"], "keep");
        assert_eq!(models[1]["contextWindow"], 272000);
        assert_eq!(models[1]["custom"]["keep"], true);
        assert_eq!(models[2]["contextWindow"], 12345);

        provider.settings_config["modelCatalog"]["models"][0]["contextWindow"] = json!(500);
        assert_eq!(
            projected.settings_config["modelCatalog"]["models"][0]["contextWindow"],
            872000
        );
    }

    #[test]
    fn upstream_model_mapping_takes_precedence_and_snake_case_catalog_is_supported() {
        let provider = provider(json!({"model_catalog":{"models":[
            {"model":"gpt-5.6-sol","upstreamModel":"vendor-gpt","contextWindow":128000},
            {"model":"visible-other","upstreamModel":"gpt-5.6-sol","contextWindow":128000},
            {"model":"visible-snake","upstream_model":"gpt-5.6-sol","contextWindow":128000},
            {"model":"gpt-5.6-sol","contextWindow":128000}
        ]}}));
        let official = vec![json!({
            "slug":"gpt-5.6-sol","context_window":272000,"max_context_window":872000
        })];
        let projected = provider_with_codex_context_preference(&provider, &official, true, true);
        let models = projected.settings_config["model_catalog"]["models"]
            .as_array()
            .unwrap();

        // An explicit upstream mapping prevents a coincidental visible-ID match.
        assert_eq!(models[0]["contextWindow"], 128000);
        assert_eq!(models[1]["contextWindow"], 872000);
        assert_eq!(models[2]["contextWindow"], 872000);
        // Without upstreamModel, the public model ID is the exact-match key.
        assert_eq!(models[3]["contextWindow"], 872000);
    }

    #[test]
    fn disabled_or_unverified_provider_is_returned_unchanged() {
        let original = provider(json!({
            "modelCatalog":{"models":[{"model":"gpt-5.6-sol","contextWindow":272000}]},
            "model_catalog_json":"C:/user/catalog.json"
        }));
        let official = vec![json!({"slug":"gpt-5.6-sol","context_window":872000})];

        assert_eq!(
            provider_with_codex_context_preference(&original, &official, true, false)
                .settings_config,
            original.settings_config
        );
        assert_eq!(
            provider_with_codex_context_preference(&original, &official, false, true)
                .settings_config,
            original.settings_config
        );
        assert_eq!(
            provider_with_codex_context_preference(&original, &official, true, true)
                .settings_config["model_catalog_json"],
            original.settings_config["model_catalog_json"]
        );
    }

    #[test]
    fn does_not_add_models_without_existing_catalog_entries() {
        let provider = provider(json!({"config":{"model":"gpt-5.6-sol"}}));
        let official = vec![json!({"slug":"gpt-5.6-sol","context_window":872000})];
        let projected = provider_with_codex_context_preference(&provider, &official, true, true);

        assert_eq!(projected.settings_config, provider.settings_config);
    }

    #[test]
    fn native_context_marker_restores_original_value_and_comment_idempotently() {
        let original = "model_context_window = 272_000 # user setting\nmodel = \"gpt-5.6-sol\"\n";
        let applied = apply_native_context_preference(original).unwrap();
        assert!(applied.contains("model_context_window = 1000000"));
        assert!(applied.contains(NATIVE_CONTEXT_MARKER));
        assert_eq!(apply_native_context_preference(&applied).unwrap(), applied);

        let restored = restore_native_context_preference(&applied).unwrap();
        let parsed = parse_native_config(&restored).unwrap();
        assert_eq!(parsed["model_context_window"].as_integer(), Some(272_000));
        assert!(restored.contains("user setting"));
        assert!(!restored.contains(NATIVE_CONTEXT_MARKER));
    }

    #[test]
    fn native_context_marker_removes_previously_absent_value() {
        let original = "model = \"gpt-5.6-sol\"\n";
        let applied = apply_native_context_preference(original).unwrap();
        let restored = restore_native_context_preference(&applied).unwrap();
        assert_eq!(
            parse_native_config(&restored).unwrap()["model"].as_str(),
            Some("gpt-5.6-sol")
        );
        assert!(parse_native_config(&restored)
            .unwrap()
            .get("model_context_window")
            .is_none());
    }

    #[test]
    fn native_context_restore_preserves_a_user_edited_numeric_value() {
        let applied = apply_native_context_preference("model_context_window = 272000\n").unwrap();
        let edited = applied.replacen(
            "model_context_window = 1000000",
            "model_context_window = 640000",
            1,
        );
        let restored = restore_native_context_preference(&edited).unwrap();
        let parsed = parse_native_config(&restored).unwrap();

        assert_eq!(parsed["model_context_window"].as_integer(), Some(640_000));
        assert!(!restored.contains(NATIVE_CONTEXT_MARKER));
    }

    #[test]
    fn malformed_or_unknown_native_context_markers_fail_closed() {
        let malformed = "model_context_window = 1000000 # CCSM_OFFICIAL_CONTEXT_1M_V1:not-json\n";
        let unknown = "model_context_window = 1000000 # CCSM_OFFICIAL_CONTEXT_2M_V1:null\n";

        assert!(restore_native_context_preference(malformed).is_err());
        assert!(restore_native_context_preference(unknown).is_err());
        assert!(apply_native_context_preference(malformed).is_err());
    }
}
