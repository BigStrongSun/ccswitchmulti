//! xAI (Grok) `Responses` request field sanitization for native Responses
//! upstreams.
//!
//! Codex 0.142+ sends `wire_api="responses"` requests carrying a handful of
//! OpenAI-backend-private fields and tool carriers that xAI's strict
//! `api.x.ai/v1/responses` serde parser rejects (HTTP 400/422). cc-switch's
//! Chat/Anthropic transforms already drop these on the way through, but the
//! *native* Responses passthrough forwards the body verbatim, so we scrub them
//! here.
//!
//! This is a faithful port of sub2api's `patchGrokResponsesBody`
//! (`backend/internal/service/openai_gateway_grok.go`), the production Go
//! gateway that routes Codex → Grok subscriptions. The compatibility layer is
//! deterministic and idempotent: besides private-field removal and structural
//! lifts, it projects unsupported namespace/custom tools onto xAI function-wire
//! shapes and restores the original Codex semantics on the response path. The
//! same input therefore still yields the same upstream body and a stable prompt
//! cache prefix. It is gated by
//! [`super::codex::provider_needs_responses_namespace_flatten`], so no unrelated
//! provider is touched.
//!
//! Run this *after* namespace flattening: by then Codex's `namespace` tools are
//! already lifted to top-level `function` tools, so the tool-type whitelist
//! below keeps them instead of dropping them.

use std::collections::{HashMap, HashSet};

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::Value;

use crate::proxy::sse::{append_utf8_safe, strip_sse_field, take_sse_block};

/// Codex plugin-private fields removed recursively at any nesting depth.
const RECURSIVE_UNSUPPORTED_FIELDS: &[&str] = &["external_web_access"];

/// Top-level request fields xAI rejects regardless of model.
const TOP_LEVEL_UNSUPPORTED_FIELDS: &[&str] = &["prompt_cache_retention", "safety_identifier"];

/// Top-level sampling fields rejected specifically by grok-4.5.
const GROK_45_UNSUPPORTED_FIELDS: &[&str] = &[
    "presence_penalty",
    "presencePenalty",
    "frequency_penalty",
    "frequencyPenalty",
    "stop",
];

const CUSTOM_TOOL_INPUT_FIELD: &str = "input";
const CUSTOM_TOOL_INPUT_DESCRIPTION: &str = "Raw string input for the original custom tool. Preserve formatting exactly and follow the original tool definition embedded in the description.";
const CUSTOM_TOOL_PRESERVED_METADATA_HEADING: &str = "Original custom tool definition:";

/// Tool `type` values xAI's Responses schema accepts. Sourced from xAI's own
/// serde error enumeration (which is more complete than sub2api's hand-copied
/// list — it includes `image_generation`). Any other `type` is a Codex/OpenAI
/// private carrier (`tool_search`, a stray `namespace`, `custom`, …) that the
/// strict parser would reject, so it is dropped.
const XAI_SUPPORTED_TOOL_TYPES: &[&str] = &[
    "function",
    "web_search",
    "x_search",
    "image_generation",
    "collections_search",
    "file_search",
    "code_execution",
    "code_interpreter",
    "mcp",
    "shell",
];

/// Strip xAI-unsupported fields and tools from a native Codex Responses request
/// body in place. Returns whether anything changed. Deterministic and
/// idempotent: running it twice on the same body changes nothing the second
/// time.
pub(crate) fn sanitize_xai_responses_request(body: &mut Value) -> bool {
    if !body.is_object() {
        return false;
    }

    let mut changed = false;

    // 1. Top-level fields xAI rejects for every model.
    for field in TOP_LEVEL_UNSUPPORTED_FIELDS {
        changed |= remove_top_level_field(body, field);
    }

    // 2. grok-4.5 additionally rejects these sampling knobs.
    if request_targets_grok_45(body) {
        for field in GROK_45_UNSUPPORTED_FIELDS {
            changed |= remove_top_level_field(body, field);
        }
    }

    // 3. Codex plugin-private flags buried at any depth (e.g. inside tools or
    //    tool parameter schemas).
    for field in RECURSIVE_UNSUPPORTED_FIELDS {
        changed |= remove_field_recursive(body, field);
    }

    // 4. Lift the `additional_tools` input carrier (Responses Lite private
    //    shape) up to top-level `tools` so the supported ones survive.
    changed |= promote_additional_tools(body);

    // 5. xAI has no Responses `custom` tool variant. Preserve the capability
    //    by projecting declarations and replayed custom calls onto its accepted
    //    function-call wire shape. The response path performs the inverse.
    changed |= project_custom_tools_to_function_wire(body);

    // 6. Drop `content: null` on reasoning input items — xAI's untagged enum
    //    deserializer refuses a present-but-null content field.
    changed |= strip_null_reasoning_content(body);

    // 7. Whitelist the tool types and clean a now-dangling `tool_choice`.
    changed |= filter_unsupported_tools(body);

    // 8. xAI requires a plain object at the root of every function schema.
    //    Codex dynamic tools legitimately use root oneOf/anyOf unions, often
    //    with a null branch. Project every such function schema generically;
    //    do not special-case automation_update or any model/tool name.
    changed |= project_xai_function_root_unions(body);

    changed
}

/// Whether the request's (possibly provider-prefixed) model resolves to
/// grok-4.5. Mirrors sub2api's suffix match: `foo/grok-4.5` counts.
fn request_targets_grok_45(body: &Value) -> bool {
    let Some(model) = body.get("model").and_then(Value::as_str) else {
        return false;
    };
    let mut model = model.trim();
    if let Some(idx) = model.rfind('/') {
        model = model[idx + 1..].trim();
    }
    model.eq_ignore_ascii_case("grok-4.5")
}

fn remove_top_level_field(body: &mut Value, field: &str) -> bool {
    body.as_object_mut()
        .and_then(|obj| obj.remove(field))
        .is_some()
}

/// Delete every occurrence of `field` in the tree, at any depth.
fn remove_field_recursive(value: &mut Value, field: &str) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = map.remove(field).is_some();
            for child in map.values_mut() {
                changed |= remove_field_recursive(child, field);
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for child in items.iter_mut() {
                changed |= remove_field_recursive(child, field);
            }
            changed
        }
        _ => false,
    }
}

fn is_additional_tools_item(item: &Value) -> bool {
    item.get("type").and_then(Value::as_str).map(str::trim) == Some("additional_tools")
}

