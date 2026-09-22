//! Compatibility for gateways that register discovery namespaces request-wide.
//! Keep discovery call/output pairs and qualified function names intact.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use url::Url;

pub(super) fn needs_namespace_consolidation(base_url: &str) -> bool {
    let Ok(url) = Url::parse(base_url) else {
        return false;
    };
    match url.host_str() {
        Some("api.deepseek.com") => true,
        Some("opencode.ai") => {
            let path = url.path().trim_end_matches('/');
            path == "/zen/go/v1" || path == "/zen/go/v1/responses"
        }
        _ => false,
    }
}

// Only inspect protocol tool lists, never arbitrary JSON in arguments or output.
fn visit_tool_lists(body: &mut Value, mut visit: impl FnMut(&mut Vec<Value>)) {
    if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) {
        visit(tools);
    }
    if let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) {
        for item in input {
            if item.get("type").and_then(Value::as_str) == Some("tool_search_output") {
                if let Some(tools) = item.get_mut("tools").and_then(Value::as_array_mut) {
                    visit(tools);
                }
            }
        }
    }
}

fn namespace_name(tool: &Value) -> Option<&str> {
    (tool.get("type")?.as_str()? == "namespace")
        .then(|| tool.get("name")?.as_str())?
        .filter(|name| !name.is_empty())
}

fn function_name(tool: &Value) -> Option<&str> {
    (tool.get("type")?.as_str()? == "function")
        .then(|| tool.get("name")?.as_str())?
        .filter(|name| !name.is_empty())
}

fn merge_namespace(merged: &mut Value, incoming: &Value) -> Option<()> {
    let mut envelope = incoming.clone();
    let incoming_tools = envelope.as_object_mut()?.remove("tools")?;
    let mut previous_envelope = merged.clone();
    previous_envelope.as_object_mut()?.remove("tools");
    if envelope != previous_envelope {
        return None;
    }
    let tools = merged.get_mut("tools")?.as_array_mut()?;
    for tool in incoming_tools.as_array()? {
        let name = function_name(tool)?;
        if let Some(existing) = tools
            .iter_mut()
            .find(|entry| function_name(entry) == Some(name))
        {
            let mut previous_definition = existing.clone();
            let mut incoming_definition = tool.clone();
            previous_definition.as_object_mut()?.remove("description");
            incoming_definition.as_object_mut()?.remove("description");
            if previous_definition != incoming_definition {
                return None;
            }
            // Plugin attribution can change between discoveries without changing
            // the callable contract. Keep the latest description in that case.
            *existing = tool.clone();
        } else {
            tools.push(tool.clone());
        }
    }
    Some(())
}

