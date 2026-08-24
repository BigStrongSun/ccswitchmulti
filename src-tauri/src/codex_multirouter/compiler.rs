use crate::codex_multirouter::schema::{
    validate_v2, CodexModelSelection, CodexRouteAuthPolicy, CodexRoutingConfigV2,
    CodexRoutingRouteV2,
};
use crate::proxy::json_canonical::{canonical_json_string, short_sha256_hex};
use crate::Provider;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledCodexRoutingPlan {
    pub routes: Vec<CompiledCodexRoute>,
    pub visible_models: Vec<String>,
    pub model_catalog: Vec<CompiledCodexModel>,
    pub spawn_agent_models: Vec<String>,
    pub dependency_fingerprint: String,
    pub warnings: Vec<CompilerWarning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledCodexRoute {
    pub id: String,
    pub label: Option<String>,
    pub enabled: bool,
    pub target_provider_id: String,
    pub match_prefixes: Vec<String>,
    pub auth_policy: CodexRouteAuthPolicy,
    pub visible_models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompiledCodexModel {
    pub visible_model: String,
    pub canonical_model: String,
    pub upstream_model: String,
    pub display_name: String,
    pub target_provider_id: String,
    pub route_id: String,
    pub api_format: String,
    pub api_format_source: String,
    #[serde(skip)]
    pub sort_index: Option<usize>,
    pub capability_summary: CodexModelCapabilitySummary,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexModelCapabilitySummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_modalities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_cache: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_ultra: Option<Value>,
    pub context_window_source: String,
    pub input_modalities_source: String,
    pub reasoning_source: String,
    pub codex_cache_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilerWarning {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRoutingCompileError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for CodexRoutingCompileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CodexRoutingCompileError {}

/// Compile a schema-v2 Router directly from the current Provider collection.
/// Every non-runtime consumer must use this entry point instead of reading the
/// Router's derived `modelCatalog` snapshot.
pub fn compile_provider_v2(
    router_provider: &Provider,
    providers: &HashMap<String, Provider>,
) -> Result<Option<(CodexRoutingConfigV2, CompiledCodexRoutingPlan)>, CodexRoutingCompileError> {
    let Some(routing) = router_provider.settings_config.get("codexRouting") else {
        return Ok(None);
    };
    let document = crate::codex_multirouter::schema::CodexRoutingDocument::parse(routing).map_err(
        |error| CodexRoutingCompileError {
            code: error.code,
            message: error.message,
        },
    )?;
    let crate::codex_multirouter::schema::CodexRoutingDocument::V2(plan) = document else {
        return Ok(None);
    };
    if !plan.enabled {
        return Ok(None);
    }
    let compiled = compile_v2(&plan, providers)?;
    Ok(Some((plan, compiled)))
}

#[derive(Clone)]
struct ModelCandidate<'a> {
    route_index: usize,
    route_id: &'a str,
    provider: &'a Provider,
    visible_model: String,
    canonical_model: String,
    upstream_model: String,
    model_entry: &'a Value,
}

pub fn compile_v2(
    plan: &CodexRoutingConfigV2,
    providers: &HashMap<String, Provider>,
) -> Result<CompiledCodexRoutingPlan, CodexRoutingCompileError> {
    if let Err(issues) = validate_v2(plan, providers) {
        let issue = issues
            .first()
            .expect("validation returned an empty issue list");
        return Err(CodexRoutingCompileError {
            code: issue.code.clone(),
            message: issue.message.clone(),
        });
    }

    let mut warnings = Vec::new();
    let candidates = collect_candidates(plan, providers, &mut warnings)?;
    let collision_counts = canonical_collision_counts(&candidates);
    let canonical_owners = canonical_owner_counts(&candidates);
    let mut used_visible = HashSet::new();
    let mut model_catalog = Vec::new();
    let mut route_visible_models = vec![Vec::new(); plan.routes.len()];

    for candidate in candidates {
        let route = &plan.routes[candidate.route_index];
        let explicit_aliases = route
            .aliases
            .iter()
            .filter(|(_, target)| {
                target.eq_ignore_ascii_case(&candidate.canonical_model)
                    || target.eq_ignore_ascii_case(&candidate.upstream_model)
            })
            .map(|(alias, _)| alias.trim().to_string())
            .filter(|alias| !alias.is_empty())
            .collect::<Vec<_>>();
        let visible_names = if explicit_aliases.is_empty() {
            vec![automatic_visible_model(
                &candidate,
                collision_counts
                    .get(&candidate.canonical_model.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(1),
                canonical_owners
                    .get(&candidate.canonical_model.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0),
            )]
        } else {
            explicit_aliases
        };

        for requested_visible in visible_names {
            let visible_model = unique_visible_model(
                &requested_visible,
                candidate.provider,
                candidate.route_id,
                &mut used_visible,
                &mut warnings,
            );
            let (api_format, api_format_source) =
                effective_api_format(candidate.provider, candidate.model_entry);
            let capability_summary =
                effective_capability_summary(candidate.provider, candidate.model_entry);
            route_visible_models[candidate.route_index].push(visible_model.clone());
            model_catalog.push(CompiledCodexModel {
                visible_model,
                canonical_model: candidate.canonical_model.clone(),
                upstream_model: candidate.upstream_model.clone(),
                display_name: model_display_name(candidate.model_entry, &candidate.canonical_model),
                target_provider_id: candidate.provider.id.clone(),
                route_id: candidate.route_id.to_string(),
                api_format,
                api_format_source,
                sort_index: model_sort_index(candidate.model_entry),
                capability_summary,
            });
        }
    }

    let routes = plan
        .routes
        .iter()
        .enumerate()
        .map(|(index, route)| CompiledCodexRoute {
            id: route.id.clone(),
            label: route.label.clone().or_else(|| {
                providers
                    .get(&route.target_provider_id)
                    .map(|provider| provider.name.trim().to_string())
                    .filter(|name| !name.is_empty())
            }),
            enabled: route.enabled,
            target_provider_id: route.target_provider_id.clone(),
            match_prefixes: route.match_prefixes.clone(),
            auth_policy: route.auth_policy.clone(),
            visible_models: route_visible_models[index].clone(),
        })
        .collect::<Vec<_>>();
    let visible_models = model_catalog
        .iter()
        .map(|model| model.visible_model.clone())
        .collect::<Vec<_>>();
    let visible_model_set = visible_models.iter().collect::<HashSet<_>>();
    let mut spawn_agent_models = plan
        .spawn_agent_models
        .iter()
        .filter(|model| visible_model_set.contains(model))
        .cloned()
        .collect::<Vec<_>>();
    // spawnAgentModels 只表达用户的优先顺序，不是另一份模型白名单。Provider
    // 新增模型后，在保留有效显式顺序的前提下按当前可路由目录补满 Codex 的前五窗口。
    for model in &visible_models {
        if spawn_agent_models.len() >= 5 {
            break;
        }
        if !spawn_agent_models.contains(model) {
            spawn_agent_models.push(model.clone());
        }
    }
    let dependency_fingerprint = dependency_fingerprint(plan, providers, &model_catalog)?;

    Ok(CompiledCodexRoutingPlan {
        routes,
        visible_models,
        model_catalog,
        spawn_agent_models,
        dependency_fingerprint,
        warnings,
    })
}

fn collect_candidates<'a>(
    plan: &'a CodexRoutingConfigV2,
    providers: &'a HashMap<String, Provider>,
    warnings: &mut Vec<CompilerWarning>,
) -> Result<Vec<ModelCandidate<'a>>, CodexRoutingCompileError> {
    let mut candidates = Vec::new();
    for (route_index, route) in plan.routes.iter().enumerate() {
        if !route.enabled {
            continue;
        }
        let provider =
            providers
                .get(&route.target_provider_id)
                .ok_or_else(|| CodexRoutingCompileError {
                    code: "target_provider_missing".to_string(),
                    message: format!(
                        "route {} targets provider `{}` which does not exist",
                        route_display(route, None),
                        route.target_provider_id,
                    ),
                })?;
        let entries = provider_model_entries(provider);
        let selected = match &route.model_selection {
            CodexModelSelection::All => None,
            CodexModelSelection::Include { models } => Some(
                models
                    .iter()
                    .map(|model| model.trim().to_ascii_lowercase())
                    .collect::<HashSet<_>>(),
            ),
        };
        let mut found = HashSet::new();
        let mut seen_canonical = HashSet::new();
        for model_entry in entries {
            let Some(visible_model) = model_name(model_entry) else {
                continue;
            };
            let canonical_model = visible_model.clone();
            let upstream_model = upstream_model(model_entry, &canonical_model);
            let visible_key = visible_model.to_ascii_lowercase();
            let canonical_key = canonical_model.to_ascii_lowercase();
            let upstream_key = upstream_model.to_ascii_lowercase();
            if selected.as_ref().is_some_and(|models| {
                !models.contains(&canonical_key) && !models.contains(&upstream_key)
            }) {
                continue;
            }
            if !seen_canonical.insert(canonical_key.clone()) {
                continue;
            }
            found.insert(visible_key);
            found.insert(canonical_key);
            found.insert(upstream_key);
            candidates.push(ModelCandidate {
                route_index,
                route_id: &route.id,
                provider,
                visible_model,
                canonical_model,
                upstream_model,
                model_entry,
            });
        }
        if let Some(selected) = selected {
            let mut missing = selected.difference(&found).cloned().collect::<Vec<_>>();
            missing.sort();
            for missing in missing {
                warnings.push(CompilerWarning {
                    code: "selected_model_unavailable".to_string(),
                    route_id: Some(route.id.clone()),
                    message: format!(
                        "route {} keeps unavailable selected model `{missing}` from provider {}; it is excluded from the current projection",
                        route_display(route, Some(provider)),
                        provider_display(provider),
                    ),
                });
            }
        }
        if let Some(missing_target) = route
            .aliases
            .values()
            .map(|target| target.trim())
            .find(|target| !found.contains(&target.to_ascii_lowercase()))
        {
            warnings.push(CompilerWarning {
                code: "alias_target_unavailable".to_string(),
                route_id: Some(route.id.clone()),
                message: format!(
                    "route {} aliases model `{missing_target}` which is not selected from provider {}",
                    route_display(route, Some(provider)),
                    provider_display(provider),
                ),
            });
        }
    }
    Ok(candidates)
}

pub fn compile_v2_strict(
    plan: &CodexRoutingConfigV2,
    providers: &HashMap<String, Provider>,
) -> Result<CompiledCodexRoutingPlan, CodexRoutingCompileError> {
    let compiled = compile_v2(plan, providers)?;
    if let Some(warning) = compiled.warnings.iter().find(|warning| {
        matches!(
            warning.code.as_str(),
            "selected_model_unavailable" | "alias_target_unavailable"
        )
    }) {
        return Err(CodexRoutingCompileError {
            code: match warning.code.as_str() {
                "selected_model_unavailable" => "selected_model_missing",
                _ => "alias_target_missing",
            }
            .to_string(),
            message: warning.message.clone(),
        });
    }
    Ok(compiled)
}

fn route_display(route: &CodexRoutingRouteV2, provider: Option<&Provider>) -> String {
    let label = route
        .label
        .as_deref()
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .or_else(|| {
            provider.and_then(|provider| {
                (!provider.name.trim().is_empty()).then_some(provider.name.trim())
            })
        });
    match label {
        Some(label) if !label.eq_ignore_ascii_case(route.id.trim()) => {
            format!("`{label}` (id `{}`)", route.id)
        }
        _ => format!("`{}`", route.id),
    }
}

fn provider_display(provider: &Provider) -> String {
    match provider.name.trim() {
        name if !name.is_empty() && !name.eq_ignore_ascii_case(provider.id.trim()) => {
            format!("`{name}` (id `{}`)", provider.id)
        }
        _ => format!("`{}`", provider.id),
    }
}

fn provider_model_entries(provider: &Provider) -> Vec<&Value> {
    provider
        .settings_config
        .get("modelCatalog")
        .or_else(|| provider.settings_config.get("model_catalog"))
        .and_then(|catalog| catalog.get("models"))
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter(|model| model.get("enabled").and_then(Value::as_bool) != Some(false))
                .collect()
        })
        .unwrap_or_default()
}