/// Promote any `additional_tools` carrier items from `input` into top-level
/// `tools`, preserving top-level order and appending carrier tools in order,
/// de-duplicated. The carrier items themselves are removed from `input`.
fn promote_additional_tools(body: &mut Value) -> bool {
    // Clone `input` up front so the later mutable write-back to `body` doesn't
    // collide with the read borrow. Only pays the clone on the rare carrier path.
    let input_items: Vec<Value> = match body.get("input").and_then(Value::as_array) {
        Some(arr) if arr.iter().any(is_additional_tools_item) => arr.clone(),
        _ => return false,
    };

    // Seed merged tools + dedup keys from the existing top-level tools.
    let mut merged: Vec<Value> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            seen.insert(tool_dedup_key(tool));
            merged.push(tool.clone());
        }
    }

    let mut filtered_input: Vec<Value> = Vec::with_capacity(input_items.len());
    let mut promoted = false;
    for item in input_items {
        if is_additional_tools_item(&item) {
            if let Some(carrier_tools) = item.get("tools").and_then(Value::as_array) {
                for tool in carrier_tools {
                    if seen.insert(tool_dedup_key(tool)) {
                        merged.push(tool.clone());
                        promoted = true;
                    }
                }
            }
            continue; // carrier item dropped regardless of dedup outcome
        }
        filtered_input.push(item);
    }

    if let Some(obj) = body.as_object_mut() {
        obj.insert("input".to_string(), Value::Array(filtered_input));
        if promoted {
            obj.insert("tools".to_string(), Value::Array(merged));
        }
    }
    // We reached here only because a carrier existed, so `input` changed.
    true
}

/// Stable dedup key for a tool: `(type, name)`, `(mcp, server_label)`, or the
/// serialized tool as a last resort. Mirrors sub2api's `grokResponsesToolDedupKey`.
fn tool_dedup_key(tool: &Value) -> String {
    let tool_type = tool
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !tool_type.is_empty() {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            let name = name.trim();
            if !name.is_empty() {
                return format!("type:{tool_type}\u{0}name:{name}");
            }
        }
        if tool_type == "mcp" {
            if let Some(label) = tool.get("server_label").and_then(Value::as_str) {
                let label = label.trim();
                if !label.is_empty() {
                    return format!("type:mcp\u{0}server_label:{label}");
                }
            }
        }
    }
    format!("json:{tool}")
}

fn strip_null_reasoning_content(body: &mut Value) -> bool {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for item in input.iter_mut() {
        if item.get("type").and_then(Value::as_str).map(str::trim) != Some("reasoning") {
            continue;
        }
        if let Some(obj) = item.as_object_mut() {
            if matches!(obj.get("content"), Some(Value::Null)) {
                obj.remove("content");
                changed = true;
            }
        }
    }
    changed
}

fn project_custom_tools_to_function_wire(body: &mut Value) -> bool {
    let mut changed = false;

    if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            if tool.get("type").and_then(Value::as_str) != Some("custom") {
                continue;
            }
            let Some(name) = tool
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(ToString::to_string)
            else {
                continue;
            };
            let original = tool.clone();
            let description = projected_custom_tool_description(&original);
            *tool = serde_json::json!({
                "type": "function",
                "name": name,
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        CUSTOM_TOOL_INPUT_FIELD: {
                            "type": "string",
                            "description": CUSTOM_TOOL_INPUT_DESCRIPTION
                        }
                    },
                    "required": [CUSTOM_TOOL_INPUT_FIELD],
                    "additionalProperties": false
                },
                "strict": false
            });
            changed = true;
        }
    }

    if let Some(choice) = body.get_mut("tool_choice") {
        if choice.get("type").and_then(Value::as_str) == Some("custom") {
            if let Some(obj) = choice.as_object_mut() {
                obj.insert("type".to_string(), Value::String("function".to_string()));
                changed = true;
            }
        }
    }

    if let Some(input) = body.get_mut("input") {
        changed |= project_custom_history_value(input);
    }

    changed
}

fn projected_custom_tool_description(tool: &Value) -> String {
    let mut description = tool
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_default();
    if !description.is_empty() {
        description.push_str("\n\n");
    }
    description.push_str(CUSTOM_TOOL_PRESERVED_METADATA_HEADING);
    description.push_str("\n```json\n");
    description.push_str(&crate::proxy::json_canonical::canonical_json_string(tool));
    description.push_str("\n```");
    description
}

fn project_custom_history_value(value: &mut Value) -> bool {
    match value {
        Value::Array(items) => items.iter_mut().fold(false, |changed, item| {
            changed | project_custom_history_value(item)
        }),
        Value::Object(obj) => {
            let item_type = obj.get("type").and_then(Value::as_str);
            if item_type == Some("custom_tool_call") {
                let input = obj.remove("input").unwrap_or(Value::String(String::new()));
                let arguments = crate::proxy::json_canonical::canonical_json_string(
                    &serde_json::json!({CUSTOM_TOOL_INPUT_FIELD: input}),
                );
                obj.insert(
                    "type".to_string(),
                    Value::String("function_call".to_string()),
                );
                obj.insert("arguments".to_string(), Value::String(arguments));
                return true;
            }
            if item_type == Some("custom_tool_call_output") {
                obj.insert(
                    "type".to_string(),
                    Value::String("function_call_output".to_string()),
                );
                return true;
            }
            obj.values_mut().fold(false, |changed, child| {
                changed | project_custom_history_value(child)
            })
        }
        _ => false,
    }
}

/// Keep only whitelisted tool types and drop a `tool_choice` that now points at
/// a removed or unsupported tool.
fn filter_unsupported_tools(body: &mut Value) -> bool {
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return false;
    };
    let original_len = tools.len();
    let filtered: Vec<Value> = tools
        .iter()
        .filter(|tool| {
            let t = tool
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            XAI_SUPPORTED_TOOL_TYPES.contains(&t)
        })
        .cloned()
        .collect();

    let mut changed = false;
    if filtered.len() != original_len {
        if let Some(obj) = body.as_object_mut() {
            if filtered.is_empty() {
                obj.remove("tools");
            } else {
                obj.insert("tools".to_string(), Value::Array(filtered.clone()));
            }
        }
        changed = true;
    }

    if body.get("tool_choice").is_some() && should_drop_tool_choice(body, &filtered) {
        if let Some(obj) = body.as_object_mut() {
            obj.remove("tool_choice");
        }
        changed = true;
    }

    changed
}

