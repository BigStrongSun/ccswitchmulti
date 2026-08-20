use crate::codex_multirouter::schema::{
    validate_v2, CodexModelSelection, CodexRouteAuthPolicy, CodexRoutingConfigV2,
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
    pub target_provider_id: String,
    pub route_id: String,
    pub api_format: String,
    pub api_format_source: String,
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

#[derive(Clone)]
struct ModelCandidate<'a> {
    route_index: usize,
    route_id: &'a str,
    provider: &'a Provider,
    canonical_model: String,
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

    let candidates = collect_candidates(plan, providers)?;
    let collision_counts = canonical_collision_counts(&candidates);
    let canonical_owners = canonical_owner_counts(&candidates);
    let mut warnings = Vec::new();
    let mut used_visible = HashSet::new();
    let mut model_catalog = Vec::new();
    let mut route_visible_models = vec![Vec::new(); plan.routes.len()];

    for candidate in candidates {
        let route = &plan.routes[candidate.route_index];
        let explicit_aliases = route
            .aliases
            .iter()
            .filter(|(_, target)| target.eq_ignore_ascii_case(&candidate.canonical_model))
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
                upstream_model: upstream_model(candidate.model_entry, &candidate.canonical_model),
                target_provider_id: candidate.provider.id.clone(),
                route_id: candidate.route_id.to_string(),
                api_format,
                api_format_source,
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
            label: route.label.clone(),
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
    let dependency_fingerprint = dependency_fingerprint(plan, providers, &model_catalog)?;

    Ok(CompiledCodexRoutingPlan {
        routes,
        visible_models,
        model_catalog,
        dependency_fingerprint,
        warnings,
    })
}

fn collect_candidates<'a>(
    plan: &'a CodexRoutingConfigV2,
    providers: &'a HashMap<String, Provider>,
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
                        "target provider `{}` does not exist",
                        route.target_provider_id
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
        for model_entry in entries {
            let Some(canonical_model) = model_name(model_entry) else {
                continue;
            };
            let canonical_key = canonical_model.to_ascii_lowercase();
            if selected
                .as_ref()
                .is_some_and(|models| !models.contains(&canonical_key))
            {
                continue;
            }
            if !found.insert(canonical_key) {
                continue;
            }
            candidates.push(ModelCandidate {
                route_index,
                route_id: &route.id,
                provider,
                canonical_model,
                model_entry,
            });
        }
        if let Some(selected) = selected {
            let missing = selected.difference(&found).next();
            if let Some(missing) = missing {
                return Err(CodexRoutingCompileError {
                    code: "selected_model_missing".to_string(),
                    message: format!(
                        "route `{}` selects model `{missing}` which is not in provider `{}`",
                        route.id, provider.id
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
            return Err(CodexRoutingCompileError {
                code: "alias_target_missing".to_string(),
                message: format!(
                    "route `{}` aliases model `{missing_target}` which is not selected from provider `{}`",
                    route.id, provider.id
                ),
            });
        }
    }
    Ok(candidates)
}

fn provider_model_entries(provider: &Provider) -> Vec<&Value> {
    provider
        .settings_config
        .get("modelCatalog")
        .or_else(|| provider.settings_config.get("model_catalog"))
        .and_then(|catalog| catalog.get("models"))
        .and_then(Value::as_array)
        .map(|models| models.iter().collect())
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
        return candidate.canonical_model.clone();
    }
    format!(
        "{}-{}",
        candidate.canonical_model,
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
        string_array_field(model_entry, &["inputModalities", "input_modalities"]),
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
        context_window_source,
        input_modalities_source,
        reasoning_source,
        codex_cache_source,
    }
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
    let input = json!({
        "plan": safe_plan,
        "providers": provider_dependencies,
        "effectiveModels": effective_models,
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
                }
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
        let plan = plan(vec![
            route("first-route", "first", CodexModelSelection::All),
            route("second-route", "second", CodexModelSelection::All),
        ]);
        let forward = compile(&plan, [first.clone(), second.clone()]);
        let reverse = compile(&plan, [second.clone(), first.clone()]);
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
        let protocol_compiled = compile(&plan, [first.clone(), protocol_changed]);
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
        let model_compiled = compile(&plan, [first, model_changed]);
        assert_ne!(
            forward.dependency_fingerprint,
            model_compiled.dependency_fingerprint
        );
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
    fn all_selection_rejects_alias_targets_missing_from_provider_catalog() {
        let provider = provider(
            "qwen",
            "Qwen",
            "openai_responses",
            json!([{"model": "qwen3.8"}]),
        );
        let mut route = route("router-qwen", "qwen", CodexModelSelection::All);
        route
            .aliases
            .insert("ghost".to_string(), "missing-model".to_string());
        let providers = [(provider.id.clone(), provider)].into_iter().collect();

        let error = compile_v2(&plan(vec![route]), &providers)
            .expect_err("unknown alias target must not be discarded");

        assert_eq!(error.code, "alias_target_missing");
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