fn model_name(model_entry: &Value) -> Option<String> {
    model_entry
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
}

fn upstream_model(model_entry: &Value, canonical_model: &str) -> String {
    string_field(model_entry, &["upstreamModel", "upstream_model"])
        .unwrap_or(canonical_model)
        .to_string()
}

fn model_display_name(model_entry: &Value, canonical_model: &str) -> String {
    string_field(model_entry, &["displayName", "display_name"])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| canonical_model.to_string())
}

fn model_sort_index(model_entry: &Value) -> Option<usize> {
    value_field(model_entry, &["sortIndex", "sort_index"])
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn canonical_collision_counts(candidates: &[ModelCandidate<'_>]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for candidate in candidates {
        *counts
            .entry(candidate.canonical_model.to_ascii_lowercase())
            .or_insert(0) += 1;
    }
    counts
}

fn canonical_owner_counts(candidates: &[ModelCandidate<'_>]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for candidate in candidates {
        if is_canonical_provider(candidate.provider) {
            *counts
                .entry(candidate.canonical_model.to_ascii_lowercase())
                .or_insert(0) += 1;
        }
    }
    counts
}

fn automatic_visible_model(
    candidate: &ModelCandidate<'_>,
    collision_count: usize,
    canonical_owner_count: usize,
) -> String {
    if collision_count <= 1
        || (canonical_owner_count == 1 && is_canonical_provider(candidate.provider))
    {
        return candidate.visible_model.clone();
    }
    format!(
        "{}-{}",
        candidate.visible_model,
        provider_name_suffix(candidate.provider)
    )
}

fn is_canonical_provider(provider: &Provider) -> bool {
    if provider.id.eq_ignore_ascii_case("codex-official")
        || provider.category.as_deref() == Some("official")
    {
        return true;
    }
    let identity = format!("{} {}", provider.id, provider.name).to_ascii_lowercase();
    if identity.contains("openai") && identity.contains("official") {
        return true;
    }
    let provider_type = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("providerType")
                .or_else(|| provider.settings_config.get("provider_type"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default();
    provider_type.eq_ignore_ascii_case("codex_oauth")
        || provider_connection_url(provider)
            .is_some_and(|url| url.contains("chatgpt.com/backend-api/codex"))
}

fn provider_name_suffix(provider: &Provider) -> String {
    let source = if provider.name.trim().is_empty() {
        provider.id.as_str()
    } else {
        provider.name.as_str()
    };
    let mut suffix = String::new();
    let mut previous_dash = false;
    for character in source.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            suffix.push(character);
            previous_dash = false;
        } else if !previous_dash && !suffix.is_empty() {
            suffix.push('-');
            previous_dash = true;
        }
    }
    while suffix.ends_with('-') {
        suffix.pop();
    }
    if suffix.is_empty() {
        "provider".to_string()
    } else {
        suffix
    }
}

fn unique_visible_model(
    requested: &str,
    provider: &Provider,
    route_id: &str,
    used: &mut HashSet<String>,
    warnings: &mut Vec<CompilerWarning>,
) -> String {
    let requested = requested.trim();
    let requested_key = requested.to_ascii_lowercase();
    if used.insert(requested_key) {
        return requested.to_string();
    }
    let base = format!("{}-{}", requested, provider_name_suffix(provider));
    let mut candidate = base.clone();
    let mut index = 2;
    while !used.insert(candidate.to_ascii_lowercase()) {
        candidate = format!("{base}-{index}");
        index += 1;
    }
    warnings.push(CompilerWarning {
        code: "visible_model_collision_resolved".to_string(),
        route_id: Some(route_id.to_string()),
        message: format!("visible model `{requested}` was renamed to `{candidate}`"),
    });
    candidate
}

fn effective_api_format(provider: &Provider, model_entry: &Value) -> (String, String) {
    if let Some(format) = string_field(model_entry, &["apiFormat", "api_format"]) {
        return (normalize_api_format(format), "provider_model".to_string());
    }
    if let Some(format) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| string_field(&provider.settings_config, &["apiFormat", "api_format"]))
    {
        return (normalize_api_format(format), "provider".to_string());
    }
    ("openai_chat".to_string(), "default".to_string())
}