fn project_xai_function_root_unions(body: &mut Value) -> bool {
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for tool in tools {
        if tool.get("type").and_then(Value::as_str) != Some("function") {
            continue;
        }
        let Some(tool) = tool.as_object_mut() else {
            continue;
        };
        if tool.contains_key("parameters") {
            changed |= project_schema_holder(tool);
        } else if let Some(function) = tool.get_mut("function").and_then(Value::as_object_mut) {
            changed |= project_schema_holder(function);
        }
    }
    changed
}

fn project_schema_holder(holder: &mut serde_json::Map<String, Value>) -> bool {
    let Some(schema) = holder.remove("parameters") else {
        return false;
    };
    match super::codex_tool_schema::project_root_object_union_schema(schema.clone()) {
        Ok(Some(projected)) => {
            holder.insert("parameters".to_string(), projected);
            holder.insert("strict".to_string(), Value::Bool(false));
            true
        }
        Ok(None) | Err(_) => {
            holder.insert("parameters".to_string(), schema);
            false
        }
    }
}

/// Whether `tool_choice` should be dropped given the surviving `tools`. String
/// choices (`"auto"`, `"none"`, `"required"`) are always kept; object choices
/// are dropped when they reference an unsupported type or a function name that
/// no longer exists.
fn should_drop_tool_choice(body: &Value, tools: &[Value]) -> bool {
    let Some(tool_choice) = body.get("tool_choice") else {
        return false;
    };
    if tools.is_empty() {
        return true;
    }
    let Some(choice) = tool_choice.as_object() else {
        return false; // "auto"/"none"/"required" string choices stay
    };
    let choice_type = choice
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if choice_type.is_empty() {
        return false;
    }
    if !XAI_SUPPORTED_TOOL_TYPES.contains(&choice_type) {
        return true;
    }
    if choice_type == "function" {
        let choice_name = choice
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| {
                choice
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("")
            .trim();
        if choice_name.is_empty() {
            return false;
        }
        let exists = tools.iter().any(|tool| {
            let t = tool
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| {
                    tool.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("")
                .trim();
            t == "function" && name == choice_name
        });
        return !exists;
    }
    false
}

/// Restore the Codex semantics that were projected onto xAI's accepted native
/// Responses wire shape. Namespace tools recover their two-part identity and
/// custom tools recover `custom_tool_call` plus raw `input`.
pub(crate) fn rewrite_xai_native_response_value(
    value: &mut Value,
    tool_context: &super::transform_codex_chat::CodexToolContext,
    namespace_map: &HashMap<String, super::transform_codex_responses_namespace::NamespacedName>,
) -> bool {
    let mut changed = normalize_xai_completed_function_arguments(value);
    changed |= super::transform_codex_responses_namespace::restore_response_namespaces(
        value,
        namespace_map,
    );
    let mut item_id_map = HashMap::new();
    changed |= restore_custom_tool_calls_value(value, tool_context, &mut item_id_map);
    changed |= rewrite_custom_item_id_references(value, &item_id_map);
    changed
}

fn normalize_xai_completed_function_arguments(value: &mut Value) -> bool {
    match value {
        Value::Array(items) => items.iter_mut().fold(false, |changed, item| {
            changed | normalize_xai_completed_function_arguments(item)
        }),
        Value::Object(obj) => {
            let item_type = obj.get("type").and_then(Value::as_str);
            let completed_arguments = item_type == Some("response.function_call_arguments.done")
                || (item_type == Some("function_call")
                    && obj.get("status").and_then(Value::as_str) != Some("in_progress"));
            let mut changed = false;
            if completed_arguments {
                if let Some(arguments) = obj.get_mut("arguments") {
                    changed |= normalize_xai_arguments_string(arguments);
                }
            }
            obj.values_mut().fold(changed, |changed, child| {
                changed | normalize_xai_completed_function_arguments(child)
            })
        }
        _ => false,
    }
}

fn normalize_xai_arguments_string(arguments: &mut Value) -> bool {
    let Some(encoded) = arguments.as_str() else {
        return false;
    };
    let Ok(mut parsed) = serde_json::from_str::<Value>(encoded) else {
        return false;
    };
    if !rewrite_whole_number_floats(&mut parsed) {
        return false;
    }
    let Ok(encoded) = serde_json::to_string(&parsed) else {
        return false;
    };
    *arguments = Value::String(encoded);
    true
}

fn rewrite_whole_number_floats(value: &mut Value) -> bool {
    match value {
        Value::Array(items) => items.iter_mut().fold(false, |changed, item| {
            changed | rewrite_whole_number_floats(item)
        }),
        Value::Object(obj) => obj.values_mut().fold(false, |changed, child| {
            changed | rewrite_whole_number_floats(child)
        }),
        Value::Number(number) if number.is_f64() => {
            let Some(float) = number.as_f64() else {
                return false;
            };
            const MAX_SAFE_JSON_INTEGER: f64 = 9_007_199_254_740_991.0;
            if !float.is_finite() || float.fract() != 0.0 || float.abs() > MAX_SAFE_JSON_INTEGER {
                return false;
            }
            if float >= 0.0 {
                *value = Value::Number(serde_json::Number::from(float as u64));
            } else {
                *value = Value::Number(serde_json::Number::from(float as i64));
            }
            true
        }
        _ => false,
    }
}

fn restore_custom_tool_calls_value(
    value: &mut Value,
    tool_context: &super::transform_codex_chat::CodexToolContext,
    item_id_map: &mut HashMap<String, String>,
) -> bool {
    match value {
        Value::Array(items) => items.iter_mut().fold(false, |changed, item| {
            changed | restore_custom_tool_calls_value(item, tool_context, item_id_map)
        }),
        Value::Object(obj) => {
            let is_custom_function = obj.get("type").and_then(Value::as_str)
                == Some("function_call")
                && obj
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| tool_context.is_custom_tool_chat_name(name));
            let mut changed = false;
            if is_custom_function {
                let arguments = obj
                    .remove("arguments")
                    .unwrap_or_else(|| Value::String(String::new()));
                let input = custom_tool_input_from_xai_arguments(&arguments);
                let original_id = obj
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let item_id = codex_custom_tool_item_id(obj);
                obj.insert(
                    "type".to_string(),
                    Value::String("custom_tool_call".to_string()),
                );
                obj.insert("id".to_string(), Value::String(item_id.clone()));
                obj.insert("input".to_string(), Value::String(input));
                if let Some(original_id) = original_id.filter(|id| id != &item_id) {
                    item_id_map.insert(original_id, item_id);
                }
                changed = true;
            }
            obj.values_mut().fold(changed, |changed, child| {
                changed | restore_custom_tool_calls_value(child, tool_context, item_id_map)
            })
        }
        _ => false,
    }
}

fn custom_tool_input_from_xai_arguments(arguments: &Value) -> String {
    match arguments {
        Value::String(arguments) => {
            super::transform_codex_chat::custom_tool_input_from_chat_arguments(arguments)
        }
        Value::Object(obj) => obj
            .get(CUSTOM_TOOL_INPUT_FIELD)
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| crate::proxy::json_canonical::canonical_json_string(arguments)),
        _ => crate::proxy::json_canonical::canonical_json_string(arguments),
    }
}

