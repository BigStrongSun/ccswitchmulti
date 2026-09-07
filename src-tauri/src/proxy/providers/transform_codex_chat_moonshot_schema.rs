//! Moonshot / Kimi Chat Completions schema compatibility for the Codex bridge.
//!
//! Their Chat validator rejects a JSON Schema node that carries `$ref` beside
//! sibling constraints. Moving that reference into `allOf` preserves the
//! conjunction while keeping every sibling in place. The rewrite is narrowly
//! gated by the resolved upstream DNS host and only walks schema-valued JSON
//! Schema keywords, so literal `$ref` data is never modified.

use serde_json::{json, Map, Value};
use url::Url;

const MOONSHOT_HOST_SUFFIXES: &[&str] = &["moonshot.cn", "moonshot.ai", "kimi.com"];

const SINGLE_SCHEMA_KEYWORDS: &[&str] = &[
    "items",
    "additionalItems",
    "unevaluatedItems",
    "contains",
    "additionalProperties",
    "unevaluatedProperties",
    "propertyNames",
    "not",
    "if",
    "then",
    "else",
    "contentSchema",
];

const SCHEMA_ARRAY_KEYWORDS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

const SCHEMA_MAP_KEYWORDS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
    "dependencies",
];

/// Match the host itself or one of its subdomains. Invalid URLs fail closed.
pub fn upstream_requires_ref_sibling_all_of(base_url: &str) -> bool {
    let Ok(url) = Url::parse(base_url.trim()) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    MOONSHOT_HOST_SUFFIXES.iter().any(|suffix| {
        host == *suffix
            || host
                .strip_suffix(suffix)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

/// Rewrite only `tools[].function.parameters` and return the changed-tool count.
pub fn wrap_ref_siblings_in_chat_tools(chat_body: &mut Value) -> usize {
    let Some(tools) = chat_body.get_mut("tools").and_then(Value::as_array_mut) else {
        return 0;
    };
    let mut changed = 0;
    for tool in tools {
        let Some(parameters) = tool
            .get_mut("function")
            .and_then(|function| function.get_mut("parameters"))
        else {
            continue;
        };
        if wrap_ref_siblings(parameters) > 0 {
            changed += 1;
        }
    }
    changed
}

/// Move `$ref` siblings into `allOf`, traversing schema-valued keywords only.
pub fn wrap_ref_siblings(schema: &mut Value) -> usize {
    let Value::Object(map) = schema else {
        return 0;
    };
    let mut rewritten = 0;
    if map.len() > 1 && map.get("$ref").is_some_and(Value::is_string) {
        move_ref_into_all_of(map);
        rewritten += 1;
    }
    for (key, child) in map.iter_mut() {
        let key = key.as_str();
        if SCHEMA_MAP_KEYWORDS.contains(&key) {
            if let Value::Object(entries) = child {
                rewritten += entries.values_mut().map(wrap_ref_siblings).sum::<usize>();
            }
        } else if SCHEMA_ARRAY_KEYWORDS.contains(&key) {
            if let Value::Array(entries) = child {
                rewritten += entries.iter_mut().map(wrap_ref_siblings).sum::<usize>();
            }
        } else if SINGLE_SCHEMA_KEYWORDS.contains(&key) {
            match child {
                Value::Array(entries) => {
                    rewritten += entries.iter_mut().map(wrap_ref_siblings).sum::<usize>();
                }
                other => rewritten += wrap_ref_siblings(other),
            }
        }
    }
    rewritten
}

fn move_ref_into_all_of(map: &mut Map<String, Value>) {
    let Some(reference) = map.remove("$ref") else {
        return;
    };
    let branch = json!({ "$ref": reference });
    match map.get_mut("allOf") {
        Some(Value::Array(branches)) => branches.push(branch),
        _ => {
            map.insert("allOf".to_string(), Value::Array(vec![branch]));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn desktop_like_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": { "$ref": "#/$defs/Prompt", "description": "Prompt to run" },
                "literal": {
                    "default": { "$ref": "data", "note": "must stay data" }
                }
            },
            "$defs": {
                "Prompt": { "$ref": "#/$defs/Text", "type": "string", "minLength": 1 },
                "Text": { "type": "string" }
            }
        })
    }

    #[test]
    fn upstream_protocol_gate_matches_only_moonshot_and_kimi_dns_boundaries() {
        for url in [
            "https://api.moonshot.cn/v1",
            "https://api.moonshot.ai/v1/",
            "https://api.kimi.com/coding/v1",
        ] {
            assert!(upstream_requires_ref_sibling_all_of(url), "{url}");
        }
        for url in [
            "https://api.openai.com/v1",
            "https://api.kimi.com.evil.net/v1",
            "https://notmoonshot.cn/v1",
            "api.moonshot.cn/v1",
        ] {
            assert!(!upstream_requires_ref_sibling_all_of(url), "{url}");
        }
    }

    #[test]
    fn upstream_protocol_wraps_ref_siblings_without_entering_data_values_and_is_idempotent() {
        let mut schema = desktop_like_schema();
        assert_eq!(wrap_ref_siblings(&mut schema), 2);
        assert_eq!(
            schema["properties"]["prompt"],
            json!({
                "description": "Prompt to run",
                "allOf": [{ "$ref": "#/$defs/Prompt" }]
            })
        );
        assert_eq!(
            schema["$defs"]["Prompt"],
            json!({
                "type": "string",
                "minLength": 1,
                "allOf": [{ "$ref": "#/$defs/Text" }]
            })
        );
        assert_eq!(
            schema["properties"]["literal"]["default"],
            json!({ "$ref": "data", "note": "must stay data" })
        );
        assert_eq!(wrap_ref_siblings(&mut schema), 0);
    }

    #[test]
    fn upstream_protocol_chat_helper_changes_only_function_parameter_schemas() {
        let mut body = json!({
            "tools": [
                {"type": "function", "function": {"name": "run", "parameters": desktop_like_schema()}},
                {"type": "web_search", "$ref": "literal", "description": "unchanged"}
            ]
        });
        assert_eq!(wrap_ref_siblings_in_chat_tools(&mut body), 1);
        assert_eq!(body["tools"][1]["$ref"], "literal");
    }
}