fn normalize_api_format(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        "openai_chat".to_string()
    } else {
        normalized
    }
}

fn effective_capability_summary(
    provider: &Provider,
    model_entry: &Value,
) -> CodexModelCapabilitySummary {
    let (context_window, context_window_source) = value_with_source(
        u64_field(model_entry, &["contextWindow", "context_window"]),
        u64_field(
            &provider.settings_config,
            &["contextWindow", "context_window", "modelContextWindow"],
        ),
    );
    let (input_modalities, input_modalities_source) = value_with_source(
        model_input_modalities(model_entry),
        string_array_field(
            &provider.settings_config,
            &["inputModalities", "input_modalities"],
        ),
    );
    let provider_reasoning = value_field(
        &provider.settings_config,
        &["reasoning", "codexChatReasoning", "codex_chat_reasoning"],
    )
    .cloned()
    .or_else(|| {
        provider
            .meta
            .as_ref()
            .and_then(|meta| meta.codex_chat_reasoning.as_ref())
            .and_then(|reasoning| serde_json::to_value(reasoning).ok())
    });
    let (reasoning, reasoning_source) = value_with_source(
        value_field(model_entry, &["reasoning"]).cloned(),
        provider_reasoning,
    );
    let provider_cache = value_field(&provider.settings_config, &["codexCache", "codex_cache"])
        .cloned()
        .or_else(|| {
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.codex_cache.as_ref())
                .and_then(|cache| serde_json::to_value(cache).ok())
        });
    let (codex_cache, codex_cache_source) = value_with_source(
        value_field(model_entry, &["codexCache", "codex_cache"]).cloned(),
        provider_cache,
    );

    CodexModelCapabilitySummary {
        context_window,
        input_modalities: input_modalities.unwrap_or_default(),
        reasoning: reasoning.map(|value| sanitize_capability_value(&value)),
        codex_cache: codex_cache.map(|value| sanitize_capability_value(&value)),
        supports_parallel_tool_calls: bool_field(
            model_entry,
            &["supportsParallelToolCalls", "supports_parallel_tool_calls"],
        ),
        base_instructions: string_field(model_entry, &["baseInstructions", "base_instructions"])
            .map(ToString::to_string),
        codex_ultra: value_field(model_entry, &["codexUltra", "codex_ultra"])
            .map(sanitize_capability_value),
        context_window_source,
        input_modalities_source,
        reasoning_source,
        codex_cache_source,
    }
}