fn codex_custom_tool_item_id(item: &serde_json::Map<String, Value>) -> String {
    if let Some(id) = item
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("ctc_"))
    {
        return id.to_string();
    }
    let identity = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or("custom");
    format!("ctc_{identity}")
}

fn rewrite_custom_item_id_references(
    value: &mut Value,
    item_id_map: &HashMap<String, String>,
) -> bool {
    if item_id_map.is_empty() {
        return false;
    }
    match value {
        Value::Array(items) => items.iter_mut().fold(false, |changed, item| {
            changed | rewrite_custom_item_id_references(item, item_id_map)
        }),
        Value::Object(obj) => {
            let mut changed = false;
            if let Some(item_id) = obj.get_mut("item_id") {
                if let Some(replacement) =
                    item_id.as_str().and_then(|id| item_id_map.get(id)).cloned()
                {
                    *item_id = Value::String(replacement);
                    changed = true;
                }
            }
            obj.values_mut().fold(changed, |changed, child| {
                changed | rewrite_custom_item_id_references(child, item_id_map)
            })
        }
        _ => false,
    }
}

#[derive(Debug, Clone, Default)]
struct XaiCustomCallStreamState {
    codex_item_id: String,
    arguments: String,
    emitted_input: String,
}

struct XaiNativeResponsesSseRewriter {
    tool_context: super::transform_codex_chat::CodexToolContext,
    namespace_map: HashMap<String, super::transform_codex_responses_namespace::NamespacedName>,
    custom_calls: HashMap<String, XaiCustomCallStreamState>,
}

impl XaiNativeResponsesSseRewriter {
    fn new(
        tool_context: super::transform_codex_chat::CodexToolContext,
        namespace_map: HashMap<String, super::transform_codex_responses_namespace::NamespacedName>,
    ) -> Self {
        Self {
            tool_context,
            namespace_map,
            custom_calls: HashMap::new(),
        }
    }

    fn rewrite_event(&mut self, mut event: Value) -> (Vec<Value>, bool) {
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_item.added") => {
                self.remember_custom_call_from_event(&event);
            }
            Some("response.function_call_arguments.delta")
                if event
                    .get("item_id")
                    .and_then(Value::as_str)
                    .is_some_and(|item_id| self.custom_calls.contains_key(item_id)) =>
            {
                return self.rewrite_custom_arguments_delta(event);
            }
            Some("response.function_call_arguments.done")
                if event
                    .get("item_id")
                    .and_then(Value::as_str)
                    .is_some_and(|item_id| self.custom_calls.contains_key(item_id)) =>
            {
                return self.rewrite_custom_arguments_done(event);
            }
            _ => {}
        }

        let changed =
            rewrite_xai_native_response_value(&mut event, &self.tool_context, &self.namespace_map);
        (vec![event], changed)
    }

    fn remember_custom_call_from_event(&mut self, event: &Value) {
        let Some(item) = event.get("item") else {
            return;
        };
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            return;
        }
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            return;
        };
        if !self.tool_context.is_custom_tool_chat_name(name) {
            return;
        }
        let Some(original_item_id) = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            return;
        };
        let codex_item_id = item
            .as_object()
            .map(codex_custom_tool_item_id)
            .unwrap_or_else(|| format!("ctc_{original_item_id}"));
        let arguments = item
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self.custom_calls.insert(
            original_item_id.to_string(),
            XaiCustomCallStreamState {
                codex_item_id,
                arguments,
                emitted_input: String::new(),
            },
        );
    }

    fn rewrite_custom_arguments_delta(&mut self, mut event: Value) -> (Vec<Value>, bool) {
        let Some(original_item_id) = event
            .get("item_id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
        else {
            return (vec![event], false);
        };
        let Some(state) = self.custom_calls.get_mut(&original_item_id) else {
            return (vec![event], false);
        };
        if let Some(delta) = event.get("delta").and_then(Value::as_str) {
            state.arguments.push_str(delta);
        }
        let Some(decoded) = decode_partial_custom_input(&state.arguments) else {
            return (Vec::new(), true);
        };
        let Some(delta) = decoded
            .strip_prefix(&state.emitted_input)
            .map(ToString::to_string)
        else {
            return (Vec::new(), true);
        };
        if delta.is_empty() {
            return (Vec::new(), true);
        }
        state.emitted_input = decoded;
        event["type"] = Value::String("response.custom_tool_call_input.delta".to_string());
        event["item_id"] = Value::String(state.codex_item_id.clone());
        event["delta"] = Value::String(delta);
        (vec![event], true)
    }

    fn rewrite_custom_arguments_done(&mut self, mut event: Value) -> (Vec<Value>, bool) {
        let Some(original_item_id) = event
            .get("item_id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
        else {
            return (vec![event], false);
        };
        let Some(state) = self.custom_calls.get_mut(&original_item_id) else {
            return (vec![event], false);
        };
        if let Some(arguments) = event.get("arguments").and_then(Value::as_str) {
            state.arguments = arguments.to_string();
        }
        let input = custom_tool_input_from_xai_arguments(&Value::String(state.arguments.clone()));
        let missing_delta = input
            .strip_prefix(&state.emitted_input)
            .filter(|delta| !delta.is_empty())
            .map(ToString::to_string);
        state.emitted_input = input.clone();

        let mut events = Vec::with_capacity(2);
        if let Some(delta) = missing_delta {
            let mut delta_event = serde_json::json!({
                "type": "response.custom_tool_call_input.delta",
                "item_id": state.codex_item_id,
                "delta": delta
            });
            if let Some(output_index) = event.get("output_index").cloned() {
                delta_event["output_index"] = output_index;
            }
            events.push(delta_event);
        }
        if let Some(obj) = event.as_object_mut() {
            obj.insert(
                "type".to_string(),
                Value::String("response.custom_tool_call_input.done".to_string()),
            );
            obj.insert(
                "item_id".to_string(),
                Value::String(state.codex_item_id.clone()),
            );
            obj.remove("arguments");
            obj.insert("input".to_string(), Value::String(input));
        }
        events.push(event);
        (events, true)
    }
}

/// Wrap xAI native Responses SSE so namespaced calls and custom-tool calls are
/// restored before Codex sees them. Custom function arguments are accumulated
/// across arbitrary SSE chunk boundaries and decoded only through the fixed
/// `{\"input\": ...}` projection contract.
pub(crate) fn create_xai_native_responses_sse_stream<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    tool_context: super::transform_codex_chat::CodexToolContext,
    namespace_map: HashMap<String, super::transform_codex_responses_namespace::NamespacedName>,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    E: std::error::Error + Send + 'static,
{
    async_stream::stream! {
        let mut buffer = String::new();
        let mut utf8_remainder: Vec<u8> = Vec::new();
        let mut rewriter = XaiNativeResponsesSseRewriter::new(tool_context, namespace_map);

        tokio::pin!(stream);
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(bytes) => {
                    append_utf8_safe(&mut buffer, &mut utf8_remainder, &bytes);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        if block.trim().is_empty() {
                            continue;
                        }
                        for rewritten in rewrite_xai_native_sse_block(&block, &mut rewriter) {
                            yield Ok(rewritten);
                        }
                    }
                }
                Err(error) => {
                    yield Err(std::io::Error::other(error.to_string()));
                    return;
                }
            }
        }

        if !utf8_remainder.is_empty() {
            buffer.push_str(&String::from_utf8_lossy(&utf8_remainder));
        }
        let tail = std::mem::take(&mut buffer);
        if !tail.trim().is_empty() {
            for rewritten in rewrite_xai_native_sse_block(&tail, &mut rewriter) {
                yield Ok(rewritten);
            }
        }
    }
}