pub(super) fn consolidate_namespaces(body: &mut Value) {
    let mut groups: HashMap<String, (usize, Option<Value>)> = HashMap::new();
    visit_tool_lists(body, |tools| {
        for tool in tools.iter() {
            let Some(name) = namespace_name(tool) else {
                continue;
            };
            let entry = groups.entry(name.to_owned()).or_insert_with(|| {
                let mut empty = tool.clone();
                empty["tools"] = Value::Array(Vec::new());
                (0, Some(empty))
            });
            entry.0 += 1;
            if let Some(merged) = entry.1.as_mut() {
                if merge_namespace(merged, tool).is_none() {
                    // Do not guess when schemas, metadata, or tool kinds conflict.
                    // Leave this entire namespace untouched for upstream validation.
                    entry.1 = None;
                }
            }
        }
    });
    groups.retain(|_, (count, merged)| *count > 1 && merged.is_some());
    if groups.is_empty() {
        return;
    }
    let mut emitted = HashSet::new();
    visit_tool_lists(body, |tools| {
        tools.retain_mut(|tool| {
            let Some(name) = namespace_name(tool).map(str::to_owned) else {
                return true;
            };
            let Some((_, Some(merged))) = groups.get(&name) else {
                return true;
            };
            if !emitted.insert(name) {
                return false;
            }
            *tool = merged.clone();
            true
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn namespace(name: &str, names: &[&str]) -> Value {
        json!({"type":"namespace", "name":name, "description":"Test tools", "tools":
            names.iter().map(|name| json!({"type":"function", "name":name,
                "parameters":{"type":"object", "properties":{}}, "strict":false})).collect::<Vec<_>>()})
    }

    fn output(id: &str, tools: Vec<Value>) -> Value {
        json!({"type":"tool_search_output", "id":format!("out_{id}"),
            "call_id":id, "status":"completed", "execution":"client", "tools":tools})
    }

    #[test]
    fn repeated_discoveries_union_tools_without_changing_history_links() {
        let mut body = json!({"input":[
            {"type":"tool_search_call", "call_id":"a", "arguments":{"query":"first"}},
            output("a", vec![namespace("mcp__hindsight", &["search"])]),
            {"type":"function_call", "namespace":"mcp__hindsight", "name":"search", "call_id":"f", "arguments":"{}"},
            {"type":"function_call_output", "call_id":"f", "output":"result"},
            {"type":"tool_search_call", "call_id":"b", "arguments":{"query":"second"}},
            output("b", vec![namespace("mcp__hindsight", &["search", "read"])])
        ]});
        let mut expected = body.clone();
        expected["input"][1]["tools"][0] = namespace("mcp__hindsight", &["search", "read"]);
        expected["input"][5]["tools"] = json!([]);
        consolidate_namespaces(&mut body);
        assert_eq!(body, expected);
        consolidate_namespaces(&mut body);
        assert_eq!(body, expected, "normalization must be idempotent");
    }

    #[test]
    fn includes_top_level_tools_and_keeps_unrelated_entries_in_order() {
        let plain = json!({"type":"function", "name":"plain"});
        let other = namespace("other", &["search"]);
        let mut body = json!({"tools":[namespace("mcp__websearch", &["search"]), plain],
            "input":[output("a", vec![namespace("mcp__websearch", &["fetch"]), other.clone()])]});
        consolidate_namespaces(&mut body);
        assert_eq!(
            body["tools"][0],
            namespace("mcp__websearch", &["search", "fetch"])
        );
        assert_eq!(body["tools"][1], plain);
        assert_eq!(body["input"][0]["tools"], json!([other]));
    }

    #[test]
    fn schema_or_metadata_conflicts_leave_entire_group_unchanged() {
        for conflict in [
            json!({"type":"namespace", "name":"n", "description":"changed", "tools":[]}),
            json!({"type":"namespace", "name":"n", "description":"Test tools", "tools":[
                {"type":"function", "name":"search", "parameters":{"type":"string"}}]}),
            json!({"type":"namespace", "name":"n", "description":"Test tools", "tools":[
                {"type":"custom", "name":"custom"}]}),
            json!({"type":"namespace", "name":"n", "description":"Test tools", "tools":null}),
        ] {
            let mut body = json!({"input":[output("a", vec![namespace("n", &["search"])]),
                output("b", vec![namespace("n", &["extra"])]), output("c", vec![conflict])]});
            let original = body.clone();
            consolidate_namespaces(&mut body);
            assert_eq!(body, original);
        }
    }

    #[test]
    fn function_description_changes_keep_latest_text_without_losing_tools() {
        let mut first = namespace("mcp__codex_apps__github", &["get_repo"]);
        first["tools"][0]["description"] = json!("Repository metadata. Plugins: Data, GitHub.");
        let mut second = namespace("mcp__codex_apps__github", &["get_repo", "search"]);
        second["tools"][0]["description"] = json!("Repository metadata. Plugin: GitHub.");
        let mut body =
            json!({"input":[output("a", vec![first]), output("b", vec![second.clone()])]});
        consolidate_namespaces(&mut body);
        assert_eq!(body["input"][0]["tools"], json!([second]));
        assert_eq!(body["input"][1]["tools"], json!([]));
    }

    #[test]
    fn unique_namespaces_and_arbitrary_nested_data_are_untouched() {
        let n = namespace("n", &["search"]);
        for mut body in [
            json!({"input":"hello"}),
            json!({"tools":[n.clone()]}),
            json!({"input":[output("a", vec![n.clone()]),
                {"type":"function_call_output", "output":{"tools":[n.clone()]}},
                {"role":"user", "content":{"type":"tool_search_output", "tools":[n.clone()]}}]}),
            json!({"tools":null, "input":[{"type":"tool_search_output", "tools":null}]}),
        ] {
            let original = body.clone();
            consolidate_namespaces(&mut body);
            assert_eq!(body, original);
        }
    }

    #[test]
    fn consolidation_is_scoped_to_known_native_gateways() {
        for url in [
            "https://api.deepseek.com/v1",
            "https://opencode.ai/zen/go/v1/",
            "https://opencode.ai/zen/go/v1/responses",
        ] {
            assert!(needs_namespace_consolidation(url), "{url}");
        }
        for url in [
            "https://api.openai.com/v1",
            "https://chatgpt.com/backend-api/codex",
            "https://api.x.ai/v1",
            "https://opencode.ai/zen/v1",
            "https://opencode.ai/zen/go/v10",
            "https://opencode.ai.example/zen/go/v1",
            "https://example.com/api.deepseek.com",
            "invalid",
        ] {
            assert!(!needs_namespace_consolidation(url), "{url}");
        }
    }
}