fn model_input_modalities(model_entry: &Value) -> Option<Vec<String>> {
    if let Some(modalities) =
        string_array_field(model_entry, &["inputModalities", "input_modalities"])
    {
        return Some(modalities);
    }
    if bool_field(model_entry, &["supportsImage", "supports_image", "vision"]) == Some(true) {
        return Some(vec!["text".to_string(), "image".to_string()]);
    }
    if bool_field(model_entry, &["textOnly", "text_only"]) == Some(true)
        || bool_field(model_entry, &["supportsImage", "supports_image", "vision"]) == Some(false)
    {
        return Some(vec!["text".to_string()]);
    }
    None
}

fn sanitize_capability_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| !capability_key_is_sensitive(key))
                .map(|(key, value)| (key.clone(), sanitize_capability_value(value)))
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.iter().map(sanitize_capability_value).collect())
        }
        _ => value.clone(),
    }
}

fn capability_key_is_sensitive(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "auth" | "authorization" | "cookie" | "credentials" | "headers" | "promptcachekey"
    ) || normalized.ends_with("apikey")
        || normalized.ends_with("password")
        || normalized.ends_with("secret")
        || normalized.ends_with("token")
}

fn value_with_source<T>(model: Option<T>, provider: Option<T>) -> (Option<T>, String) {
    if let Some(value) = model {
        return (Some(value), "provider_model".to_string());
    }
    if let Some(value) = provider {
        return (Some(value), "provider".to_string());
    }
    (None, "unknown".to_string())
}

fn string_field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn value_field<'a>(value: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|name| value.get(*name))
}

fn u64_field(value: &Value, names: &[&str]) -> Option<u64> {
    value_field(value, names)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
}

fn bool_field(value: &Value, names: &[&str]) -> Option<bool> {
    value_field(value, names).and_then(Value::as_bool)
}

fn string_array_field(value: &Value, names: &[&str]) -> Option<Vec<String>> {
    let values = value_field(value, names)?.as_array()?;
    let values = values
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    (!values.is_empty()).then_some(values)
}

fn dependency_fingerprint(
    plan: &CodexRoutingConfigV2,
    providers: &HashMap<String, Provider>,
    model_catalog: &[CompiledCodexModel],
) -> Result<String, CodexRoutingCompileError> {
    let mut provider_dependencies = BTreeMap::new();
    for route in &plan.routes {
        let Some(provider) = providers.get(&route.target_provider_id) else {
            continue;
        };
        provider_dependencies.insert(
            provider.id.clone(),
            json!({
                "id": provider.id,
                "name": provider.name,
                "category": provider.category,
                "connectionUrl": provider_connection_url(provider),
                "apiFormat": provider.meta.as_ref().and_then(|meta| meta.api_format.as_deref())
                    .or_else(|| string_field(&provider.settings_config, &["apiFormat", "api_format"])),
                "authOwner": provider_auth_owner(provider),
            }),
        );
    }
    let safe_plan = json!({
        "schemaVersion": plan.schema_version,
        "enabled": plan.enabled,
        "defaultRouteId": plan.default_route_id,
        "routes": plan.routes,
    });
    let effective_models =
        serde_json::to_value(model_catalog).map_err(|error| CodexRoutingCompileError {
            code: "compiler_serialization_failed".to_string(),
            message: error.to_string(),
        })?;
    let effective_model_order = model_catalog
        .iter()
        .map(|model| {
            json!({
                "model": model.visible_model,
                "targetProviderId": model.target_provider_id,
                "sortIndex": model.sort_index,
            })
        })
        .collect::<Vec<_>>();
    let input = json!({
        "plan": safe_plan,
        "providers": provider_dependencies,
        "effectiveModels": effective_models,
        "effectiveModelOrder": effective_model_order,
    });
    Ok(short_sha256_hex(canonical_json_string(&input).as_bytes()))
}