fn rewrite_xai_native_sse_block(
    block: &str,
    rewriter: &mut XaiNativeResponsesSseRewriter,
) -> Vec<Bytes> {
    let mut data_parts = Vec::new();
    for line in block.lines() {
        if let Some(data) = strip_sse_field(line, "data") {
            data_parts.push(data);
        }
    }
    if data_parts.is_empty() {
        return vec![Bytes::from(format!("{block}\n\n"))];
    }
    let data = data_parts.join("\n");
    if data.trim() == "[DONE]" {
        return vec![Bytes::from(format!("{block}\n\n"))];
    }
    let event: Value = match serde_json::from_str(&data) {
        Ok(event) => event,
        Err(_) => return vec![Bytes::from(format!("{block}\n\n"))],
    };
    let (events, changed) = rewriter.rewrite_event(event);
    if !changed {
        return vec![Bytes::from(format!("{block}\n\n"))];
    }
    events
        .into_iter()
        .map(|event| {
            let event_name = event
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("message");
            let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
            Bytes::from(format!("event: {event_name}\ndata: {data}\n\n"))
        })
        .collect()
}

fn decode_partial_custom_input(arguments: &str) -> Option<String> {
    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(arguments) {
        return obj
            .get(CUSTOM_TOOL_INPUT_FIELD)
            .and_then(Value::as_str)
            .map(ToString::to_string);
    }

    let key_start = arguments.find("\"input\"")? + "\"input\"".len();
    let after_key = arguments.get(key_start..)?;
    let colon = after_key.find(':')?;
    let value = after_key.get(colon + 1..)?.trim_start();
    let encoded = value.strip_prefix('"')?;
    Some(decode_partial_json_string(encoded))
}

fn decode_partial_json_string(encoded: &str) -> String {
    let chars: Vec<char> = encoded.chars().collect();
    let mut decoded = String::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            break;
        }
        if ch != '\\' {
            decoded.push(ch);
            index += 1;
            continue;
        }
        if index + 1 >= chars.len() {
            break;
        }
        let escape = chars[index + 1];
        match escape {
            '"' | '\\' | '/' => {
                decoded.push(escape);
                index += 2;
            }
            'b' => {
                decoded.push('\u{0008}');
                index += 2;
            }
            'f' => {
                decoded.push('\u{000c}');
                index += 2;
            }
            'n' => {
                decoded.push('\n');
                index += 2;
            }
            'r' => {
                decoded.push('\r');
                index += 2;
            }
            't' => {
                decoded.push('\t');
                index += 2;
            }
            'u' => {
                if index + 6 > chars.len() {
                    break;
                }
                let hex: String = chars[index + 2..index + 6].iter().collect();
                let Ok(code) = u16::from_str_radix(&hex, 16) else {
                    break;
                };
                if (0xD800..=0xDBFF).contains(&code) {
                    if index + 12 > chars.len()
                        || chars[index + 6] != '\\'
                        || chars[index + 7] != 'u'
                    {
                        break;
                    }
                    let low_hex: String = chars[index + 8..index + 12].iter().collect();
                    let Ok(low) = u16::from_str_radix(&low_hex, 16) else {
                        break;
                    };
                    if !(0xDC00..=0xDFFF).contains(&low) {
                        break;
                    }
                    let scalar =
                        0x10000 + (((code as u32) - 0xD800) << 10) + ((low as u32) - 0xDC00);
                    let Some(ch) = char::from_u32(scalar) else {
                        break;
                    };
                    decoded.push(ch);
                    index += 12;
                    continue;
                }
                if (0xDC00..=0xDFFF).contains(&code) {
                    break;
                }
                let Some(ch) = char::from_u32(code as u32) else {
                    break;
                };
                decoded.push(ch);
                index += 6;
            }
            _ => break,
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::{stream, StreamExt};
    use serde_json::json;

    #[test]
    fn strips_external_web_access_recursively() {
        let mut body = json!({
            "model": "grok-4.5",
            "external_web_access": true,
            "tools": [
                {"type": "function", "name": "f", "external_web_access": true,
                 "parameters": {"type": "object", "q": {"external_web_access": true}}}
            ],
            "metadata": {"external_web_access": false}
        });
        assert!(sanitize_xai_responses_request(&mut body));
        let s = body.to_string();
        assert!(!s.contains("external_web_access"), "left over: {s}");
    }

    #[test]
    fn strips_top_level_unsupported_fields() {
        let mut body = json!({
            "model": "grok-4.5",
            "prompt_cache_retention": "24h",
            "safety_identifier": "abc"
        });
        assert!(sanitize_xai_responses_request(&mut body));
        assert!(body.get("prompt_cache_retention").is_none());
        assert!(body.get("safety_identifier").is_none());
    }

    #[test]
    fn strips_grok_45_only_sampling_fields() {
        let mut body = json!({
            "model": "grok-4.5",
            "presence_penalty": 0.1,
            "frequency_penalty": 0.2,
            "stop": ["x"]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        assert!(body.get("presence_penalty").is_none());
        assert!(body.get("frequency_penalty").is_none());
        assert!(body.get("stop").is_none());
    }

    #[test]
    fn keeps_sampling_fields_for_non_grok_45() {
        let mut body = json!({
            "model": "grok-4-fast",
            "presence_penalty": 0.1,
            "stop": ["x"]
        });
        // No unsupported fields present, so no change and knobs preserved.
        assert!(!sanitize_xai_responses_request(&mut body));
        assert_eq!(body.get("presence_penalty"), Some(&json!(0.1)));
        assert_eq!(body.get("stop"), Some(&json!(["x"])));
    }

    #[test]
    fn matches_grok_45_with_provider_prefix() {
        let mut body = json!({"model": "xai/grok-4.5", "stop": ["x"]});
        assert!(sanitize_xai_responses_request(&mut body));
        assert!(body.get("stop").is_none());
    }

    #[test]
    fn promotes_additional_tools_dedup() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [{"type": "function", "name": "kept"}],
            "input": [
                {"type": "message", "role": "user", "content": "hi"},
                {"type": "additional_tools", "tools": [
                    {"type": "function", "name": "kept"},
                    {"type": "function", "name": "extra"}
                ]}
            ]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        // carrier removed from input
        let input = body.get("input").unwrap().as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert!(input.iter().all(|i| !is_additional_tools_item(i)));
        // extra promoted, kept not duplicated
        let tools = body.get("tools").unwrap().as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t.get("name").and_then(Value::as_str).unwrap())
            .collect();
        assert_eq!(names, vec!["kept", "extra"]);
    }

    #[test]
    fn strips_null_reasoning_content() {
        let mut body = json!({
            "model": "grok-4.5",
            "input": [
                {"type": "reasoning", "content": null, "id": "r1"},
                {"type": "reasoning", "content": [{"text": "keep"}], "id": "r2"}
            ]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        let input = body.get("input").unwrap().as_array().unwrap();
        assert!(input[0].get("content").is_none());
        assert!(input[1].get("content").is_some());
    }

    #[test]
    fn projects_every_xai_function_root_union_with_non_object_branches() {
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [
                {
                    "type": "function",
                    "name": "mcp__codex_app__automation_update",
                    "strict": true,
                    "parameters": {
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "action": {"const": "view"},
                                    "id": {"type": "string"}
                                },
                                "required": ["action", "id"],
                                "additionalProperties": false
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "action": {"const": "create"},
                                    "name": {"type": "string"}
                                },
                                "required": ["action", "name"],
                                "additionalProperties": false
                            },
                            {"type": "null"}
                        ]
                    }
                },
                {
                    "type": "function",
                    "name": "third_party_dynamic_tool",
                    "strict": true,
                    "parameters": {
                        "anyOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "mode": {"enum": ["direct"]},
                                    "destination": {"$ref": "#/$defs/destination"}
                                },
                                "required": ["mode"]
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "mode": {"enum": ["routed"]},
                                    "destination": {"$ref": "#/$defs/destination"}
                                },
                                "required": ["mode", "destination"]
                            },
                            {"type": "null"}
                        ],
                        "$defs": {
                            "destination": {
                                "type": "object",
                                "properties": {"kind": {"type": "string"}}
                            }
                        }
                    }
                },
                {
                    "type": "function",
                    "name": "already_valid",
                    "strict": true,
                    "parameters": {
                        "type": "object",
                        "properties": {"query": {"type": "string"}},
                        "required": ["query"]
                    }
                }
            ]
        });

        assert!(sanitize_xai_responses_request(&mut body));

        let automation = &body["tools"][0];
        assert_eq!(automation["strict"], false);
        assert_eq!(automation["parameters"]["type"], "object");
        assert!(automation["parameters"].get("oneOf").is_none());
        assert_eq!(
            automation["parameters"]["properties"]["action"],
            json!({"anyOf": [{"const": "view"}, {"const": "create"}]})
        );
        assert_eq!(automation["parameters"]["required"], json!(["action"]));
        assert_eq!(automation["parameters"]["additionalProperties"], false);

        let dynamic = &body["tools"][1];
        assert_eq!(dynamic["strict"], false);
        assert_eq!(dynamic["parameters"]["type"], "object");
        assert!(dynamic["parameters"].get("anyOf").is_none());
        assert_eq!(dynamic["parameters"]["required"], json!(["mode"]));
        assert_eq!(
            dynamic["parameters"]["$defs"]["destination"]["type"],
            "object"
        );

        assert_eq!(body["tools"][2]["strict"], true);
        assert_eq!(body["tools"][2]["parameters"]["type"], "object");
        assert_eq!(body["tools"][2]["parameters"]["required"], json!(["query"]));
    }

    #[test]
    fn projects_custom_tool_declaration_choice_and_history_to_function_wire_shape() {
        let mut body = json!({
            "model": "grok-4.6",
            "tools": [{
                "type": "custom",
                "name": "apply_patch",
                "description": "Apply a patch to files.",
                "format": {"type": "grammar", "syntax": "lark", "definition": "start: PATCH"}
            }],
            "tool_choice": {"type": "custom", "name": "apply_patch"},
            "input": [
                {
                    "type": "custom_tool_call",
                    "id": "ctc_patch",
                    "call_id": "call_patch",
                    "name": "apply_patch",
                    "input": "*** Begin Patch\n*** End Patch"
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_patch",
                    "output": "Done!"
                }
            ]
        });

        assert!(sanitize_xai_responses_request(&mut body));

        let tool = &body["tools"][0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["name"], "apply_patch");
        assert_eq!(tool["parameters"]["type"], "object");
        assert_eq!(tool["parameters"]["properties"]["input"]["type"], "string");
        assert_eq!(tool["parameters"]["required"], json!(["input"]));
        assert!(tool["description"]
            .as_str()
            .expect("projected description")
            .contains("Apply a patch to files."));
        assert!(tool["description"]
            .as_str()
            .expect("projected description")
            .contains("\"syntax\":\"lark\""));
        assert_eq!(
            body["tool_choice"],
            json!({"type": "function", "name": "apply_patch"})
        );
        assert_eq!(body["input"][0]["type"], "function_call");
        assert_eq!(body["input"][0]["name"], "apply_patch");
        assert_eq!(
            serde_json::from_str::<Value>(body["input"][0]["arguments"].as_str().unwrap()).unwrap(),
            json!({"input": "*** Begin Patch\n*** End Patch"})
        );
        assert!(body["input"][0].get("input").is_none());
        assert_eq!(body["input"][1]["type"], "function_call_output");
        assert_eq!(body["input"][1]["call_id"], "call_patch");
        assert_eq!(body["input"][1]["output"], "Done!");
        assert!(!sanitize_xai_responses_request(&mut body));
    }

    #[test]
    fn projects_custom_tools_while_filtering_other_unsupported_types() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [
                {"type": "function", "name": "f"},
                {"type": "tool_search"},
                {"type": "custom", "name": "c"},
                {"type": "mcp", "server_label": "s"}
            ]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        let types: Vec<&str> = body
            .get("tools")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.get("type").and_then(Value::as_str).unwrap())
            .collect();
        assert_eq!(types, vec!["function", "function", "mcp"]);
        assert_eq!(body["tools"][1]["name"], "c");
        assert_eq!(body["tools"][1]["parameters"]["required"], json!(["input"]));
    }

    #[test]
    fn restores_non_streaming_xai_function_call_to_codex_custom_tool_call() {
        let request = json!({
            "model": "grok-4.6",
            "tools": [{"type": "custom", "name": "apply_patch"}],
            "input": "Patch the file."
        });
        let tool_context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let namespace_map =
            super::super::transform_codex_responses_namespace::namespace_restore_map(&request);
        let mut response = json!({
            "id": "resp_1",
            "object": "response",
            "status": "completed",
            "output": [{
                "id": "fc_patch",
                "type": "function_call",
                "status": "completed",
                "call_id": "call_patch",
                "name": "apply_patch",
                "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
            }]
        });

        assert!(rewrite_xai_native_response_value(
            &mut response,
            &tool_context,
            &namespace_map
        ));

        let item = &response["output"][0];
        assert_eq!(item["id"], "ctc_call_patch");
        assert_eq!(item["type"], "custom_tool_call");
        assert_eq!(item["status"], "completed");
        assert_eq!(item["call_id"], "call_patch");
        assert_eq!(item["name"], "apply_patch");
        assert_eq!(item["input"], "*** Begin Patch\n*** End Patch");
        assert!(item.get("arguments").is_none());
    }

    #[test]
    fn normalizes_whole_float_arguments_only_on_completed_function_payloads() {
        let request = json!({
            "model": "grok-4.6",
            "tools": [{
                "type": "function",
                "name": "write_stdin",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "integer"},
                        "yield_time_ms": {"type": "integer"},
                        "ratio": {"type": "number"}
                    }
                }
            }]
        });
        let tool_context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let namespace_map =
            super::super::transform_codex_responses_namespace::namespace_restore_map(&request);
        let mut response = json!({
            "output": [{
                "id": "fc_write",
                "type": "function_call",
                "status": "completed",
                "call_id": "call_write",
                "name": "write_stdin",
                "arguments": "{\"session_id\":92116.0,\"yield_time_ms\":120000.0,\"ratio\":1.5}"
            }]
        });

        assert!(rewrite_xai_native_response_value(
            &mut response,
            &tool_context,
            &namespace_map
        ));
        let arguments: Value =
            serde_json::from_str(response["output"][0]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(arguments["session_id"].as_i64(), Some(92116));
        assert_eq!(arguments["yield_time_ms"].as_u64(), Some(120000));
        assert_eq!(arguments["ratio"].as_f64(), Some(1.5));
        assert!(arguments["ratio"].as_i64().is_none());

        let mut delta = json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_write",
            "delta": "{\"session_id\":92116.0"
        });
        let original_delta = delta.clone();
        assert!(!rewrite_xai_native_response_value(
            &mut delta,
            &tool_context,
            &namespace_map
        ));
        assert_eq!(delta, original_delta);
    }

    #[tokio::test]
    async fn restores_streaming_xai_custom_tool_call_event_lifecycle() {
        let request = json!({
            "model": "grok-4.6",
            "tools": [{"type": "custom", "name": "apply_patch"}],
            "input": "Patch the file."
        });
        let tool_context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let namespace_map =
            super::super::transform_codex_responses_namespace::namespace_restore_map(&request);
        let events = [
            json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "id": "fc_patch",
                    "type": "function_call",
                    "status": "in_progress",
                    "call_id": "call_patch",
                    "name": "apply_patch",
                    "arguments": ""
                }
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_patch",
                "output_index": 0,
                "delta": "{\"input\":\"*** Begin"
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_patch",
                "output_index": 0,
                "delta": " Patch\\n*** End Patch\"}"
            }),
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_patch",
                "output_index": 0,
                "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
            }),
            json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": {
                    "id": "fc_patch",
                    "type": "function_call",
                    "status": "completed",
                    "call_id": "call_patch",
                    "name": "apply_patch",
                    "arguments": "{\"input\":\"*** Begin Patch\\n*** End Patch\"}"
                }
            }),
        ];
        let chunks = events.into_iter().map(|event| {
            Ok::<Bytes, std::io::Error>(Bytes::from(format!(
                "event: {}\ndata: {}\n\n",
                event["type"].as_str().unwrap(),
                serde_json::to_string(&event).unwrap()
            )))
        });

        let output = create_xai_native_responses_sse_stream(
            stream::iter(chunks),
            tool_context,
            namespace_map,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect::<String>();

        assert!(output.contains("event: response.custom_tool_call_input.delta"));
        assert!(output.contains("event: response.custom_tool_call_input.done"));
        assert!(output.contains("\"type\":\"custom_tool_call\""));
        assert!(output.contains("\"id\":\"ctc_call_patch\""));
        assert!(output.contains("\"item_id\":\"ctc_call_patch\""));
        assert!(output.contains("*** Begin Patch\\n*** End Patch"));
        assert!(!output.contains("response.function_call_arguments"));
        assert!(!output.contains("\"type\":\"function_call\""));
    }

    #[tokio::test]
    async fn streaming_custom_tool_input_preserves_split_unicode_surrogate_pairs() {
        let request = json!({
            "model": "grok-4.6",
            "tools": [{"type": "custom", "name": "apply_patch"}],
            "input": "Patch the file."
        });
        let tool_context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let namespace_map =
            super::super::transform_codex_responses_namespace::namespace_restore_map(&request);
        let events = [
            json!({
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "id": "fc_patch_unicode",
                    "type": "function_call",
                    "status": "in_progress",
                    "call_id": "call_patch_unicode",
                    "name": "apply_patch",
                    "arguments": ""
                }
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_patch_unicode",
                "output_index": 0,
                "delta": "{\"input\":\"Deploy \\uD83D"
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_patch_unicode",
                "output_index": 0,
                "delta": "\\uDE80 now"
            }),
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_patch_unicode",
                "output_index": 0,
                "delta": "\"}"
            }),
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_patch_unicode",
                "output_index": 0,
                "arguments": "{\"input\":\"Deploy \\uD83D\\uDE80 now\"}"
            }),
        ];
        let chunks = events.into_iter().map(|event| {
            Ok::<Bytes, std::io::Error>(Bytes::from(format!(
                "event: {}\ndata: {}\n\n",
                event["type"].as_str().unwrap(),
                serde_json::to_string(&event).unwrap()
            )))
        });

        let output = create_xai_native_responses_sse_stream(
            stream::iter(chunks),
            tool_context,
            namespace_map,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect::<String>();

        let deltas = output
            .split("\n\n")
            .filter_map(|block| block.lines().find_map(|line| line.strip_prefix("data: ")))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .filter(|event| event["type"] == "response.custom_tool_call_input.delta")
            .filter_map(|event| event["delta"].as_str().map(ToString::to_string))
            .collect::<String>();
        assert_eq!(deltas, "Deploy 🚀 now");
    }

    #[tokio::test]
    async fn streaming_ordinary_function_done_normalizes_whole_float_arguments() {
        let request = json!({
            "model": "grok-4.6",
            "tools": [{
                "type": "function",
                "name": "write_stdin",
                "parameters": {"type": "object"}
            }]
        });
        let tool_context =
            super::super::transform_codex_chat::build_codex_tool_context_from_request(&request);
        let namespace_map =
            super::super::transform_codex_responses_namespace::namespace_restore_map(&request);
        let events = [
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_write",
                "delta": "{\"session_id\":92116.0"
            }),
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_write",
                "arguments": "{\"session_id\":92116.0,\"ratio\":1.5}"
            }),
        ];
        let chunks = events.into_iter().map(|event| {
            Ok::<Bytes, std::io::Error>(Bytes::from(format!(
                "event: {}\ndata: {}\n\n",
                event["type"].as_str().unwrap(),
                serde_json::to_string(&event).unwrap()
            )))
        });

        let output = create_xai_native_responses_sse_stream(
            stream::iter(chunks),
            tool_context,
            namespace_map,
        )
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect::<String>();

        let events = output
            .split("\n\n")
            .filter_map(|block| block.lines().find_map(|line| line.strip_prefix("data: ")))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .collect::<Vec<_>>();
        let delta = events
            .iter()
            .find(|event| event["type"] == "response.function_call_arguments.delta")
            .expect("ordinary function delta");
        assert_eq!(delta["delta"], "{\"session_id\":92116.0");
        let done = events
            .iter()
            .find(|event| event["type"] == "response.function_call_arguments.done")
            .expect("ordinary function done");
        let arguments: Value =
            serde_json::from_str(done["arguments"].as_str().expect("done arguments")).unwrap();
        assert_eq!(arguments["session_id"].as_i64(), Some(92116));
        assert_eq!(arguments["ratio"].as_f64(), Some(1.5));
        assert!(arguments["ratio"].as_i64().is_none());
    }

    #[test]
    fn drops_dangling_function_tool_choice() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [{"type": "tool_search"}],
            "tool_choice": {"type": "function", "name": "gone"}
        });
        assert!(sanitize_xai_responses_request(&mut body));
        // tool_search filtered → no tools → tool_choice dropped
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn keeps_valid_function_tool_choice() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [{"type": "function", "name": "run"}],
            "tool_choice": {"type": "function", "name": "run"}
        });
        assert!(!sanitize_xai_responses_request(&mut body));
        assert_eq!(
            body.get("tool_choice").unwrap(),
            &json!({"type": "function", "name": "run"})
        );
    }

    #[test]
    fn keeps_string_tool_choice() {
        let mut body = json!({
            "model": "grok-4.5",
            "tools": [{"type": "function", "name": "run"}],
            "tool_choice": "auto"
        });
        assert!(!sanitize_xai_responses_request(&mut body));
        assert_eq!(body.get("tool_choice").unwrap(), &json!("auto"));
    }

    #[test]
    fn noop_on_clean_request() {
        let mut body = json!({
            "model": "grok-4.5",
            "input": [{"type": "message", "role": "user", "content": "hi"}],
            "tools": [{"type": "function", "name": "f"}]
        });
        assert!(!sanitize_xai_responses_request(&mut body));
    }

    #[test]
    fn idempotent_second_pass() {
        let mut body = json!({
            "model": "grok-4.5",
            "external_web_access": true,
            "prompt_cache_retention": "24h",
            "tools": [{"type": "function", "name": "f"}, {"type": "tool_search"}]
        });
        assert!(sanitize_xai_responses_request(&mut body));
        // second pass finds nothing left to change
        assert!(!sanitize_xai_responses_request(&mut body));
    }
}