fn provider_connection_url(provider: &Provider) -> Option<String> {
    string_field(
        &provider.settings_config,
        &["baseUrl", "base_url", "openaiBaseUrl", "openai_base_url"],
    )
    .map(ToString::to_string)
    .or_else(|| {
        provider
            .settings_config
            .get("config")
            .and_then(Value::as_str)
            .and_then(crate::codex_config::extract_codex_base_url)
    })
}

fn provider_auth_owner(provider: &Provider) -> Value {
    let mut owner = Map::new();
    if let Some(binding) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.auth_binding.as_ref())
        .and_then(|binding| serde_json::to_value(binding).ok())
        .and_then(|value| value.as_object().cloned())
    {
        for key in ["source", "authProvider", "accountId"] {
            if let Some(value) = binding.get(key) {
                owner.insert(key.to_string(), value.clone());
            }
        }
    }
    if let Some(provider_type) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
    {
        owner.insert("providerType".to_string(), json!(provider_type));
    }
    Value::Object(owner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_multirouter::schema::{
        CodexModelSelection, CodexRouteAuthPolicy, CodexRoutingConfigV2, CodexRoutingRouteV2,
    };
    use crate::{Provider, ProviderMeta};
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, HashMap};

    fn provider(id: &str, name: &str, api_format: &str, models: Value) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            name.to_string(),
            json!({
                "apiFormat": api_format,
                "apiKey": format!("secret-{id}"),
                "baseUrl": format!("https://{id}.example/v1"),
                "modelCatalog": {"models": models}
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            api_format: Some(api_format.to_string()),
            ..Default::default()
        });
        provider
    }

    fn route(
        id: &str,
        target_provider_id: &str,
        model_selection: CodexModelSelection,
    ) -> CodexRoutingRouteV2 {
        CodexRoutingRouteV2 {
            id: id.to_string(),
            label: Some(id.to_string()),
            enabled: true,
            target_provider_id: target_provider_id.to_string(),
            model_selection,
            match_prefixes: Vec::new(),
            aliases: BTreeMap::new(),
            auth_policy: CodexRouteAuthPolicy::default(),
        }
    }

    fn plan(routes: Vec<CodexRoutingRouteV2>) -> CodexRoutingConfigV2 {
        CodexRoutingConfigV2 {
            schema_version: 2,
            enabled: true,
            default_route_id: routes.first().map(|route| route.id.clone()),
            routes,
            subagent_version: None,
            subagent_v2: None,
            spawn_agent_models: Vec::new(),
            extensions: BTreeMap::new(),
        }
    }

    fn compile(
        plan: &CodexRoutingConfigV2,
        providers: impl IntoIterator<Item = Provider>,
    ) -> CompiledCodexRoutingPlan {
        let providers = providers
            .into_iter()
            .map(|provider| (provider.id.clone(), provider))
            .collect::<HashMap<_, _>>();
        compile_v2(plan, &providers).expect("compile v2")
    }

    #[test]
    fn provider_default_protocol_is_inherited_without_route_snapshot() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.8"}]),
        );
        let compiled = compile(
            &plan(vec![route("router-qwen", "qwen", CodexModelSelection::All)]),
            [provider],
        );

        assert_eq!(compiled.model_catalog[0].api_format, "openai_responses");
        assert_eq!(compiled.model_catalog[0].api_format_source, "provider");
    }

    #[test]
    fn spawn_agent_selection_is_filtered_by_compiled_visible_models() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.8"}, {"model": "qwen3.9"}]),
        );
        let mut plan = plan(vec![route("router-qwen", "qwen", CodexModelSelection::All)]);
        plan.spawn_agent_models = vec!["qwen3.9".to_string(), "removed".to_string()];
        let compiled = compile(&plan, [provider]);

        assert_eq!(compiled.spawn_agent_models, vec!["qwen3.9", "qwen3.8"]);
    }

    #[test]
    fn disabled_provider_models_are_excluded_from_v2_routes_and_spawn_agents() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([
                {"model": "qwen-enabled"},
                {"model": "qwen-disabled", "enabled": false}
            ]),
        );
        let mut plan = plan(vec![route("router-qwen", "qwen", CodexModelSelection::All)]);
        plan.spawn_agent_models = vec!["qwen-disabled".to_string(), "qwen-enabled".to_string()];

        let compiled = compile(&plan, [provider]);

        assert_eq!(compiled.visible_models, vec!["qwen-enabled"]);
        assert_eq!(compiled.routes[0].visible_models, vec!["qwen-enabled"]);
        assert_eq!(compiled.spawn_agent_models, vec!["qwen-enabled"]);
    }

    #[test]
    fn unavailable_include_models_do_not_block_provider_fact_updates() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.9"}]),
        );
        let route = route(
            "router-qwen",
            "qwen",
            CodexModelSelection::Include {
                models: vec!["qwen3.8".to_string(), "qwen3.9".to_string()],
            },
        );

        let compiled = compile(&plan(vec![route]), [provider]);

        assert_eq!(compiled.visible_models, vec!["qwen3.9"]);
        assert!(compiled.warnings.iter().any(|warning| {
            warning.code == "selected_model_unavailable"
                && warning.route_id.as_deref() == Some("router-qwen")
                && warning.message.contains("qwen3.8")
        }));
    }

    #[test]
    fn disabled_routes_do_not_require_stale_provider_dependencies() {
        let mut disabled = route(
            "disabled-qwen",
            "deleted-provider",
            CodexModelSelection::Include {
                models: vec!["deleted-model".to_string()],
            },
        );
        disabled.enabled = false;

        let compiled = compile_v2(&plan(vec![disabled]), &HashMap::new())
            .expect("disabled route must not block the active plan");

        assert!(compiled.visible_models.is_empty());
        assert!(compiled.warnings.is_empty());
    }

    #[test]
    fn model_protocol_overrides_provider_default_per_canonical_model() {
        let provider = provider(
            "mixed",
            "Mixed",
            "openai_chat",
            json!([
                {"model": "chat-model"},
                {"model": "responses-model", "apiFormat": "openai_responses"}
            ]),
        );
        let compiled = compile(
            &plan(vec![route(
                "router-mixed",
                "mixed",
                CodexModelSelection::All,
            )]),
            [provider],
        );
        let formats = compiled
            .model_catalog
            .iter()
            .map(|model| {
                (
                    model.canonical_model.as_str(),
                    model.api_format.as_str(),
                    model.api_format_source.as_str(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            formats,
            vec![
                ("chat-model", "openai_chat", "provider"),
                ("responses-model", "openai_responses", "provider_model"),
            ]
        );
    }

    #[test]
    fn all_auto_includes_new_models_while_include_remains_closed() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen-a"}, {"model": "qwen-b"}]),
        );
        let all = compile(
            &plan(vec![route("all", "qwen", CodexModelSelection::All)]),
            [provider.clone()],
        );
        let included = compile(
            &plan(vec![route(
                "include",
                "qwen",
                CodexModelSelection::Include {
                    models: vec!["qwen-a".to_string()],
                },
            )]),
            [provider],
        );

        assert_eq!(all.visible_models, vec!["qwen-a", "qwen-b"]);
        assert_eq!(included.visible_models, vec!["qwen-a"]);
    }

    #[test]
    fn explicit_aliases_are_preserved_and_collisions_get_stable_provider_aliases() {
        let official = provider(
            "codex-official",
            "OpenAI Official",
            "openai_responses",
            json!([{"model": "shared-model"}]),
        );
        let relay = provider(
            "relay",
            "Qwen Relay",
            "openai_responses",
            json!([{"model": "shared-model"}]),
        );
        let mut relay_route = route("relay-route", "relay", CodexModelSelection::All);
        relay_route
            .aliases
            .insert("my-qwen".to_string(), "shared-model".to_string());
        let compiled = compile(
            &plan(vec![
                route("official-route", "codex-official", CodexModelSelection::All),
                relay_route,
            ]),
            [relay, official],
        );
        let visible = compiled
            .model_catalog
            .iter()
            .map(|model| {
                (
                    model.target_provider_id.as_str(),
                    model.visible_model.as_str(),
                    model.canonical_model.as_str(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            visible,
            vec![
                ("codex-official", "shared-model", "shared-model"),
                ("relay", "my-qwen", "shared-model"),
            ]
        );
    }

    #[test]
    fn aliases_accept_upstream_model_ids_from_provider_catalog() {
        let relay = provider(
            "5626e6b9-33cb-4c3b-8d16-af8176e16209",
            "DeepSeek Relay",
            "openai_responses",
            json!([{
                "model": "deepseek-v4-flash",
                "upstreamModel": "deepseek-v4-flash-0731"
            }]),
        );
        let mut relay_route = route(
            "router-5626e6b9-33cb-4c3b-8d16-af8176e16209",
            &relay.id,
            CodexModelSelection::All,
        );
        relay_route.aliases.insert(
            "deepseek-v4-flash-0731".to_string(),
            "deepseek-v4-flash-0731".to_string(),
        );

        let compiled = compile(&plan(vec![relay_route]), [relay]);

        assert_eq!(
            compiled.model_catalog[0].visible_model,
            "deepseek-v4-flash-0731"
        );
        assert_eq!(
            compiled.model_catalog[0].canonical_model,
            "deepseek-v4-flash"
        );
        assert_eq!(
            compiled.model_catalog[0].upstream_model,
            "deepseek-v4-flash-0731"
        );
    }

    #[test]
    fn include_selection_accepts_alias_target_upstream_model_id() {
        let relay = provider(
            "5626e6b9-33cb-4c3b-8d16-af8176e16209",
            "DeepSeek Relay",
            "openai_responses",
            json!([{
                "model": "deepseek-v4-flash",
                "upstreamModel": "deepseek-v4-flash-0731"
            }]),
        );
        let mut relay_route = route(
            "router-5626e6b9-33cb-4c3b-8d16-af8176e16209",
            &relay.id,
            CodexModelSelection::Include {
                models: vec!["deepseek-v4-flash".to_string()],
            },
        );
        relay_route.aliases.insert(
            "deepseek-v4-flash-0731".to_string(),
            "deepseek-v4-flash-0731".to_string(),
        );

        let compiled = compile(&plan(vec![relay_route]), [relay]);

        assert_eq!(compiled.visible_models, vec!["deepseek-v4-flash-0731"]);
        assert_eq!(
            compiled.model_catalog[0].canonical_model,
            "deepseek-v4-flash"
        );
        assert_eq!(
            compiled.model_catalog[0].upstream_model,
            "deepseek-v4-flash-0731"
        );
    }

    #[test]
    fn persisted_alias_does_not_follow_provider_rename() {
        let relay = provider(
            "relay",
            "Renamed Relay",
            "openai_responses",
            json!([{"model": "shared-model"}]),
        );
        let mut relay_route = route("relay-route", "relay", CodexModelSelection::All);
        relay_route
            .aliases
            .insert("shared-model-relay".to_string(), "shared-model".to_string());

        let compiled = compile(&plan(vec![relay_route]), [relay]);

        assert_eq!(compiled.visible_models, vec!["shared-model-relay"]);
        assert_eq!(compiled.model_catalog[0].canonical_model, "shared-model");
    }

    #[test]
    fn capability_summary_comes_from_the_canonical_model_entry() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{
                "model": "qwen3.8",
                "contextWindow": 262144,
                "inputModalities": ["text", "image"],
                "reasoning": {
                    "schemaVersion": 2,
                    "supportStatus": "confirmed_supported",
                    "controlKind": "graded",
                    "supportedEfforts": ["low", "high"],
                    "defaultEffort": "high",
                    "disableAllowed": false,
                    "upstream": {"format": "string", "parameter": "reasoning_effort"}
                },
                "codexCache": {
                    "cacheMode": "auto_prefix_cache",
                    "supportsPromptCacheKey": false,
                    "usageFields": ["usage.cached_tokens"]
                },
                "supportsParallelToolCalls": true,
                "baseInstructions": "Use the Provider model contract.",
                "codexUltra": {"enabled": true, "providerEffort": "high"}
            }]),
        );
        let compiled = compile(
            &plan(vec![route("router-qwen", "qwen", CodexModelSelection::All)]),
            [provider],
        );
        let summary = &compiled.model_catalog[0].capability_summary;

        assert_eq!(summary.context_window, Some(262_144));
        assert_eq!(summary.input_modalities, vec!["text", "image"]);
        assert_eq!(
            summary
                .reasoning
                .as_ref()
                .and_then(|value| value.get("controlKind")),
            Some(&json!("graded"))
        );
        assert_eq!(
            summary
                .codex_cache
                .as_ref()
                .and_then(|value| value.get("cacheMode")),
            Some(&json!("auto_prefix_cache"))
        );
        assert_eq!(summary.context_window_source, "provider_model");
        assert_eq!(summary.input_modalities_source, "provider_model");
        assert_eq!(summary.reasoning_source, "provider_model");
        assert_eq!(summary.codex_cache_source, "provider_model");
        assert_eq!(summary.supports_parallel_tool_calls, Some(true));
        assert_eq!(
            summary.base_instructions.as_deref(),
            Some("Use the Provider model contract.")
        );
        assert_eq!(
            summary
                .codex_ultra
                .as_ref()
                .and_then(|value| value.get("providerEffort")),
            Some(&json!("high"))
        );
    }

    #[test]
    fn dependency_fingerprint_is_order_independent_and_changes_with_effective_inputs() {
        let first = provider(
            "first",
            "First",
            "openai_chat",
            json!([{"model": "first-model"}]),
        );
        let second = provider(
            "second",
            "Second",
            "openai_responses",
            json!([{"model": "second-model"}]),
        );
        let routing_plan = plan(vec![
            route("first-route", "first", CodexModelSelection::All),
            route("second-route", "second", CodexModelSelection::All),
        ]);
        let forward = compile(&routing_plan, [first.clone(), second.clone()]);
        let reverse = compile(&routing_plan, [second.clone(), first.clone()]);
        assert_eq!(
            forward.dependency_fingerprint,
            reverse.dependency_fingerprint
        );

        let protocol_changed = provider(
            "second",
            "Second",
            "openai_chat",
            json!([{"model": "second-model"}]),
        );
        let protocol_compiled = compile(&routing_plan, [first.clone(), protocol_changed]);
        assert_ne!(
            forward.dependency_fingerprint,
            protocol_compiled.dependency_fingerprint
        );

        let model_changed = provider(
            "second",
            "Second",
            "openai_responses",
            json!([{"model": "second-model", "inputModalities": ["text", "image"]}]),
        );
        let model_compiled = compile(&routing_plan, [first, model_changed]);
        assert_ne!(
            forward.dependency_fingerprint,
            model_compiled.dependency_fingerprint
        );

        let sort_changed = provider(
            "second",
            "Second",
            "openai_responses",
            json!([{"model": "second-model", "sortIndex": 1}]),
        );
        let sort_plan = plan(vec![
            route("first-route", "first", CodexModelSelection::All),
            route("second-route", "second", CodexModelSelection::All),
        ]);
        let sort_compiled = compile(
            &sort_plan,
            [
                provider(
                    "first",
                    "First",
                    "openai_chat",
                    json!([{"model": "first-model"}]),
                ),
                sort_changed,
            ],
        );
        assert_ne!(
            forward.dependency_fingerprint,
            sort_compiled.dependency_fingerprint
        );
    }

    #[test]
    fn dependency_fingerprint_tracks_every_projected_provider_model_field() {
        let plan = plan(vec![route("router-qwen", "qwen", CodexModelSelection::All)]);
        let baseline = compile(
            &plan,
            [provider(
                "qwen",
                "Qwen",
                "openai_responses",
                json!([{"model": "qwen3.8"}]),
            )],
        );
        let changes = [
            json!([{"model": "qwen3.8", "displayName": "Qwen 3.8 Updated"}]),
            json!([{"model": "qwen3.8", "contextWindow": 262144}]),
            json!([{"model": "qwen3.8", "inputModalities": ["text", "image"]}]),
            json!([{
                "model": "qwen3.8",
                "reasoning": {
                    "schemaVersion": 2,
                    "supportStatus": "confirmed_supported",
                    "controlKind": "graded",
                    "supportedEfforts": ["low", "high", "max"],
                    "defaultEffort": "high",
                    "disableAllowed": false,
                    "upstream": {
                        "format": "string",
                        "parameter": "reasoning_effort",
                        "effortMap": {"low": "low", "high": "high", "max": "max"}
                    }
                }
            }]),
            json!([{
                "model": "qwen3.8",
                "codexCache": {"cacheMode": "qwen_context_cache"}
            }]),
            json!([{"model": "qwen3.8", "supportsParallelToolCalls": true}]),
            json!([{"model": "qwen3.8", "baseInstructions": "Updated instructions"}]),
            json!([{
                "model": "qwen3.8",
                "codexUltra": {"enabled": true, "providerEffort": "high"}
            }]),
            json!([{"model": "qwen3.8", "apiFormat": "openai_chat"}]),
            json!([{"model": "qwen3.8", "upstreamModel": "qwen3.8-202608"}]),
            json!([{"model": "qwen3.8", "sortIndex": 4}]),
            json!([{"model": "qwen3.8", "enabled": false}]),
            json!([{"model": "qwen3.8"}, {"model": "qwen3.9"}]),
        ];

        for changed_models in changes {
            let changed = compile(
                &plan,
                [provider("qwen", "Qwen", "openai_responses", changed_models)],
            );
            assert_ne!(
                baseline.dependency_fingerprint, changed.dependency_fingerprint,
                "every projected Provider model change must invalidate the MultiRouter projection"
            );
        }
    }

    #[test]
    fn compiled_and_diagnostic_serialization_never_contains_provider_secrets() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.8"}]),
        );
        let compiled = compile(
            &plan(vec![route("router-qwen", "qwen", CodexModelSelection::All)]),
            [provider],
        );
        let serialized = serde_json::to_string(&compiled).expect("serialize compiled plan");

        assert!(!serialized.contains("secret-qwen"));
        assert!(!serialized.contains("https://qwen.example/v1"));
        assert!(!serialized.to_ascii_lowercase().contains("apikey"));
        assert!(!compiled.dependency_fingerprint.contains("secret-qwen"));
    }

    #[test]
    fn unavailable_alias_target_is_reported_without_blocking_provider_updates() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.8"}]),
        );
        let mut route = route("router-qwen", "qwen", CodexModelSelection::All);
        route.label = Some("Qwen relay".to_string());
        route
            .aliases
            .insert("ghost".to_string(), "missing-model".to_string());
        let providers = [(provider.id.clone(), provider)].into_iter().collect();

        let compiled = compile_v2(&plan(vec![route]), &providers)
            .expect("stale alias must not block the remaining provider catalog");

        assert_eq!(compiled.visible_models, vec!["qwen3.8"]);
        assert!(compiled.warnings.iter().any(|warning| {
            warning.code == "alias_target_unavailable"
                && warning.route_id.as_deref() == Some("router-qwen")
                && warning.message.contains("Qwen relay")
                && warning.message.contains("Qwen")
        }));
    }

    #[test]
    fn route_without_label_uses_provider_name_for_compiled_display() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.8"}]),
        );
        let mut route = route("router-qwen", "qwen", CodexModelSelection::All);
        route.label = None;

        let compiled = compile(&plan(vec![route]), [provider]);

        assert_eq!(compiled.routes[0].label.as_deref(), Some("Qwen"));
    }

    #[test]
    fn dependency_fingerprint_changes_when_upstream_model_mapping_changes() {
        let first = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.8", "upstreamModel": "qwen-upstream-a"}]),
        );
        let changed = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.8", "upstreamModel": "qwen-upstream-b"}]),
        );
        let plan = plan(vec![route("router-qwen", "qwen", CodexModelSelection::All)]);

        assert_ne!(
            compile(&plan, [first]).dependency_fingerprint,
            compile(&plan, [changed]).dependency_fingerprint
        );
    }

    #[test]
    fn capability_summary_recursively_removes_unknown_secret_fields() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{
                "model": "qwen3.8",
                "reasoning": {
                    "supportStatus": "confirmed_supported",
                    "credentials": {"apiKey": "nested-reasoning-secret"}
                },
                "codexCache": {
                    "cacheMode": "auto_prefix_cache",
                    "promptCacheKey": "private-session-key",
                    "token": "nested-cache-secret"
                }
            }]),
        );
        let compiled = compile(
            &plan(vec![route("router-qwen", "qwen", CodexModelSelection::All)]),
            [provider],
        );
        let serialized = serde_json::to_string(&compiled).expect("serialize compiled plan");

        assert!(!serialized.contains("nested-reasoning-secret"));
        assert!(!serialized.contains("private-session-key"));
        assert!(!serialized.contains("nested-cache-secret"));
        assert!(serialized.contains("confirmed_supported"));
        assert!(serialized.contains("auto_prefix_cache"));
    }
}
