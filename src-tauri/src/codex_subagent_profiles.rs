//! Codex V2 questionnaire persistence, validation, compilation, and safe preview projection.

use crate::proxy::providers::codex_reasoning::{
    CodexReasoningEffort, ResolvedSubagentReasoningCapability,
};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubagentVersion {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionPolicy {
    Balanced,
    OfficialFirst,
    ThirdPartyFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskStrength {
    LongContextReading,
    RepositoryExploration,
    EvidenceCollection,
    Summarization,
    ComplexDebugging,
    ArchitectureDesign,
    BoundedImplementation,
    ComplexImplementation,
    Testing,
    HighRiskReview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Optimization {
    Speed,
    Balanced,
    Quality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteScope {
    ReadOnly,
    BoundedChanges,
    ComplexChanges,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preference {
    Preferred,
    Eligible,
    Fallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyQuestionnaireReasoningEffort {
    Auto,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

pub type ModelReasoningEffort = CodexReasoningEffort;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningRuntimePolicy {
    Delegated,
    ModelDefault,
    Fixed,
    Disabled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSubagentReasoningPolicy {
    pub policy: ReasoningRuntimePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<CodexReasoningEffort>,
}

impl CodexSubagentReasoningPolicy {
    fn validate(&self, key: &str) -> Result<(), CompileError> {
        match (self.policy, self.effort) {
            (ReasoningRuntimePolicy::Fixed, Some(CodexReasoningEffort::None)) => {
                Err(validation_error(
                    "invalid_reasoning_policy",
                    Some(key),
                    "fixed effort cannot be none",
                ))
            }
            (ReasoningRuntimePolicy::Fixed, Some(_)) => Ok(()),
            (ReasoningRuntimePolicy::Fixed, None) => Err(validation_error(
                "missing_fixed_reasoning_effort",
                Some(key),
                "fixed reasoning policy requires effort",
            )),
            (_, Some(_)) => Err(validation_error(
                "unexpected_reasoning_effort",
                Some(key),
                "only fixed reasoning policy accepts effort",
            )),
            (_, None) => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Official,
    ThirdParty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InputModality {
    Text,
    Image,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexSubagentV2 {
    pub(crate) schema_version: u8,
    pub(crate) selection_policy: SelectionPolicy,
    pub(crate) profiles: Vec<ParsedProfileEntry>,
}

struct PublicProfiles<'a>(&'a [ParsedProfileEntry]);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSubagentQuestionnaire {
    pub task_strengths: Vec<TaskStrength>,
    pub optimization: Optimization,
    pub write_scope: WriteScope,
    pub preference: Preference,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSubagentProfileConfig {
    pub model: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_modalities: Option<Vec<InputModality>>,
    pub questionnaire: CodexSubagentQuestionnaire,
    pub reasoning: CodexSubagentReasoningPolicy,
    #[serde(
        default,
        skip_serializing_if = "CodexSubagentProfileOverrides::is_empty"
    )]
    pub overrides: CodexSubagentProfileOverrides,
}

impl Serialize for PublicProfiles<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for entry in self.0 {
            match entry {
                ParsedProfileEntry::Valid(profile) => {
                    map.serialize_entry(
                        &profile.key,
                        &CodexSubagentProfileConfig {
                            model: profile.model.clone(),
                            enabled: profile.enabled,
                            input_modalities: profile.input_modalities.clone(),
                            questionnaire: CodexSubagentQuestionnaire {
                                task_strengths: profile.strengths.clone(),
                                optimization: profile.optimization,
                                write_scope: profile.write_scope,
                                preference: profile.preference,
                            },
                            reasoning: profile.reasoning.clone(),
                            overrides: profile.overrides.clone(),
                        },
                    )?;
                }
                ParsedProfileEntry::Invalid { key, raw, .. } => {
                    map.serialize_entry(key, raw)?;
                }
            }
        }
        map.end()
    }
}

impl Serialize for CodexSubagentV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("CodexSubagentV2", 3)?;
        state.serialize_field("schemaVersion", &self.schema_version)?;
        state.serialize_field("selectionPolicy", &self.selection_policy)?;
        state.serialize_field("profiles", &PublicProfiles(&self.profiles))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for CodexSubagentV2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_persisted_subagent_v2(&value)
            .map_err(|error| serde::de::Error::custom(format!("{error:?}")))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParsedProfileEntry {
    Valid(ParsedCodexSubagentProfile),
    Invalid {
        key: String,
        raw: Value,
        validation_code: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedCodexSubagentProfile {
    pub key: String,
    pub model: String,
    pub enabled: bool,
    pub input_modalities: Option<Vec<InputModality>>,
    pub strengths: Vec<TaskStrength>,
    pub optimization: Optimization,
    pub write_scope: WriteScope,
    pub preference: Preference,
    pub reasoning: CodexSubagentReasoningPolicy,
    pub overrides: CodexSubagentProfileOverrides,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSubagentProfileOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub developer_instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname_candidates: Option<Vec<String>>,
}

impl CodexSubagentProfileOverrides {
    pub fn is_empty(&self) -> bool {
        self.role_name.is_none()
            && self.description.is_none()
            && self.developer_instructions.is_none()
            && self.nickname_candidates.is_none()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyCodexSubagentProfileOverrides {
    role_name: Option<String>,
    description: Option<String>,
    developer_instructions: Option<String>,
    nickname_candidates: Option<Vec<String>>,
    model_reasoning_effort: Option<CodexReasoningEffort>,
}

impl LegacyCodexSubagentProfileOverrides {
    fn into_current(self) -> CodexSubagentProfileOverrides {
        CodexSubagentProfileOverrides {
            role_name: self.role_name,
            description: self.description,
            developer_instructions: self.developer_instructions,
            nickname_candidates: self.nickname_candidates,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogModel {
    pub model: String,
    pub provider_kind: ProviderKind,
    pub routable: bool,
    pub context_window: u64,
    pub reasoning: ResolvedSubagentReasoningCapability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileRequest {
    pub subagent_version: SubagentVersion,
    pub persisted_subagent_v2: Option<CodexSubagentV2>,
    pub catalog_models: Vec<CatalogModel>,
    pub occupied_role_names: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CompileOutput {
    pub generated_roles: Vec<GeneratedRole>,
    pub profile_statuses: Vec<ProfileStatus>,
    pub preserved_invalid_profiles: Vec<Value>,
    pub diagnostics: Vec<Diagnostic>,
    pub legacy_managed_roles_preserved: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedRole {
    pub requested_role_name: String,
    pub effective_role_name: String,
    pub description: String,
    pub developer_instructions: String,
    pub nickname_candidates: Vec<String>,
    pub model: String,
    pub model_provider: String,
    pub effort: Option<CodexReasoningEffort>,
    pub context_window: u64,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
struct RoleToml<'a> {
    name: &'a str,
    description: &'a str,
    developer_instructions: &'a str,
    nickname_candidates: &'a [String],
    model: &'a str,
    model_provider: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_reasoning_effort: Option<CodexReasoningEffort>,
    model_context_window: u64,
}

pub fn render_generated_role_toml(
    role: &GeneratedRole,
    managed_marker: &str,
) -> Result<String, CompileError> {
    let body = toml::to_string(&RoleToml {
        name: &role.effective_role_name,
        description: &role.description,
        developer_instructions: &role.developer_instructions,
        nickname_candidates: &role.nickname_candidates,
        model: &role.model,
        model_provider: &role.model_provider,
        model_reasoning_effort: role.effort,
        model_context_window: role.context_window,
    })
    .map_err(|error| validation_error("toml_serialization", None, &error.to_string()))?;
    Ok(format!("{managed_marker}\n{body}"))
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProfileStatus {
    pub(crate) key: String,
    pub(crate) model: Option<String>,
    pub(crate) status: ProfileStatusCode,
    pub(crate) reason: Option<DiagnosticReasonCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatusCode {
    Routable,
    Disabled,
    Unroutable,
    Invalid,
    Collision,
    InactiveV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticReasonCode {
    Disabled,
    Unroutable,
    Invalid,
    Collision,
    InactiveV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    model: Option<String>,
    role: Option<String>,
    profile_key: Option<String>,
    policy: SelectionPolicy,
    status: ProfileStatusCode,
    reason_code: Option<DiagnosticReasonCode>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CompileError {
    Validation {
        code: String,
        profile_key: Option<String>,
        detail: String,
    },
}

pub type CompileResult = Result<CompileOutput, CompileError>;

fn validation_error(code: &str, key: Option<&str>, detail: &str) -> CompileError {
    CompileError::Validation {
        code: code.to_string(),
        profile_key: key.map(ToString::to_string),
        detail: detail.to_string(),
    }
}

fn enum_field<T: for<'de> Deserialize<'de>>(
    value: Option<&Value>,
    missing_code: &str,
    invalid_code: &str,
    key: &str,
    missing_detail: &str,
    invalid_detail: &str,
) -> Result<T, CompileError> {
    let value = value.ok_or_else(|| validation_error(missing_code, Some(key), missing_detail))?;
    serde_json::from_value(value.clone())
        .map_err(|_| validation_error(invalid_code, Some(key), invalid_detail))
}

pub fn parse_persisted_subagent_v2(raw: &Value) -> Result<CodexSubagentV2, CompileError> {
    if !raw.is_object() {
        return Err(validation_error(
            "invalid_subagent_v2",
            None,
            "subagentV2 must be an object",
        ));
    }
    let schema = raw
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            validation_error("missing_schema_version", None, "schemaVersion is required")
        })?;
    if !matches!(schema, 1 | 2) {
        return Err(validation_error(
            "unsupported_schema_version",
            None,
            "schemaVersion must be 1 or 2",
        ));
    }
    let selection_policy = match raw.get("selectionPolicy") {
        None => SelectionPolicy::Balanced,
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| {
            validation_error(
                "invalid_selection_policy",
                None,
                "selectionPolicy is not an allowed enum member",
            )
        })?,
    };
    let profiles_value = raw
        .get("profiles")
        .ok_or_else(|| validation_error("missing_profiles", None, "profiles is required"))?;
    let profiles = profiles_value
        .as_object()
        .cloned()
        .ok_or_else(|| validation_error("invalid_profiles", None, "profiles must be an object"))?;
    let mut parsed = Vec::with_capacity(profiles.len());
    for (key, raw_profile) in profiles {
        let p = raw_profile.as_object().ok_or_else(|| {
            validation_error("invalid_profile", Some(&key), "profile must be an object")
        })?;
        let model_value = p
            .get("model")
            .ok_or_else(|| validation_error("missing_model", Some(&key), "model is required"))?;
        let model = model_value.as_str().ok_or_else(|| {
            validation_error("invalid_model", Some(&key), "model must be a string")
        })?;
        if model.trim().is_empty() {
            return Err(validation_error(
                "empty_model",
                Some(&key),
                "model must be nonempty",
            ));
        }
        let enabled_value = p.get("enabled").ok_or_else(|| {
            validation_error("missing_enabled", Some(&key), "enabled is required")
        })?;
        let enabled = enabled_value.as_bool().ok_or_else(|| {
            validation_error("invalid_enabled", Some(&key), "enabled must be a boolean")
        })?;
        let input_modalities = match p.get("inputModalities") {
            None => None,
            Some(value) => {
                let modalities = value.as_array().ok_or_else(|| {
                    validation_error(
                        "invalid_input_modalities",
                        Some(&key),
                        "inputModalities must be an array",
                    )
                })?;
                let parsed = modalities
                    .iter()
                    .map(|item| {
                        serde_json::from_value::<InputModality>(item.clone()).map_err(|_| {
                            validation_error(
                                "invalid_input_modalities",
                                Some(&key),
                                "inputModalities allows only text and image",
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let valid = matches!(parsed.as_slice(), [InputModality::Text])
                    || matches!(
                        parsed.as_slice(),
                        [InputModality::Text, InputModality::Image]
                    );
                if !valid {
                    return Err(validation_error(
                        "invalid_input_modalities",
                        Some(&key),
                        "inputModalities must be exactly [text] or [text, image]",
                    ));
                }
                Some(parsed)
            }
        };
        let questionnaire_value = p.get("questionnaire").ok_or_else(|| {
            validation_error(
                "missing_questionnaire",
                Some(&key),
                "questionnaire is required",
            )
        })?;
        let q = questionnaire_value.as_object().ok_or_else(|| {
            validation_error(
                "invalid_questionnaire",
                Some(&key),
                "questionnaire must be an object",
            )
        })?;
        let strengths_value = q.get("taskStrengths").ok_or_else(|| {
            validation_error(
                "missing_task_strengths",
                Some(&key),
                "questionnaire.taskStrengths is required",
            )
        })?;
        let strengths_array = strengths_value.as_array().ok_or_else(|| {
            validation_error(
                "unknown_task_strength",
                Some(&key),
                "taskStrengths contains an unknown enum member",
            )
        })?;
        if !(1..=5).contains(&strengths_array.len()) {
            return Err(validation_error(
                "strength_count",
                Some(&key),
                "taskStrengths must contain 1 through 5 members",
            ));
        }
        let mut strengths = Vec::new();
        let mut seen = HashSet::new();
        for item in strengths_array {
            let strength: TaskStrength = serde_json::from_value(item.clone()).map_err(|_| {
                validation_error(
                    "unknown_task_strength",
                    Some(&key),
                    "taskStrengths contains an unknown enum member",
                )
            })?;
            if !seen.insert(strength) {
                return Err(validation_error(
                    "duplicate_task_strength",
                    Some(&key),
                    "taskStrengths members must be unique",
                ));
            }
            strengths.push(strength);
        }
        let optimization = enum_field(
            q.get("optimization"),
            "missing_optimization",
            "invalid_optimization",
            &key,
            "questionnaire.optimization is required",
            "optimization is not an allowed enum member",
        )?;
        let write_scope = enum_field(
            q.get("writeScope"),
            "missing_write_scope",
            "invalid_write_scope",
            &key,
            "questionnaire.writeScope is required",
            "writeScope is not an allowed enum member",
        )?;
        let preference = enum_field(
            q.get("preference"),
            "missing_preference",
            "invalid_preference",
            &key,
            "questionnaire.preference is required",
            "preference is not an allowed enum member",
        )?;
        let (reasoning, overrides) = if schema == 1 {
            let legacy_effort: LegacyQuestionnaireReasoningEffort = enum_field(
                q.get("reasoningEffort"),
                "missing_reasoning_effort",
                "invalid_reasoning_effort",
                &key,
                "questionnaire.reasoningEffort is required",
                "reasoningEffort is not an allowed enum member",
            )?;
            let legacy_overrides: LegacyCodexSubagentProfileOverrides = match p.get("overrides") {
                Some(v) => serde_json::from_value(v.clone()).map_err(|_| {
                    validation_error(
                        "invalid_override_effort",
                        Some(&key),
                        "modelReasoningEffort allows only low, medium, high, xhigh, max, or ultra",
                    )
                })?,
                None => LegacyCodexSubagentProfileOverrides::default(),
            };
            let fixed_effort = legacy_overrides
                .model_reasoning_effort
                .or(match legacy_effort {
                    LegacyQuestionnaireReasoningEffort::Auto => None,
                    LegacyQuestionnaireReasoningEffort::Low => Some(CodexReasoningEffort::Low),
                    LegacyQuestionnaireReasoningEffort::Medium => {
                        Some(CodexReasoningEffort::Medium)
                    }
                    LegacyQuestionnaireReasoningEffort::High => Some(CodexReasoningEffort::High),
                    LegacyQuestionnaireReasoningEffort::XHigh => Some(CodexReasoningEffort::XHigh),
                });
            let reasoning = match fixed_effort {
                Some(effort) => CodexSubagentReasoningPolicy {
                    policy: ReasoningRuntimePolicy::Fixed,
                    effort: Some(effort),
                },
                None => CodexSubagentReasoningPolicy {
                    policy: ReasoningRuntimePolicy::Delegated,
                    effort: None,
                },
            };
            (reasoning, legacy_overrides.into_current())
        } else {
            if q.contains_key("reasoningEffort") {
                return Err(validation_error(
                    "legacy_reasoning_effort_in_schema_v2",
                    Some(&key),
                    "schemaVersion 2 stores reasoning outside questionnaire",
                ));
            }
            let reasoning: CodexSubagentReasoningPolicy = enum_field(
                p.get("reasoning"),
                "missing_reasoning_policy",
                "invalid_reasoning_policy",
                &key,
                "reasoning is required",
                "reasoning policy is invalid",
            )?;
            reasoning.validate(&key)?;
            let overrides: CodexSubagentProfileOverrides = match p.get("overrides") {
                Some(v) => {
                    if v.get("modelReasoningEffort").is_some() {
                        return Err(validation_error(
                            "legacy_override_effort_in_schema_v2",
                            Some(&key),
                            "schemaVersion 2 stores effort in reasoning",
                        ));
                    }
                    serde_json::from_value(v.clone()).map_err(|_| {
                        validation_error("invalid_overrides", Some(&key), "overrides is invalid")
                    })?
                }
                None => CodexSubagentProfileOverrides::default(),
            };
            (reasoning, overrides)
        };
        if key != normalize_profile_key(model) {
            return Err(validation_error(
                "profile_key_model_mismatch",
                Some(&key),
                "profile key must equal normalize_profile_key(model)",
            ));
        }
        parsed.push(ParsedProfileEntry::Valid(ParsedCodexSubagentProfile {
            key,
            model: model.to_string(),
            enabled,
            input_modalities,
            strengths,
            optimization,
            write_scope,
            preference,
            reasoning,
            overrides,
        }));
    }
    Ok(CodexSubagentV2 {
        schema_version: 2,
        selection_policy,
        profiles: parsed,
    })
}

pub fn normalize_profile_key(value: &str) -> String {
    value
        .trim()
        .nfkc()
        .collect::<String>()
        .case_fold()
        .collect()
}

/// Runtime loader that preserves malformed profile values while retaining strict top-level
/// schema validation. This lets the UI surface and repair one bad entry without losing peers.
pub fn parse_persisted_subagent_v2_tolerant(raw: &Value) -> Result<CodexSubagentV2, CompileError> {
    let source_schema = raw
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            validation_error("missing_schema_version", None, "schemaVersion is required")
        })?;
    let profiles = raw
        .get("profiles")
        .ok_or_else(|| validation_error("missing_profiles", None, "profiles is required"))?
        .as_object()
        .cloned()
        .ok_or_else(|| validation_error("invalid_profiles", None, "profiles must be an object"))?;
    let mut top_level = raw.as_object().cloned().ok_or_else(|| {
        validation_error("invalid_subagent_v2", None, "subagentV2 must be an object")
    })?;
    top_level.insert("profiles".to_string(), serde_json::json!({}));
    let top_level = Value::Object(top_level);
    let mut parsed = parse_persisted_subagent_v2(&top_level)?;
    for (key, profile_raw) in profiles {
        let one = serde_json::json!({
            "schemaVersion": source_schema,
            "selectionPolicy": parsed.selection_policy,
            "profiles": { key.clone(): profile_raw.clone() }
        });
        match parse_persisted_subagent_v2(&one) {
            Ok(one) => parsed.profiles.extend(one.profiles),
            Err(CompileError::Validation { code, .. }) => {
                parsed.profiles.push(ParsedProfileEntry::Invalid {
                    key,
                    raw: profile_raw,
                    validation_code: code,
                });
            }
        }
    }
    Ok(parsed)
}

fn normalize_role_name(value: &str) -> String {
    let mut out = String::new();
    let mut invalid = false;
    for ch in value.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_' {
            if invalid && !out.is_empty() {
                out.push('-');
            }
            invalid = false;
            out.push(ch);
        } else {
            invalid = true;
        }
    }
    loop {
        let next = out
            .replace("--", "-")
            .replace("__", "_")
            .replace("-_", "-")
            .replace("_-", "-");
        if next == out {
            break;
        }
        out = next;
    }
    out.trim_matches(['-', '_']).to_string()
}

fn compile_reasoning_policy(
    policy: &CodexSubagentReasoningPolicy,
    capability: &ResolvedSubagentReasoningCapability,
    profile_key: &str,
) -> Result<Option<CodexReasoningEffort>, CompileError> {
    match policy.policy {
        ReasoningRuntimePolicy::Delegated => Ok(None),
        ReasoningRuntimePolicy::ModelDefault => {
            capability.provider_default_effort.map(Some).ok_or_else(|| {
                validation_error(
                    "missing_default_reasoning_effort",
                    Some(profile_key),
                    "target model has no resolved default reasoning effort",
                )
            })
        }
        ReasoningRuntimePolicy::Fixed => {
            let effort = policy.effort.ok_or_else(|| {
                validation_error(
                    "missing_fixed_reasoning_effort",
                    Some(profile_key),
                    "fixed reasoning policy requires effort",
                )
            })?;
            if !capability.codex_selectable_efforts.contains(&effort) {
                return Err(validation_error(
                    "unsupported_reasoning_effort",
                    Some(profile_key),
                    "fixed reasoning effort is not supported by the target model",
                ));
            }
            Ok(Some(effort))
        }
        ReasoningRuntimePolicy::Disabled => {
            if !capability.disable_allowed
                || !capability
                    .codex_selectable_efforts
                    .contains(&CodexReasoningEffort::None)
            {
                return Err(validation_error(
                    "reasoning_disable_unsupported",
                    Some(profile_key),
                    "target model does not support disabling reasoning",
                ));
            }
            Ok(Some(CodexReasoningEffort::None))
        }
    }
}

fn strength_label(strength: TaskStrength) -> &'static str {
    match strength {
        TaskStrength::LongContextReading => "long-context reading",
        TaskStrength::RepositoryExploration => "repository exploration",
        TaskStrength::EvidenceCollection => "evidence collection",
        TaskStrength::Summarization => "summarization",
        TaskStrength::ComplexDebugging => "complex debugging",
        TaskStrength::ArchitectureDesign => "architecture design",
        TaskStrength::BoundedImplementation => "bounded implementation",
        TaskStrength::ComplexImplementation => "complex implementation",
        TaskStrength::Testing => "testing",
        TaskStrength::HighRiskReview => "high-risk review",
    }
}

fn joined_strengths(strengths: &[TaskStrength]) -> String {
    let labels = strengths
        .iter()
        .copied()
        .map(strength_label)
        .collect::<Vec<_>>();
    match labels.as_slice() {
        [] => "unspecified delegated work".to_string(),
        [only] => (*only).to_string(),
        [left, right] => format!("{left} and {right}"),
        _ => format!(
            "{}, and {}",
            labels[..labels.len() - 1].join(", "),
            labels[labels.len() - 1]
        ),
    }
}

fn optimization_label(value: Optimization) -> &'static str {
    match value {
        Optimization::Speed => "speed",
        Optimization::Balanced => "balanced execution",
        Optimization::Quality => "quality",
    }
}

fn write_scope_label(value: WriteScope) -> &'static str {
    match value {
        WriteScope::ReadOnly => "read-only work",
        WriteScope::BoundedChanges => "bounded changes",
        WriteScope::ComplexChanges => "complex changes",
    }
}

fn provider_label(value: ProviderKind) -> &'static str {
    match value {
        ProviderKind::Official => "official",
        ProviderKind::ThirdParty => "third-party",
    }
}

const ALL_TASK_STRENGTHS: [TaskStrength; 10] = [
    TaskStrength::LongContextReading,
    TaskStrength::RepositoryExploration,
    TaskStrength::EvidenceCollection,
    TaskStrength::Summarization,
    TaskStrength::ComplexDebugging,
    TaskStrength::ArchitectureDesign,
    TaskStrength::BoundedImplementation,
    TaskStrength::ComplexImplementation,
    TaskStrength::Testing,
    TaskStrength::HighRiskReview,
];

fn excluded_strengths(strengths: &[TaskStrength]) -> String {
    let selected = strengths.iter().copied().collect::<HashSet<_>>();
    joined_strengths(
        &ALL_TASK_STRENGTHS
            .iter()
            .copied()
            .filter(|strength| !selected.contains(strength))
            .collect::<Vec<_>>(),
    )
}

fn selection_behavior(
    policy: SelectionPolicy,
    preference: Preference,
    provider_kind: ProviderKind,
) -> String {
    let provider = provider_label(provider_kind);
    match preference {
        Preference::Fallback => format!(
            "This {provider} fallback profile is never promoted and may be used only when preferred profiles are unavailable or it is explicitly requested."
        ),
        Preference::Preferred => match policy {
            SelectionPolicy::OfficialFirst => format!(
                "Task match makes this preferred {provider} profile override the global official-first provider bias."
            ),
            SelectionPolicy::ThirdPartyFirst if provider_kind == ProviderKind::ThirdParty => {
                "Under third-party-first selection, matching preferred or eligible third-party profiles go first; this profile is preferred.".to_string()
            }
            SelectionPolicy::ThirdPartyFirst => format!(
                "Under third-party-first selection, matching preferred or eligible third-party profiles go first; this {provider} profile remains preferred when its task match is stronger."
            ),
            SelectionPolicy::Balanced => "Under balanced selection, when declared task strengths match, prefer this specialized role over the built-in generic default, worker, and explorer roles; provider identity does not break ties otherwise.".to_string(),
        },
        Preference::Eligible => match policy {
            SelectionPolicy::OfficialFirst if provider_kind == ProviderKind::ThirdParty => {
                "Under official-first selection, this eligible third-party profile may be chosen only for a strong, well-bounded task match; otherwise prefer an official provider path.".to_string()
            }
            SelectionPolicy::OfficialFirst => {
                "Under official-first selection, this eligible official profile follows the global official provider preference when the task matches.".to_string()
            }
            SelectionPolicy::ThirdPartyFirst if provider_kind == ProviderKind::ThirdParty => {
                "Under third-party-first selection, matching preferred or eligible third-party profiles go first; this profile is eligible.".to_string()
            }
            SelectionPolicy::ThirdPartyFirst => {
                "Under third-party-first selection, matching preferred or eligible third-party profiles go first; this official profile remains eligible when no stronger third-party task match exists.".to_string()
            }
            SelectionPolicy::Balanced => format!(
                "Under balanced selection, this eligible {provider} profile has no provider bias."
            ),
        },
    }
}

fn instruction_selection_behavior(
    policy: SelectionPolicy,
    preference: Preference,
    provider_kind: ProviderKind,
) -> String {
    match (policy, preference, provider_kind) {
        (_, Preference::Fallback, kind) => format!(
            "Never promote this {} fallback profile; use it only when preferred profiles are unavailable or it is explicitly requested.",
            provider_label(kind)
        ),
        (SelectionPolicy::OfficialFirst, Preference::Preferred, kind) => format!(
            "Task match makes this preferred {} profile override the global official-first provider bias.",
            provider_label(kind)
        ),
        (SelectionPolicy::OfficialFirst, Preference::Eligible, ProviderKind::ThirdParty) => {
            "Under official-first selection, use this eligible third-party profile only for a strong, well-bounded task match; otherwise prefer an official provider path.".to_string()
        }
        (SelectionPolicy::ThirdPartyFirst, Preference::Eligible, ProviderKind::ThirdParty) => {
            "Under third-party-first selection, put matching preferred or eligible third-party profiles first; this profile is eligible.".to_string()
        }
        _ => selection_behavior(policy, preference, provider_kind),
    }
}

fn generated_description_for_provider(
    policy: SelectionPolicy,
    p: &ParsedCodexSubagentProfile,
    provider_kind: ProviderKind,
) -> String {
    let base = if let Some(description) = &p.overrides.description {
        description.clone()
    } else {
        format!(
        "This role matches delegated {} tasks. It excludes {}, and it does not own final integration, merging, or release. It optimizes for {} and is limited to {}. {}",
        joined_strengths(&p.strengths),
        excluded_strengths(&p.strengths),
        optimization_label(p.optimization),
        write_scope_label(p.write_scope),
        selection_behavior(policy, p.preference, provider_kind),
        )
    };
    match p.input_modalities.as_deref() {
        Some([InputModality::Text]) => {
            format!("{base} It accepts text input only and cannot inspect or understand images.")
        }
        Some([InputModality::Text, InputModality::Image]) => {
            format!("{base} It supports text and image input, including image understanding.")
        }
        _ => format!(
            "{base} Input capabilities are unknown, so this role must not be assigned tasks that depend on image understanding."
        ),
    }
}

fn generated_instructions_for_provider(
    policy: SelectionPolicy,
    p: &ParsedCodexSubagentProfile,
    provider_kind: ProviderKind,
) -> String {
    let base = if let Some(value) = &p.overrides.developer_instructions {
        value.clone()
    } else {
        let scope_instruction = match p.write_scope {
        WriteScope::ReadOnly => format!(
            "Optimize for {} and keep all work read-only.",
            optimization_label(p.optimization)
        ),
        WriteScope::BoundedChanges => format!(
            "Optimize for {} and edit only the files explicitly assigned to this role; do not expand ownership without parent approval.",
            optimization_label(p.optimization)
        ),
        WriteScope::ComplexChanges => format!(
            "Optimize for {}; cross-module changes are allowed within the delegated objective, but final integration, merging, and release remain with the parent agent.",
            optimization_label(p.optimization)
        ),
        };
        let task_boundary = if p.write_scope == WriteScope::ReadOnly {
            format!(
                "Work only on delegated {} tasks and do not edit files.",
                joined_strengths(&p.strengths)
            )
        } else {
            format!(
                "Work only on delegated {} tasks.",
                joined_strengths(&p.strengths)
            )
        };
        let final_boundary = if p.write_scope == WriteScope::ComplexChanges {
            "Return concrete evidence and verification to the parent agent."
        } else {
            "Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent."
        };
        format!(
            "{task_boundary} {scope_instruction} {} {final_boundary}",
            instruction_selection_behavior(policy, p.preference, provider_kind),
        )
    };
    match p.input_modalities.as_deref() {
        Some([InputModality::Text]) => format!(
            "{base} This model does not support image input; do not select this role for tasks that depend on image understanding."
        ),
        Some([InputModality::Text, InputModality::Image]) => format!(
            "{base} This model supports image input and may be selected for tasks that require image understanding."
        ),
        _ => format!(
            "{base} This model's input capabilities are unknown; do not select this role for tasks that depend on image understanding."
        ),
    }
}

fn default_role_name(p: &ParsedCodexSubagentProfile) -> String {
    p.key.clone()
}

fn is_valid_codex_nickname(nickname: &str) -> bool {
    !nickname.is_empty()
        && nickname.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_')
        })
}

fn sanitize_codex_nickname(source: &str) -> String {
    let sanitized = source
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_') {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = sanitized.chars();
    let nickname = chars
        .next()
        .map(|character| character.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default();
    if is_valid_codex_nickname(&nickname) {
        nickname
    } else {
        "CCSwitch Worker".to_string()
    }
}

fn default_nickname(p: &ParsedCodexSubagentProfile) -> String {
    sanitize_codex_nickname(&p.key)
}

fn profile_collision_identity(entry: &ParsedProfileEntry) -> String {
    match entry {
        ParsedProfileEntry::Valid(profile) => normalize_profile_key(&profile.model),
        ParsedProfileEntry::Invalid { key, raw, .. } => raw
            .get("model")
            .and_then(Value::as_str)
            .map(normalize_profile_key)
            .filter(|identity| !identity.is_empty())
            .unwrap_or_else(|| normalize_profile_key(key)),
    }
}

fn validate_and_trim_intrinsic_overrides(
    profile: &mut ParsedCodexSubagentProfile,
) -> Result<(), CompileError> {
    if let Some(description) = profile.overrides.description.as_mut() {
        *description = description.trim().to_string();
        if description.is_empty() {
            return Err(validation_error(
                "empty_description",
                Some(&profile.key),
                "description must contain non-whitespace characters",
            ));
        }
    }
    if let Some(instructions) = profile.overrides.developer_instructions.as_mut() {
        *instructions = instructions.trim().to_string();
        if instructions.is_empty() {
            return Err(validation_error(
                "empty_developer_instructions",
                Some(&profile.key),
                "developerInstructions must contain non-whitespace characters",
            ));
        }
    }
    if let Some(nicknames) = profile.overrides.nickname_candidates.as_mut() {
        if !(1..=3).contains(&nicknames.len()) {
            return Err(validation_error(
                "nickname_count",
                Some(&profile.key),
                "nicknameCandidates must contain 1 through 3 entries",
            ));
        }
        let mut seen = HashSet::new();
        for nickname in nicknames {
            let was_empty = nickname.is_empty();
            *nickname = nickname.trim().to_string();
            if nickname.is_empty() {
                return Err(validation_error(
                    "empty_nickname",
                    Some(&profile.key),
                    if was_empty {
                        "nickname must be nonempty"
                    } else {
                        "nickname must contain non-whitespace characters"
                    },
                ));
            }
            if !is_valid_codex_nickname(nickname) {
                return Err(validation_error(
                    "invalid_nickname",
                    Some(&profile.key),
                    "nickname uses only ASCII alphanumeric, space, dash, underscore",
                ));
            }
            if !seen.insert(nickname.clone()) {
                return Err(validation_error(
                    "duplicate_nickname",
                    Some(&profile.key),
                    "nicknameCandidates must be unique",
                ));
            }
        }
    }
    Ok(())
}

pub fn compile_subagent_v2_profiles(request: &CompileRequest) -> CompileResult {
    let Some(config) = &request.persisted_subagent_v2 else {
        return Ok(CompileOutput {
            generated_roles: vec![],
            profile_statuses: vec![],
            preserved_invalid_profiles: vec![],
            diagnostics: vec![],
            legacy_managed_roles_preserved: true,
        });
    };
    let mut normalized: HashMap<String, usize> = HashMap::new();
    for entry in &config.profiles {
        *normalized
            .entry(profile_collision_identity(entry))
            .or_default() += 1;
    }
    let mut output = CompileOutput {
        generated_roles: vec![],
        profile_statuses: vec![],
        preserved_invalid_profiles: vec![],
        diagnostics: vec![],
        legacy_managed_roles_preserved: false,
    };
    let mut occupied: HashSet<String> = request
        .occupied_role_names
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    for entry in &config.profiles {
        let collision = normalized
            .get(&profile_collision_identity(entry))
            .copied()
            .unwrap_or(0)
            > 1;
        let mut p = match entry {
            ParsedProfileEntry::Invalid { raw, .. } => {
                output.preserved_invalid_profiles.push(raw.clone());
                output.profile_statuses.push(ProfileStatus {
                    key: String::new(),
                    model: None,
                    status: if collision {
                        ProfileStatusCode::Collision
                    } else {
                        ProfileStatusCode::Invalid
                    },
                    reason: Some(if collision {
                        DiagnosticReasonCode::Collision
                    } else {
                        DiagnosticReasonCode::Invalid
                    }),
                });
                continue;
            }
            ParsedProfileEntry::Valid(p) => p.clone(),
        };
        validate_and_trim_intrinsic_overrides(&mut p)?;
        let push_status = |output: &mut CompileOutput, status, reason| {
            output.profile_statuses.push(ProfileStatus {
                key: p.key.clone(),
                model: Some(p.model.clone()),
                status,
                reason,
            })
        };
        if collision {
            push_status(
                &mut output,
                ProfileStatusCode::Collision,
                Some(DiagnosticReasonCode::Collision),
            );
            continue;
        }
        if request.subagent_version == SubagentVersion::V1 {
            push_status(
                &mut output,
                ProfileStatusCode::InactiveV1,
                Some(DiagnosticReasonCode::InactiveV1),
            );
            continue;
        }
        if !p.enabled {
            push_status(
                &mut output,
                ProfileStatusCode::Disabled,
                Some(DiagnosticReasonCode::Disabled),
            );
            continue;
        }
        let catalog = request
            .catalog_models
            .iter()
            .find(|m| m.model.eq_ignore_ascii_case(&p.model) && m.routable);
        let Some(catalog) = catalog else {
            push_status(
                &mut output,
                ProfileStatusCode::Unroutable,
                Some(DiagnosticReasonCode::Unroutable),
            );
            continue;
        };
        let requested = p
            .overrides
            .role_name
            .clone()
            .unwrap_or_else(|| default_role_name(&p));
        let base = normalize_role_name(&requested);
        if base.is_empty() {
            return Err(validation_error(
                "empty_role_name",
                Some(&p.key),
                "roleName is empty after ASCII normalization",
            ));
        }
        if matches!(base.as_str(), "default" | "worker" | "explorer") {
            return Err(validation_error(
                "reserved_role_name",
                Some(&p.key),
                "normalized roleName conflicts with a built-in role",
            ));
        }
        let mut effective = base.clone();
        if occupied.contains(&effective.to_ascii_lowercase()) {
            effective = format!("ccswitch-{base}");
            let mut suffix = 2;
            while occupied.contains(&effective.to_ascii_lowercase()) {
                effective = format!("ccswitch-{base}-{suffix}");
                suffix += 1;
            }
        }
        occupied.insert(effective.to_ascii_lowercase());
        let nicknames = p
            .overrides
            .nickname_candidates
            .clone()
            .unwrap_or_else(|| vec![default_nickname(&p)]);
        let effort = compile_reasoning_policy(&p.reasoning, &catalog.reasoning, &p.key)?;
        let warnings = Vec::new();
        output.generated_roles.push(GeneratedRole {
            requested_role_name: requested,
            effective_role_name: effective,
            description: generated_description_for_provider(
                config.selection_policy,
                &p,
                catalog.provider_kind,
            ),
            developer_instructions: generated_instructions_for_provider(
                config.selection_policy,
                &p,
                catalog.provider_kind,
            ),
            nickname_candidates: nicknames,
            model: p.model.clone(),
            model_provider: "codex_model_router_v2".to_string(),
            effort,
            context_window: catalog.context_window,
            warnings,
        });
        push_status(&mut output, ProfileStatusCode::Routable, None);
    }
    output.diagnostics = output
        .profile_statuses
        .iter()
        .filter(|status| status.status != ProfileStatusCode::Routable)
        .map(|status| Diagnostic {
            model: status.model.clone(),
            role: None,
            profile_key: None,
            policy: config.selection_policy,
            status: status.status,
            reason_code: status.reason,
        })
        .collect();
    Ok(output)
}

pub fn initialize_legacy_subagent_v2() -> Result<CodexSubagentV2, CompileError> {
    let flash = ParsedCodexSubagentProfile {
        key: "deepseek-v4-flash".into(),
        model: "deepseek-v4-flash".into(),
        enabled: true,
        input_modalities: Some(vec![InputModality::Text]),
        strengths: vec![
            TaskStrength::LongContextReading,
            TaskStrength::RepositoryExploration,
            TaskStrength::EvidenceCollection,
            TaskStrength::Summarization,
            TaskStrength::Testing,
        ],
        optimization: Optimization::Speed,
        write_scope: WriteScope::ReadOnly,
        preference: Preference::Preferred,
        reasoning: CodexSubagentReasoningPolicy {
            policy: ReasoningRuntimePolicy::Delegated,
            effort: None,
        },
        overrides: CodexSubagentProfileOverrides::default(),
    };
    let mut pro = flash.clone();
    pro.key = "deepseek-v4-pro".into();
    pro.model = "deepseek-v4-pro".into();
    pro.strengths = vec![
        TaskStrength::ComplexDebugging,
        TaskStrength::ArchitectureDesign,
        TaskStrength::ComplexImplementation,
        TaskStrength::HighRiskReview,
        TaskStrength::Testing,
    ];
    pro.optimization = Optimization::Quality;
    pro.write_scope = WriteScope::ComplexChanges;
    Ok(CodexSubagentV2 {
        schema_version: 2,
        selection_policy: SelectionPolicy::Balanced,
        profiles: vec![
            ParsedProfileEntry::Valid(flash),
            ParsedProfileEntry::Valid(pro),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::providers::codex_reasoning::{
        ReasoningConfidence, ReasoningSupportKind, ResolvedSubagentReasoningCapability,
    };
    use std::collections::BTreeMap;

    fn s(value: &str) -> String {
        value.to_owned()
    }

    fn valid(profile: ParsedCodexSubagentProfile) -> ParsedProfileEntry {
        ParsedProfileEntry::Valid(profile)
    }

    fn profile(key: &str, model: &str) -> ParsedCodexSubagentProfile {
        ParsedCodexSubagentProfile {
            key: s(key),
            model: s(model),
            enabled: true,
            input_modalities: None,
            strengths: vec![TaskStrength::RepositoryExploration],
            optimization: Optimization::Speed,
            write_scope: WriteScope::ReadOnly,
            preference: Preference::Eligible,
            reasoning: CodexSubagentReasoningPolicy {
                policy: ReasoningRuntimePolicy::Delegated,
                effort: None,
            },
            overrides: CodexSubagentProfileOverrides::default(),
        }
    }

    fn config(
        selection_policy: SelectionPolicy,
        profiles: Vec<ParsedProfileEntry>,
    ) -> CodexSubagentV2 {
        CodexSubagentV2 {
            schema_version: 2,
            selection_policy,
            profiles,
        }
    }

    fn catalog(model: &str, routable: bool) -> CatalogModel {
        CatalogModel {
            model: s(model),
            provider_kind: ProviderKind::ThirdParty,
            routable,
            context_window: 1_000_000,
            reasoning: deepseek_reasoning(),
        }
    }

    fn deepseek_reasoning() -> ResolvedSubagentReasoningCapability {
        ResolvedSubagentReasoningCapability {
            support_kind: ReasoningSupportKind::EffortLevels,
            source: Some(s("builtin")),
            confidence: ReasoningConfidence::Confirmed,
            codex_selectable_efforts: vec![
                CodexReasoningEffort::None,
                CodexReasoningEffort::Low,
                CodexReasoningEffort::Medium,
                CodexReasoningEffort::High,
                CodexReasoningEffort::XHigh,
                CodexReasoningEffort::Max,
            ],
            provider_accepted_efforts: vec![
                CodexReasoningEffort::Low,
                CodexReasoningEffort::High,
                CodexReasoningEffort::Max,
            ],
            provider_default_effort: Some(CodexReasoningEffort::High),
            disable_allowed: true,
            effort_map: BTreeMap::from([
                (CodexReasoningEffort::Low, CodexReasoningEffort::Low),
                (CodexReasoningEffort::Medium, CodexReasoningEffort::High),
                (CodexReasoningEffort::High, CodexReasoningEffort::High),
                (CodexReasoningEffort::XHigh, CodexReasoningEffort::High),
                (CodexReasoningEffort::Max, CodexReasoningEffort::Max),
            ]),
        }
    }

    fn request(config: Option<CodexSubagentV2>) -> CompileRequest {
        CompileRequest {
            subagent_version: SubagentVersion::V2,
            persisted_subagent_v2: config,
            catalog_models: vec![catalog("DeepSeek-V4-Flash", true)],
            occupied_role_names: vec![],
        }
    }

    fn validation(code: &str, key: Option<&str>, detail: &str) -> CompileError {
        CompileError::Validation {
            code: s(code),
            profile_key: key.map(s),
            detail: s(detail),
        }
    }

    fn role(
        requested: &str,
        effective: &str,
        description: &str,
        instructions: &str,
        nicknames: Vec<String>,
        effort: impl Into<Option<CodexReasoningEffort>>,
    ) -> GeneratedRole {
        GeneratedRole {
            requested_role_name: s(requested),
            effective_role_name: s(effective),
            description: format!("{description}{UNKNOWN_MODALITY_DESCRIPTION_SAFETY}"),
            developer_instructions: format!("{instructions}{UNKNOWN_MODALITY_INSTRUCTIONS_SAFETY}"),
            nickname_candidates: nicknames,
            model: s("DeepSeek-V4-Flash"),
            model_provider: s("codex_model_router_v2"),
            effort: effort.into(),
            context_window: 1_000_000,
            warnings: Vec::new(),
        }
    }

    fn status(
        key: &str,
        model: Option<&str>,
        status: ProfileStatusCode,
        reason: Option<DiagnosticReasonCode>,
    ) -> ProfileStatus {
        ProfileStatus {
            key: s(key),
            model: model.map(s),
            status,
            reason,
        }
    }

    fn output(roles: Vec<GeneratedRole>, statuses: Vec<ProfileStatus>) -> CompileOutput {
        let diagnostics = statuses
            .iter()
            .filter(|status| status.status != ProfileStatusCode::Routable)
            .map(|status| Diagnostic {
                model: status.model.clone(),
                role: None,
                profile_key: None,
                policy: SelectionPolicy::Balanced,
                status: status.status,
                reason_code: status.reason,
            })
            .collect();
        CompileOutput {
            generated_roles: roles,
            profile_statuses: statuses,
            preserved_invalid_profiles: vec![],
            diagnostics,
            legacy_managed_roles_preserved: false,
        }
    }

    fn expected_routable_output(role: GeneratedRole) -> CompileOutput {
        output(
            vec![role],
            vec![status(
                "flash",
                Some("DeepSeek-V4-Flash"),
                ProfileStatusCode::Routable,
                None,
            )],
        )
    }

    // Independent literal fixtures for the production compiler contract.  These are intentionally
    // not derived through production helpers, so a compiler regression cannot rewrite its own oracle.
    const UNKNOWN_MODALITY_DESCRIPTION_SAFETY: &str = " Input capabilities are unknown, so this role must not be assigned tasks that depend on image understanding.";
    const UNKNOWN_MODALITY_INSTRUCTIONS_SAFETY: &str = " This model's input capabilities are unknown; do not select this role for tasks that depend on image understanding.";
    const DESC_BALANCED_REPOSITORY: &str = "This role matches delegated repository exploration tasks. It excludes long-context reading, evidence collection, summarization, complex debugging, architecture design, bounded implementation, complex implementation, testing, and high-risk review, and it does not own final integration, merging, or release. It optimizes for speed and is limited to read-only work. Under balanced selection, this eligible third-party profile has no provider bias.";
    const DESC_BALANCED_PREFERRED: &str = "This role matches delegated repository exploration tasks. It excludes long-context reading, evidence collection, summarization, complex debugging, architecture design, bounded implementation, complex implementation, testing, and high-risk review, and it does not own final integration, merging, or release. It optimizes for speed and is limited to read-only work. Under balanced selection, when declared task strengths match, prefer this specialized role over the built-in generic default, worker, and explorer roles; provider identity does not break ties otherwise.";
    const DESC_ARCHITECTURE: &str = "This role matches delegated architecture design tasks. It excludes long-context reading, repository exploration, evidence collection, summarization, complex debugging, bounded implementation, complex implementation, testing, and high-risk review, and it does not own final integration, merging, or release. It optimizes for quality and is limited to read-only work. Under balanced selection, this eligible third-party profile has no provider bias.";
    const DESC_TESTING: &str = "This role matches delegated testing tasks. It excludes long-context reading, repository exploration, evidence collection, summarization, complex debugging, architecture design, bounded implementation, complex implementation, and high-risk review, and it does not own final integration, merging, or release. It optimizes for speed and is limited to read-only work. Under balanced selection, this eligible third-party profile has no provider bias.";
    const DESC_OFFICIAL_FIRST_ELIGIBLE: &str = "This role matches delegated repository exploration tasks. It excludes long-context reading, evidence collection, summarization, complex debugging, architecture design, bounded implementation, complex implementation, testing, and high-risk review, and it does not own final integration, merging, or release. It optimizes for speed and is limited to read-only work. Under official-first selection, this eligible third-party profile may be chosen only for a strong, well-bounded task match; otherwise prefer an official provider path.";
    const DESC_THIRD_PARTY_FIRST_ELIGIBLE: &str = "This role matches delegated repository exploration tasks. It excludes long-context reading, evidence collection, summarization, complex debugging, architecture design, bounded implementation, complex implementation, testing, and high-risk review, and it does not own final integration, merging, or release. It optimizes for speed and is limited to read-only work. Under third-party-first selection, matching preferred or eligible third-party profiles go first; this profile is eligible.";
    const DESC_OFFICIAL_FIRST_PREFERRED: &str = "This role matches delegated repository exploration tasks. It excludes long-context reading, evidence collection, summarization, complex debugging, architecture design, bounded implementation, complex implementation, testing, and high-risk review, and it does not own final integration, merging, or release. It optimizes for speed and is limited to read-only work. Task match makes this preferred third-party profile override the global official-first provider bias.";
    const DESC_FALLBACK: &str = "This role matches delegated repository exploration tasks. It excludes long-context reading, evidence collection, summarization, complex debugging, architecture design, bounded implementation, complex implementation, testing, and high-risk review, and it does not own final integration, merging, or release. It optimizes for speed and is limited to read-only work. This third-party fallback profile is never promoted and may be used only when preferred profiles are unavailable or it is explicitly requested.";
    const INSTRUCTIONS_BALANCED_REPOSITORY: &str = "Work only on delegated repository exploration tasks and do not edit files. Optimize for speed and keep all work read-only. Under balanced selection, this eligible third-party profile has no provider bias. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_BALANCED_PREFERRED: &str = "Work only on delegated repository exploration tasks and do not edit files. Optimize for speed and keep all work read-only. Under balanced selection, when declared task strengths match, prefer this specialized role over the built-in generic default, worker, and explorer roles; provider identity does not break ties otherwise. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_ARCHITECTURE: &str = "Work only on delegated architecture design tasks and do not edit files. Optimize for quality and keep all work read-only. Under balanced selection, this eligible third-party profile has no provider bias. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_TESTING: &str = "Work only on delegated testing tasks and do not edit files. Optimize for speed and keep all work read-only. Under balanced selection, this eligible third-party profile has no provider bias. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_OFFICIAL_FIRST_ELIGIBLE: &str = "Work only on delegated repository exploration tasks and do not edit files. Optimize for speed and keep all work read-only. Under official-first selection, use this eligible third-party profile only for a strong, well-bounded task match; otherwise prefer an official provider path. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_THIRD_PARTY_FIRST_ELIGIBLE: &str = "Work only on delegated repository exploration tasks and do not edit files. Optimize for speed and keep all work read-only. Under third-party-first selection, put matching preferred or eligible third-party profiles first; this profile is eligible. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_OFFICIAL_FIRST_PREFERRED: &str = "Work only on delegated repository exploration tasks and do not edit files. Optimize for speed and keep all work read-only. Task match makes this preferred third-party profile override the global official-first provider bias. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_FALLBACK: &str = "Work only on delegated repository exploration tasks and do not edit files. Optimize for speed and keep all work read-only. Never promote this third-party fallback profile; use it only when preferred profiles are unavailable or it is explicitly requested. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_BOUNDED_CHANGES: &str = "Work only on delegated bounded implementation tasks. Optimize for balanced execution and edit only the files explicitly assigned to this role; do not expand ownership without parent approval. Under balanced selection, this eligible third-party profile has no provider bias. Return concrete evidence to the parent; final integration, merging, and release remain with the parent agent.";
    const INSTRUCTIONS_COMPLEX_CHANGES: &str = "Work only on delegated complex implementation tasks. Optimize for quality; cross-module changes are allowed within the delegated objective, but final integration, merging, and release remain with the parent agent. Under balanced selection, this eligible third-party profile has no provider bias. Return concrete evidence and verification to the parent agent.";

    fn assert_parse(raw: Value, expected: Result<CodexSubagentV2, CompileError>) {
        assert_eq!(parse_persisted_subagent_v2(&raw), expected);
    }

    fn assert_compile(request: &CompileRequest, expected: CompileResult) {
        assert_eq!(compile_subagent_v2_profiles(request), expected);
    }

    fn questionnaire() -> Value {
        json!({
            "taskStrengths": ["repository_exploration"],
            "optimization": "speed",
            "writeScope": "read_only",
            "preference": "eligible",
            "reasoningEffort": "auto"
        })
    }

    fn raw_profile_with_questionnaire(questionnaire: Value) -> Value {
        json!({
            "schemaVersion": 1,
            "profiles": {
                "flash": {
                    "model": "DeepSeek-V4-Flash",
                    "enabled": true,
                    "questionnaire": questionnaire
                }
            }
        })
    }

    fn raw_profile(strengths: Value) -> Value {
        let mut q = questionnaire();
        q["taskStrengths"] = strengths;
        raw_profile_with_questionnaire(q)
    }

    fn canonical_raw_profile(strengths: Value) -> Value {
        let mut raw = raw_profile(strengths);
        let profile = raw["profiles"]
            .as_object_mut()
            .expect("profiles fixture is an object")
            .remove("flash")
            .expect("flash fixture exists");
        raw["profiles"]["deepseek-v4-flash"] = profile;
        raw
    }

    fn legacy_reasoning_profile(
        questionnaire_effort: &str,
        override_effort: Option<&str>,
    ) -> Value {
        let mut raw = canonical_raw_profile(json!(["repository_exploration"]));
        raw["profiles"]["deepseek-v4-flash"]["questionnaire"]["reasoningEffort"] =
            json!(questionnaire_effort);
        if let Some(effort) = override_effort {
            raw["profiles"]["deepseek-v4-flash"]["overrides"] =
                json!({"modelReasoningEffort": effort});
        }
        raw
    }

    fn only_profile(config: &CodexSubagentV2) -> &ParsedCodexSubagentProfile {
        match config.profiles.as_slice() {
            [ParsedProfileEntry::Valid(profile)] => profile,
            profiles => panic!("expected one valid profile, got {profiles:?}"),
        }
    }

    fn fixed_reasoning(effort: CodexReasoningEffort) -> CodexSubagentReasoningPolicy {
        CodexSubagentReasoningPolicy {
            policy: ReasoningRuntimePolicy::Fixed,
            effort: Some(effort),
        }
    }

    #[test]
    fn legacy_auto_migrates_to_delegated_schema_v2() {
        let parsed = parse_persisted_subagent_v2(&legacy_reasoning_profile("auto", None))
            .expect("migrate legacy auto");
        assert_eq!(parsed.schema_version, 2);
        assert_eq!(
            only_profile(&parsed).reasoning,
            CodexSubagentReasoningPolicy {
                policy: ReasoningRuntimePolicy::Delegated,
                effort: None,
            }
        );
    }

    #[test]
    fn legacy_explicit_effort_migrates_to_fixed_schema_v2() {
        let parsed = parse_persisted_subagent_v2(&legacy_reasoning_profile("high", None))
            .expect("migrate legacy explicit effort");
        assert_eq!(
            only_profile(&parsed).reasoning,
            fixed_reasoning(CodexReasoningEffort::High)
        );
    }

    #[test]
    fn legacy_override_has_priority_during_schema_v2_migration() {
        let parsed = parse_persisted_subagent_v2(&legacy_reasoning_profile("auto", Some("xhigh")))
            .expect("migrate legacy override");
        assert_eq!(
            only_profile(&parsed).reasoning,
            fixed_reasoning(CodexReasoningEffort::XHigh)
        );
        let serialized = serde_json::to_value(&parsed).expect("serialize migrated profile");
        assert_eq!(serialized["schemaVersion"], 2);
        assert_eq!(
            serialized["profiles"]["deepseek-v4-flash"]["reasoning"],
            json!({"policy": "fixed", "effort": "xhigh"})
        );
        assert!(serialized["profiles"]["deepseek-v4-flash"]["questionnaire"]
            .get("reasoningEffort")
            .is_none());
        assert!(serialized["profiles"]["deepseek-v4-flash"]["overrides"]
            .get("modelReasoningEffort")
            .is_none());
    }

    fn raw_profile_missing_questionnaire_field(field: &str) -> Value {
        let mut q = questionnaire();
        q.as_object_mut()
            .expect("questionnaire fixture is an object")
            .remove(field);
        raw_profile_with_questionnaire(q)
    }

    fn expected_valid_profile_with_strengths(strengths: Vec<TaskStrength>) -> CodexSubagentV2 {
        let mut p = profile("deepseek-v4-flash", "DeepSeek-V4-Flash");
        p.strengths = strengths;
        config(SelectionPolicy::Balanced, vec![valid(p)])
    }

    fn generated_for_profile(p: ParsedCodexSubagentProfile, expected: GeneratedRole) {
        assert_compile(
            &request(Some(config(SelectionPolicy::Balanced, vec![valid(p)]))),
            Ok(expected_routable_output(expected)),
        );
    }

    #[test]
    fn codex_subagent_v2_defaults_only_missing_selection_policy() {
        assert_parse(
            json!({"schemaVersion": 1, "profiles": {}}),
            Ok(config(SelectionPolicy::Balanced, vec![])),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_missing_schema_version() {
        assert_parse(
            json!({"profiles": {}}),
            Err(validation(
                "missing_schema_version",
                None,
                "schemaVersion is required",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_schema_version_other_than_one_or_two() {
        assert_parse(
            json!({"schemaVersion": 3, "profiles": {}}),
            Err(validation(
                "unsupported_schema_version",
                None,
                "schemaVersion must be 1 or 2",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_invalid_selection_policy_enum() {
        assert_parse(
            json!({"schemaVersion": 1, "selectionPolicy": "fastest", "profiles": {}}),
            Err(validation(
                "invalid_selection_policy",
                None,
                "selectionPolicy is not an allowed enum member",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_invalid_optimization_enum() {
        let mut q = questionnaire();
        q["optimization"] = json!("fastest");
        assert_parse(
            raw_profile_with_questionnaire(q),
            Err(validation(
                "invalid_optimization",
                Some("flash"),
                "optimization is not an allowed enum member",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_invalid_write_scope_enum() {
        let mut q = questionnaire();
        q["writeScope"] = json!("unbounded");
        assert_parse(
            raw_profile_with_questionnaire(q),
            Err(validation(
                "invalid_write_scope",
                Some("flash"),
                "writeScope is not an allowed enum member",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_invalid_preference_enum() {
        let mut q = questionnaire();
        q["preference"] = json!("always");
        assert_parse(
            raw_profile_with_questionnaire(q),
            Err(validation(
                "invalid_preference",
                Some("flash"),
                "preference is not an allowed enum member",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_invalid_questionnaire_effort_enum() {
        let mut q = questionnaire();
        q["reasoningEffort"] = json!("max");
        assert_parse(
            raw_profile_with_questionnaire(q),
            Err(validation(
                "invalid_reasoning_effort",
                Some("flash"),
                "reasoningEffort is not an allowed enum member",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_auto_as_override_effort_enum() {
        let mut raw = raw_profile(json!(["repository_exploration"]));
        raw["profiles"]["flash"]["overrides"] = json!({"modelReasoningEffort": "auto"});
        assert_parse(
            raw,
            Err(validation(
                "invalid_override_effort",
                Some("flash"),
                "modelReasoningEffort allows only low, medium, high, xhigh, max, or ultra",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_missing_task_strengths() {
        assert_parse(
            raw_profile_missing_questionnaire_field("taskStrengths"),
            Err(validation(
                "missing_task_strengths",
                Some("flash"),
                "questionnaire.taskStrengths is required",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_missing_optimization() {
        assert_parse(
            raw_profile_missing_questionnaire_field("optimization"),
            Err(validation(
                "missing_optimization",
                Some("flash"),
                "questionnaire.optimization is required",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_missing_write_scope() {
        assert_parse(
            raw_profile_missing_questionnaire_field("writeScope"),
            Err(validation(
                "missing_write_scope",
                Some("flash"),
                "questionnaire.writeScope is required",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_missing_preference() {
        assert_parse(
            raw_profile_missing_questionnaire_field("preference"),
            Err(validation(
                "missing_preference",
                Some("flash"),
                "questionnaire.preference is required",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_missing_reasoning_effort() {
        assert_parse(
            raw_profile_missing_questionnaire_field("reasoningEffort"),
            Err(validation(
                "missing_reasoning_effort",
                Some("flash"),
                "questionnaire.reasoningEffort is required",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_round_trips_all_overrides() {
        let mut p = profile("deepseek-v4-flash", "DeepSeek-V4-Flash");
        p.overrides = CodexSubagentProfileOverrides {
            role_name: Some(s("flash-reader")),
            description: Some(s("Manual.")),
            developer_instructions: Some(s("Read only.")),
            nickname_candidates: Some(vec![s("Flash Reader")]),
        };
        p.reasoning = fixed_reasoning(CodexReasoningEffort::XHigh);
        assert_parse(
            json!({"schemaVersion":1,"profiles":{"deepseek-v4-flash":{"model":"DeepSeek-V4-Flash","enabled":true,"questionnaire":{"taskStrengths":["repository_exploration"],"optimization":"speed","writeScope":"read_only","preference":"eligible","reasoningEffort":"auto"},"overrides":{"roleName":"flash-reader","description":"Manual.","developerInstructions":"Read only.","nicknameCandidates":["Flash Reader"],"modelReasoningEffort":"xhigh"}}}}),
            Ok(config(SelectionPolicy::Balanced, vec![valid(p)])),
        );
    }

    #[test]
    fn generated_default_nickname_sanitizes_model_punctuation_for_codex_role_files() {
        let mut compile_request = request(Some(config(
            SelectionPolicy::Balanced,
            vec![valid(profile("qwen3.8", "qwen3.8"))],
        )));
        compile_request.catalog_models = vec![catalog("qwen3.8", true)];
        let output = compile_subagent_v2_profiles(&compile_request).expect("compile Qwen profile");
        let role = output.generated_roles.first().expect("generated Qwen role");
        assert_eq!(role.nickname_candidates, vec!["Qwen3 8"]);
        let toml = render_generated_role_toml(role, "# managed").expect("render Qwen role");
        assert!(toml.contains("nickname_candidates = [\"Qwen3 8\"]"));
        assert!(!toml.contains("nickname_candidates = [\"Qwen3.8\"]"));
    }

    #[test]
    fn generated_default_nickname_is_codex_valid_for_diverse_model_identifiers() {
        let cases = [
            ("gpt-4.1", "Gpt-4 1"),
            ("claude-3.7-sonnet", "Claude-3 7-sonnet"),
            ("qwen2.5-coder", "Qwen2 5-coder"),
            ("moonshot_v1.8", "Moonshot_v1 8"),
            ("vendor/model:1.0", "Vendor model 1 0"),
            ("模型.版本", "CCSwitch Worker"),
            ("...", "CCSwitch Worker"),
        ];

        for (model, expected) in cases {
            let nickname = sanitize_codex_nickname(model);
            assert_eq!(nickname, expected, "model={model}");
            assert!(is_valid_codex_nickname(&nickname), "model={model}");
        }
    }

    #[test]
    fn every_automatically_generated_nickname_satisfies_codex_role_grammar() {
        let models = [
            "qwen3.8",
            "gpt-4.1",
            "deepseek-v4.flash",
            "vendor/model:1.0",
            "...model",
        ];

        for model in models {
            let mut compile_request = request(Some(config(
                SelectionPolicy::Balanced,
                vec![valid(profile(model, model))],
            )));
            compile_request.catalog_models = vec![catalog(model, true)];
            let output = compile_subagent_v2_profiles(&compile_request)
                .unwrap_or_else(|error| panic!("compile model={model}: {error:?}"));
            let role = output
                .generated_roles
                .first()
                .unwrap_or_else(|| panic!("missing generated role for model={model}"));
            assert_eq!(role.nickname_candidates.len(), 1, "model={model}");
            assert!(
                is_valid_codex_nickname(&role.nickname_candidates[0]),
                "model={model}, nickname={}",
                role.nickname_candidates[0]
            );
            render_generated_role_toml(role, "# managed")
                .unwrap_or_else(|error| panic!("render model={model}: {error:?}"));
        }
    }

    #[test]
    fn codex_subagent_v2_text_only_capability_round_trips_and_guards_generated_copy() {
        let mut raw = canonical_raw_profile(json!(["repository_exploration"]));
        raw["profiles"]["deepseek-v4-flash"]["inputModalities"] = json!(["text"]);
        raw["profiles"]["deepseek-v4-flash"]["overrides"] = json!({
            "description": "Manual specialist description.",
            "developerInstructions": "Follow the delegated objective."
        });

        let parsed = parse_persisted_subagent_v2(&raw).expect("parse text-only capability");
        let round_trip = serde_json::to_value(&parsed).expect("serialize text-only capability");
        assert_eq!(
            round_trip["profiles"]["deepseek-v4-flash"]["inputModalities"],
            json!(["text"])
        );

        let compiled = compile_subagent_v2_profiles(&request(Some(parsed)))
            .expect("compile text-only capability");
        let role = &compiled.generated_roles[0];
        assert_eq!(
            role.description,
            "Manual specialist description. It accepts text input only and cannot inspect or understand images."
        );
        assert_eq!(
            role.developer_instructions,
            "Follow the delegated objective. This model does not support image input; do not select this role for tasks that depend on image understanding."
        );
    }

    #[test]
    fn codex_subagent_v2_multimodal_capability_round_trips_and_advertises_image_understanding() {
        let mut raw = canonical_raw_profile(json!(["repository_exploration"]));
        raw["profiles"]["deepseek-v4-flash"]["inputModalities"] = json!(["text", "image"]);

        let parsed = parse_persisted_subagent_v2(&raw).expect("parse multimodal capability");
        let round_trip = serde_json::to_value(&parsed).expect("serialize multimodal capability");
        assert_eq!(
            round_trip["profiles"]["deepseek-v4-flash"]["inputModalities"],
            json!(["text", "image"])
        );

        let compiled = compile_subagent_v2_profiles(&request(Some(parsed)))
            .expect("compile multimodal capability");
        let role = &compiled.generated_roles[0];
        assert!(role
            .description
            .ends_with("It supports text and image input, including image understanding."));
        assert!(role.developer_instructions.ends_with(
            "This model supports image input and may be selected for tasks that require image understanding."
        ));
    }

    #[test]
    fn codex_subagent_v2_unknown_input_modalities_guard_automatic_copy() {
        let compiled = compile_subagent_v2_profiles(&request(Some(config(
            SelectionPolicy::Balanced,
            vec![valid(profile("flash", "DeepSeek-V4-Flash"))],
        ))))
        .expect("compile profile with unknown input capabilities");
        let role = &compiled.generated_roles[0];

        assert!(role
            .description
            .ends_with(UNKNOWN_MODALITY_DESCRIPTION_SAFETY));
        assert!(role
            .developer_instructions
            .ends_with(UNKNOWN_MODALITY_INSTRUCTIONS_SAFETY));
    }

    #[test]
    fn codex_subagent_v2_unknown_input_modalities_guard_manual_overrides() {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.overrides.description = Some(s("Manual role selection guidance."));
        p.overrides.developer_instructions = Some(s("Follow the delegated objective."));
        let compiled = compile_subagent_v2_profiles(&request(Some(config(
            SelectionPolicy::Balanced,
            vec![valid(p)],
        ))))
        .expect("compile overridden profile with unknown input capabilities");
        let role = &compiled.generated_roles[0];

        assert_eq!(
            role.description,
            format!("Manual role selection guidance.{UNKNOWN_MODALITY_DESCRIPTION_SAFETY}")
        );
        assert_eq!(
            role.developer_instructions,
            format!("Follow the delegated objective.{UNKNOWN_MODALITY_INSTRUCTIONS_SAFETY}")
        );
    }

    #[test]
    fn codex_subagent_v2_public_serde_shape_is_keyed_and_nested() {
        let legacy = json!({
            "schemaVersion": 1,
            "selectionPolicy": "official_first",
            "profiles": {
                "deepseek-v4-flash": {
                    "model": "DeepSeek-V4-Flash",
                    "enabled": true,
                    "questionnaire": {
                        "taskStrengths": ["repository_exploration"],
                        "optimization": "speed",
                        "writeScope": "read_only",
                        "preference": "eligible",
                        "reasoningEffort": "auto"
                    }
                }
            }
        });
        let expected = json!({
            "schemaVersion": 2,
            "selectionPolicy": "official_first",
            "profiles": {
                "deepseek-v4-flash": {
                    "model": "DeepSeek-V4-Flash",
                    "enabled": true,
                    "questionnaire": {
                        "taskStrengths": ["repository_exploration"],
                        "optimization": "speed",
                        "writeScope": "read_only",
                        "preference": "eligible"
                    },
                    "reasoning": {"policy": "delegated"}
                }
            }
        });
        let parsed = parse_persisted_subagent_v2(&legacy).expect("strict public payload");
        assert_eq!(
            serde_json::to_value(parsed).expect("serialize public payload"),
            expected.clone(),
            "the persisted API is a keyed map and must not expose internal keys or flattened questionnaire fields"
        );
        let serde_parsed: CodexSubagentV2 =
            serde_json::from_value(expected.clone()).expect("deserialize aggregate public DTO");
        assert_eq!(
            serde_json::to_value(serde_parsed).expect("round-trip aggregate public DTO"),
            expected
        );
    }

    #[test]
    fn codex_subagent_profile_public_serde_shape_is_exactly_nested() {
        let profile = CodexSubagentProfileConfig {
            model: s("DeepSeek-V4-Flash"),
            enabled: true,
            input_modalities: None,
            questionnaire: CodexSubagentQuestionnaire {
                task_strengths: vec![TaskStrength::RepositoryExploration],
                optimization: Optimization::Speed,
                write_scope: WriteScope::ReadOnly,
                preference: Preference::Eligible,
            },
            reasoning: CodexSubagentReasoningPolicy {
                policy: ReasoningRuntimePolicy::Delegated,
                effort: None,
            },
            overrides: CodexSubagentProfileOverrides::default(),
        };
        let literal = json!({
            "model": "DeepSeek-V4-Flash",
            "enabled": true,
            "questionnaire": {
                "taskStrengths": ["repository_exploration"],
                "optimization": "speed",
                "writeScope": "read_only",
                "preference": "eligible"
            },
            "reasoning": {"policy": "delegated"}
        });
        assert_eq!(
            serde_json::to_value(&profile).expect("serialize public profile"),
            literal,
            "profile-alone serialization must use the same nested public DTO as the aggregate"
        );
        assert_eq!(
            serde_json::from_value::<CodexSubagentProfileConfig>(literal)
                .expect("deserialize public profile"),
            profile
        );
    }

    #[test]
    fn codex_subagent_v2_strict_parser_rejects_required_container_and_scalar_errors() {
        let cases = [
            (json!({"schemaVersion": 1}), "missing_profiles"),
            (
                json!({"schemaVersion": 1, "profiles": []}),
                "invalid_profiles",
            ),
            (
                json!({"schemaVersion": 1, "profiles": {"flash": {"enabled": true, "questionnaire": questionnaire()}}}),
                "missing_model",
            ),
            (
                json!({"schemaVersion": 1, "profiles": {"flash": {"model": "", "enabled": true, "questionnaire": questionnaire()}}}),
                "empty_model",
            ),
            (
                json!({"schemaVersion": 1, "profiles": {"flash": {"model": 7, "enabled": true, "questionnaire": questionnaire()}}}),
                "invalid_model",
            ),
            (
                json!({"schemaVersion": 1, "profiles": {"flash": {"model": "m", "questionnaire": questionnaire()}}}),
                "missing_enabled",
            ),
            (
                json!({"schemaVersion": 1, "profiles": {"flash": {"model": "m", "enabled": "yes", "questionnaire": questionnaire()}}}),
                "invalid_enabled",
            ),
            (
                json!({"schemaVersion": 1, "profiles": {"flash": {"model": "m", "enabled": true}}}),
                "missing_questionnaire",
            ),
            (
                json!({"schemaVersion": 1, "profiles": {"flash": {"model": "m", "enabled": true, "questionnaire": []}}}),
                "invalid_questionnaire",
            ),
            (
                json!({"schemaVersion": 1, "profiles": {"flash": false}}),
                "invalid_profile",
            ),
        ];
        for (raw, expected_code) in cases {
            let actual = parse_persisted_subagent_v2(&raw);
            assert!(
                matches!(actual, Err(CompileError::Validation { ref code, .. }) if code == expected_code),
                "expected {expected_code}, got {actual:?}"
            );
        }
    }

    #[test]
    fn codex_subagent_v2_tolerant_loader_requires_profiles_but_preserves_bad_entries() {
        assert!(matches!(
            parse_persisted_subagent_v2_tolerant(&json!({"schemaVersion": 1})),
            Err(CompileError::Validation { ref code, .. }) if code == "missing_profiles"
        ));
        let raw = json!({
            "schemaVersion": 1,
            "profiles": {
                "bad-model": {"model": 7, "enabled": true, "questionnaire": questionnaire()},
                "bad-enabled": {"model": "m", "enabled": "yes", "questionnaire": questionnaire()}
            }
        });
        let parsed =
            parse_persisted_subagent_v2_tolerant(&raw).expect("preserve malformed entries");
        assert_eq!(parsed.profiles.len(), 2);
        assert!(parsed
            .profiles
            .iter()
            .all(|entry| matches!(entry, ParsedProfileEntry::Invalid { .. })));
    }

    #[test]
    fn codex_subagent_v2_requires_each_profile_key_to_match_its_canonical_model() {
        let raw = json!({
            "schemaVersion": 1,
            "profiles": {
                "friendly-alias": {
                    "model": " DeepSeek-V4-Flash ",
                    "enabled": true,
                    "questionnaire": questionnaire()
                }
            }
        });

        let actual = parse_persisted_subagent_v2(&raw);

        assert!(
            matches!(actual, Err(CompileError::Validation { ref code, ref profile_key, .. })
                if code == "profile_key_model_mismatch"
                    && profile_key.as_deref() == Some("friendly-alias")),
            "persisted map keys must equal normalize_profile_key(profile.model), got {actual:?}"
        );
    }

    #[test]
    fn codex_subagent_v2_tolerant_loader_isolates_duplicate_canonical_models_under_unrelated_keys()
    {
        let profile = json!({
            "model": "DeepSeek-V4-Flash",
            "enabled": true,
            "questionnaire": questionnaire()
        });
        let raw = json!({
            "schemaVersion": 1,
            "profiles": {
                "first-alias": profile.clone(),
                "second-alias": profile
            }
        });

        let parsed = parse_persisted_subagent_v2_tolerant(&raw)
            .expect("tolerant status loading must preserve both bad entries");

        assert_eq!(parsed.profiles.len(), 2);
        assert!(parsed.profiles.iter().all(|entry| matches!(
            entry,
            ParsedProfileEntry::Invalid { validation_code, .. }
                if validation_code == "profile_key_model_mismatch"
        )));
        let compiled = compile_subagent_v2_profiles(&request(Some(parsed)))
            .expect("isolated invalid profiles must remain status-readable");
        assert_eq!(
            compiled
                .profile_statuses
                .iter()
                .map(|status| status.status)
                .collect::<Vec<_>>(),
            vec![ProfileStatusCode::Collision, ProfileStatusCode::Collision],
            "tolerant compilation must group isolated entries by extractable canonical model identity"
        );
        assert_eq!(compiled.preserved_invalid_profiles.len(), 2);
        assert!(compiled.generated_roles.is_empty());
        let diagnostics = serde_json::to_value(&compiled.diagnostics)
            .expect("serialize collision diagnostics without raw identities");
        assert_eq!(diagnostics[0]["status"], "collision");
        assert_eq!(diagnostics[1]["status"], "collision");
        assert_eq!(diagnostics[0]["profileKey"], Value::Null);
        assert_eq!(diagnostics[1]["profileKey"], Value::Null);
        let public_diagnostics = diagnostics.to_string();
        assert!(!public_diagnostics.contains("first-alias"));
        assert!(!public_diagnostics.contains("second-alias"));
    }

    #[test]
    fn codex_subagent_v2_rejects_zero_task_strengths() {
        assert_parse(
            raw_profile(json!([])),
            Err(validation(
                "strength_count",
                Some("flash"),
                "taskStrengths must contain 1 through 5 members",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_accepts_one_task_strength() {
        assert_parse(
            canonical_raw_profile(json!(["testing"])),
            Ok(expected_valid_profile_with_strengths(vec![
                TaskStrength::Testing,
            ])),
        );
    }

    #[test]
    fn codex_subagent_v2_accepts_five_unique_task_strengths() {
        assert_parse(
            canonical_raw_profile(json!([
                "long_context_reading",
                "repository_exploration",
                "evidence_collection",
                "summarization",
                "testing"
            ])),
            Ok(expected_valid_profile_with_strengths(vec![
                TaskStrength::LongContextReading,
                TaskStrength::RepositoryExploration,
                TaskStrength::EvidenceCollection,
                TaskStrength::Summarization,
                TaskStrength::Testing,
            ])),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_six_task_strengths() {
        assert_parse(
            raw_profile(json!([
                "long_context_reading",
                "repository_exploration",
                "evidence_collection",
                "summarization",
                "testing",
                "architecture_design"
            ])),
            Err(validation(
                "strength_count",
                Some("flash"),
                "taskStrengths must contain 1 through 5 members",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_duplicate_task_strength() {
        assert_parse(
            raw_profile(json!(["testing", "testing"])),
            Err(validation(
                "duplicate_task_strength",
                Some("flash"),
                "taskStrengths members must be unique",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_unknown_task_strength() {
        assert_parse(
            raw_profile(json!(["unknown"])),
            Err(validation(
                "unknown_task_strength",
                Some("flash"),
                "taskStrengths contains an unknown enum member",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_nfkc_collision_rejects_all_and_keeps_models() {
        let a = profile("Ｆｏｏ", "Ｆｏｏ");
        let b = profile("foo", "foo");
        assert_compile(
            &request(Some(config(
                SelectionPolicy::Balanced,
                vec![valid(a), valid(b)],
            ))),
            Ok(output(
                vec![],
                vec![
                    status(
                        "Ｆｏｏ",
                        Some("Ｆｏｏ"),
                        ProfileStatusCode::Collision,
                        Some(DiagnosticReasonCode::Collision),
                    ),
                    status(
                        "foo",
                        Some("foo"),
                        ProfileStatusCode::Collision,
                        Some(DiagnosticReasonCode::Collision),
                    ),
                ],
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_default_case_fold_collision_keeps_models() {
        assert_compile(
            &request(Some(config(
                SelectionPolicy::Balanced,
                vec![
                    valid(profile("Straße", "Straße")),
                    valid(profile("STRASSE", "STRASSE")),
                ],
            ))),
            Ok(output(
                vec![],
                vec![
                    status(
                        "Straße",
                        Some("Straße"),
                        ProfileStatusCode::Collision,
                        Some(DiagnosticReasonCode::Collision),
                    ),
                    status(
                        "STRASSE",
                        Some("STRASSE"),
                        ProfileStatusCode::Collision,
                        Some(DiagnosticReasonCode::Collision),
                    ),
                ],
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_collision_counts_valid_and_invalid_raw_keys() {
        let raw = json!({"model": 7, "enabled": true, "questionnaire": questionnaire()});
        let mut valid_profile = profile("Straße", "Straße");
        valid_profile.overrides.nickname_candidates = Some(vec![s("Street")]);
        let saved = config(
            SelectionPolicy::Balanced,
            vec![
                valid(valid_profile),
                ParsedProfileEntry::Invalid {
                    key: s("STRASSE"),
                    raw: raw.clone(),
                    validation_code: s("invalid_model"),
                },
            ],
        );
        let actual =
            compile_subagent_v2_profiles(&request(Some(saved))).expect("controlled collision");
        assert!(actual.generated_roles.is_empty());
        assert_eq!(
            actual
                .profile_statuses
                .iter()
                .map(|status| status.status)
                .collect::<Vec<_>>(),
            vec![ProfileStatusCode::Collision, ProfileStatusCode::Collision]
        );
        assert_eq!(actual.preserved_invalid_profiles, vec![raw]);
    }

    #[test]
    fn codex_subagent_v2_collision_counts_two_invalid_raw_keys() {
        let raw_a = json!({"model": 1});
        let raw_b = json!({"enabled": "yes"});
        let saved = config(
            SelectionPolicy::Balanced,
            vec![
                ParsedProfileEntry::Invalid {
                    key: s("Ｆｏｏ"),
                    raw: raw_a.clone(),
                    validation_code: s("invalid_model"),
                },
                ParsedProfileEntry::Invalid {
                    key: s("foo"),
                    raw: raw_b.clone(),
                    validation_code: s("invalid_enabled"),
                },
            ],
        );
        let actual =
            compile_subagent_v2_profiles(&request(Some(saved))).expect("controlled collision");
        assert_eq!(
            actual
                .profile_statuses
                .iter()
                .map(|status| status.status)
                .collect::<Vec<_>>(),
            vec![ProfileStatusCode::Collision, ProfileStatusCode::Collision]
        );
        assert_eq!(actual.preserved_invalid_profiles, vec![raw_a, raw_b]);
    }

    #[test]
    fn codex_subagent_v2_generated_copy_covers_every_questionnaire_dimension() {
        let strengths = [
            (TaskStrength::LongContextReading, "long-context reading"),
            (
                TaskStrength::RepositoryExploration,
                "repository exploration",
            ),
            (TaskStrength::EvidenceCollection, "evidence collection"),
            (TaskStrength::Summarization, "summarization"),
            (TaskStrength::ComplexDebugging, "complex debugging"),
            (TaskStrength::ArchitectureDesign, "architecture design"),
            (
                TaskStrength::BoundedImplementation,
                "bounded implementation",
            ),
            (
                TaskStrength::ComplexImplementation,
                "complex implementation",
            ),
            (TaskStrength::Testing, "testing"),
            (TaskStrength::HighRiskReview, "high-risk review"),
        ];
        for (strength, phrase) in strengths {
            let mut p = profile("flash", "DeepSeek-V4-Flash");
            p.strengths = vec![strength];
            let description = generated_description_for_provider(
                SelectionPolicy::Balanced,
                &p,
                ProviderKind::ThirdParty,
            );
            let instructions = generated_instructions_for_provider(
                SelectionPolicy::Balanced,
                &p,
                ProviderKind::ThirdParty,
            );
            assert!(
                description.to_ascii_lowercase().contains(phrase),
                "description missing {phrase}: {description}"
            );
            assert!(
                instructions.to_ascii_lowercase().contains(phrase),
                "instructions missing {phrase}: {instructions}"
            );
            assert!(
                (2..=5).contains(&description.matches('.').count()),
                "description must remain 2-5 sentences including modality safety: {description}"
            );
        }
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.optimization = Optimization::Quality;
        p.write_scope = WriteScope::BoundedChanges;
        p.preference = Preference::Preferred;
        let description = generated_description_for_provider(
            SelectionPolicy::OfficialFirst,
            &p,
            ProviderKind::ThirdParty,
        );
        assert!(description.contains("quality"));
        assert!(description.contains("bounded changes"));
        assert!(description.contains("preferred"));
        assert!(description.contains("official-first"));
    }

    #[test]
    fn codex_subagent_v2_generated_copy_reflects_selected_provider_kind() {
        let saved = config(
            SelectionPolicy::Balanced,
            vec![valid(profile("flash", "DeepSeek-V4-Flash"))],
        );
        let mut req = request(Some(saved));
        req.catalog_models[0].provider_kind = ProviderKind::Official;
        let role = compile_subagent_v2_profiles(&req)
            .expect("compile official profile")
            .generated_roles
            .into_iter()
            .next()
            .expect("generated role");
        assert!(role.description.to_ascii_lowercase().contains("official"));
        assert!(role
            .developer_instructions
            .to_ascii_lowercase()
            .contains("official"));
    }

    #[test]
    fn codex_subagent_v2_write_scope_instructions_have_exact_ownership_boundaries() {
        let mut bounded = profile("flash", "DeepSeek-V4-Flash");
        bounded.strengths = vec![TaskStrength::BoundedImplementation];
        bounded.optimization = Optimization::Balanced;
        bounded.write_scope = WriteScope::BoundedChanges;
        assert_eq!(
            generated_instructions_for_provider(
                SelectionPolicy::Balanced,
                &bounded,
                ProviderKind::ThirdParty,
            ),
            format!("{INSTRUCTIONS_BOUNDED_CHANGES}{UNKNOWN_MODALITY_INSTRUCTIONS_SAFETY}")
        );

        let mut complex = profile("flash", "DeepSeek-V4-Flash");
        complex.strengths = vec![TaskStrength::ComplexImplementation];
        complex.optimization = Optimization::Quality;
        complex.write_scope = WriteScope::ComplexChanges;
        assert_eq!(
            generated_instructions_for_provider(
                SelectionPolicy::Balanced,
                &complex,
                ProviderKind::ThirdParty,
            ),
            format!("{INSTRUCTIONS_COMPLEX_CHANGES}{UNKNOWN_MODALITY_INSTRUCTIONS_SAFETY}")
        );
    }

    fn effort_profile(
        strength: TaskStrength,
        optimization: Optimization,
        override_effort: Option<ModelReasoningEffort>,
    ) -> ParsedCodexSubagentProfile {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.strengths = vec![strength];
        p.optimization = optimization;
        p.reasoning = match override_effort {
            Some(effort) => fixed_reasoning(effort),
            None => CodexSubagentReasoningPolicy {
                policy: ReasoningRuntimePolicy::Delegated,
                effort: None,
            },
        };
        p
    }

    fn reasoning_policy_toml(policy: CodexSubagentReasoningPolicy) -> Result<String, CompileError> {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.reasoning = policy;
        let output = compile_subagent_v2_profiles(&request(Some(config(
            SelectionPolicy::Balanced,
            vec![valid(p)],
        ))))?;
        let role = output
            .generated_roles
            .first()
            .expect("reasoning policy should generate one role");
        render_generated_role_toml(role, "# managed")
    }

    #[test]
    fn reasoning_policy_delegated_omits_role_effort_for_pinned_model() {
        let toml = reasoning_policy_toml(CodexSubagentReasoningPolicy {
            policy: ReasoningRuntimePolicy::Delegated,
            effort: None,
        })
        .expect("delegated role TOML");
        assert!(!toml.contains("model_reasoning_effort"));
    }

    #[test]
    fn reasoning_policy_model_default_pins_resolved_catalog_default() {
        let toml = reasoning_policy_toml(CodexSubagentReasoningPolicy {
            policy: ReasoningRuntimePolicy::ModelDefault,
            effort: None,
        })
        .expect("model-default role TOML");
        assert!(toml.contains("model_reasoning_effort = \"high\""));
    }

    #[test]
    fn reasoning_policy_fixed_max_round_trips_exact_role_toml() {
        let toml = reasoning_policy_toml(fixed_reasoning(CodexReasoningEffort::Max))
            .expect("fixed max role TOML");
        let parsed: toml::Value = toml::from_str(&toml).expect("parse fixed max role TOML");
        assert_eq!(
            parsed
                .get("model_reasoning_effort")
                .and_then(toml::Value::as_str),
            Some("max")
        );
    }

    #[test]
    fn reasoning_policy_disabled_writes_none_when_capability_allows_disable() {
        let toml = reasoning_policy_toml(CodexSubagentReasoningPolicy {
            policy: ReasoningRuntimePolicy::Disabled,
            effort: None,
        })
        .expect("disabled role TOML");
        assert!(toml.contains("model_reasoning_effort = \"none\""));
    }

    #[test]
    fn reasoning_policy_rejects_fixed_effort_absent_from_selectable_catalog() {
        assert_eq!(
            reasoning_policy_toml(fixed_reasoning(CodexReasoningEffort::Ultra)),
            Err(validation(
                "unsupported_reasoning_effort",
                Some("flash"),
                "fixed reasoning effort is not supported by the target model",
            ))
        );
    }

    #[test]
    fn codex_subagent_v2_delegated_effort_is_not_selected_from_complex_strength() {
        generated_for_profile(
            effort_profile(
                TaskStrength::ArchitectureDesign,
                Optimization::Quality,
                None,
            ),
            role(
                "flash",
                "flash",
                DESC_ARCHITECTURE,
                INSTRUCTIONS_ARCHITECTURE,
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ),
        );
    }

    #[test]
    fn codex_subagent_v2_delegated_effort_is_not_selected_from_read_only_strengths() {
        generated_for_profile(
            effort_profile(
                TaskStrength::RepositoryExploration,
                Optimization::Speed,
                None,
            ),
            role(
                "flash",
                "flash",
                DESC_BALANCED_REPOSITORY,
                INSTRUCTIONS_BALANCED_REPOSITORY,
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ),
        );
    }

    #[test]
    fn codex_subagent_v2_fixed_max_round_trips_into_role_toml() {
        let generated = role(
            "flash",
            "flash",
            DESC_ARCHITECTURE,
            INSTRUCTIONS_ARCHITECTURE,
            vec![s("Flash")],
            ModelReasoningEffort::Max,
        );
        generated_for_profile(
            effort_profile(
                TaskStrength::ArchitectureDesign,
                Optimization::Quality,
                Some(ModelReasoningEffort::Max),
            ),
            generated.clone(),
        );
        let toml =
            render_generated_role_toml(&generated, "# managed").expect("fixed max role TOML");
        let parsed: toml::Value = toml::from_str(&toml).expect("parse fixed max role TOML");
        assert_eq!(
            parsed
                .get("model_reasoning_effort")
                .and_then(toml::Value::as_str),
            Some("max")
        );
    }

    #[test]
    fn codex_subagent_v2_delegated_effort_is_not_selected_from_testing_strength() {
        generated_for_profile(
            effort_profile(TaskStrength::Testing, Optimization::Speed, None),
            role(
                "flash",
                "flash",
                DESC_TESTING,
                INSTRUCTIONS_TESTING,
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ),
        );
    }

    #[test]
    fn codex_subagent_v2_fixed_effort_is_explicit() {
        generated_for_profile(
            effort_profile(
                TaskStrength::ArchitectureDesign,
                Optimization::Quality,
                Some(ModelReasoningEffort::XHigh),
            ),
            role(
                "flash",
                "flash",
                DESC_ARCHITECTURE,
                INSTRUCTIONS_ARCHITECTURE,
                vec![s("Flash")],
                ModelReasoningEffort::XHigh,
            ),
        );
    }

    fn policy_profile(preference: Preference) -> ParsedCodexSubagentProfile {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.preference = preference;
        p
    }

    fn assert_policy(
        selection_policy: SelectionPolicy,
        preference: Preference,
        description: &str,
        developer_instructions: &str,
    ) {
        assert_compile(
            &request(Some(config(
                selection_policy,
                vec![valid(policy_profile(preference))],
            ))),
            Ok(expected_routable_output(role(
                "flash",
                "flash",
                description,
                developer_instructions,
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ))),
        );
    }

    #[test]
    fn codex_subagent_v2_balanced_policy_adds_no_provider_bias() {
        assert_policy(
            SelectionPolicy::Balanced,
            Preference::Eligible,
            DESC_BALANCED_REPOSITORY,
            INSTRUCTIONS_BALANCED_REPOSITORY,
        );
    }

    #[test]
    fn codex_subagent_v2_balanced_preferred_profile_outranks_builtin_generic_roles() {
        assert_policy(
            SelectionPolicy::Balanced,
            Preference::Preferred,
            DESC_BALANCED_PREFERRED,
            INSTRUCTIONS_BALANCED_PREFERRED,
        );
    }

    #[test]
    fn codex_subagent_v2_official_first_policy_keeps_high_risk_work_official() {
        assert_policy(
            SelectionPolicy::OfficialFirst,
            Preference::Eligible,
            DESC_OFFICIAL_FIRST_ELIGIBLE,
            INSTRUCTIONS_OFFICIAL_FIRST_ELIGIBLE,
        );
    }

    #[test]
    fn codex_subagent_v2_third_party_first_policy_promotes_eligible_profile() {
        assert_policy(
            SelectionPolicy::ThirdPartyFirst,
            Preference::Eligible,
            DESC_THIRD_PARTY_FIRST_ELIGIBLE,
            INSTRUCTIONS_THIRD_PARTY_FIRST_ELIGIBLE,
        );
    }

    #[test]
    fn codex_subagent_v2_preferred_profile_overrides_official_provider_bias() {
        assert_policy(
            SelectionPolicy::OfficialFirst,
            Preference::Preferred,
            DESC_OFFICIAL_FIRST_PREFERRED,
            INSTRUCTIONS_OFFICIAL_FIRST_PREFERRED,
        );
    }

    #[test]
    fn codex_subagent_v2_fallback_profile_is_never_promoted() {
        assert_policy(
            SelectionPolicy::ThirdPartyFirst,
            Preference::Fallback,
            DESC_FALLBACK,
            INSTRUCTIONS_FALLBACK,
        );
    }

    #[test]
    fn codex_subagent_v2_manual_description_fully_replaces_policy_text() {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.overrides.description = Some(s("Manual selection text only."));
        generated_for_profile(
            p,
            role(
                "flash",
                "flash",
                "Manual selection text only.",
                INSTRUCTIONS_BALANCED_REPOSITORY,
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ),
        );
    }

    #[test]
    fn codex_subagent_v2_restoring_description_keeps_other_override() {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.overrides.developer_instructions = Some(s("Keep this override."));
        generated_for_profile(
            p,
            role(
                "flash",
                "flash",
                DESC_BALANCED_REPOSITORY,
                "Keep this override.",
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ),
        );
    }

    fn role_name_request(role_name: &str) -> CompileRequest {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.overrides.role_name = Some(s(role_name));
        request(Some(config(SelectionPolicy::Balanced, vec![valid(p)])))
    }

    #[test]
    fn codex_subagent_v2_normalizes_mixed_role_name_separators() {
        assert_compile(
            &role_name_request("Foo__-- Bar"),
            Ok(expected_routable_output(role(
                "Foo__-- Bar",
                "foo-bar",
                DESC_BALANCED_REPOSITORY,
                INSTRUCTIONS_BALANCED_REPOSITORY,
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ))),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_empty_normalized_role_name() {
        assert_compile(
            &role_name_request("深度模型!!!"),
            Err(validation(
                "empty_role_name",
                Some("flash"),
                "roleName is empty after ASCII normalization",
            )),
        );
    }

    fn assert_builtin_role_rejected(role_name: &str) {
        assert_compile(
            &role_name_request(role_name),
            Err(validation(
                "reserved_role_name",
                Some("flash"),
                "normalized roleName conflicts with a built-in role",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_builtin_default_role_name() {
        assert_builtin_role_rejected(" DEFAULT ");
    }

    #[test]
    fn codex_subagent_v2_rejects_builtin_worker_role_name() {
        assert_builtin_role_rejected("Worker");
    }

    #[test]
    fn codex_subagent_v2_rejects_builtin_explorer_role_name() {
        assert_builtin_role_rejected("explorer");
    }

    #[test]
    fn codex_subagent_v2_resolves_case_insensitive_occupied_role_names_in_order() {
        let mut request = role_name_request("Review");
        request.occupied_role_names =
            vec![s("REVIEW"), s("CcSwitch-Review"), s("CCSWITCH-REVIEW-2")];
        assert_compile(
            &request,
            Ok(expected_routable_output(role(
                "Review",
                "ccswitch-review-3",
                DESC_BALANCED_REPOSITORY,
                INSTRUCTIONS_BALANCED_REPOSITORY,
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ))),
        );
    }

    fn nicknames(values: Vec<&str>) -> CompileRequest {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.overrides.nickname_candidates = Some(values.into_iter().map(s).collect());
        request(Some(config(SelectionPolicy::Balanced, vec![valid(p)])))
    }

    fn expected_nicknames(values: Vec<&str>) -> CompileOutput {
        expected_routable_output(role(
            "flash",
            "flash",
            DESC_BALANCED_REPOSITORY,
            INSTRUCTIONS_BALANCED_REPOSITORY,
            values.into_iter().map(s).collect(),
            Option::<CodexReasoningEffort>::None,
        ))
    }

    #[test]
    fn codex_subagent_v2_rejects_zero_nicknames() {
        assert_compile(
            &nicknames(vec![]),
            Err(validation(
                "nickname_count",
                Some("flash"),
                "nicknameCandidates must contain 1 through 3 entries",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_accepts_one_nickname() {
        assert_compile(&nicknames(vec!["One"]), Ok(expected_nicknames(vec!["One"])));
    }

    #[test]
    fn codex_subagent_v2_accepts_three_nicknames() {
        assert_compile(
            &nicknames(vec!["One", "Two", "Three"]),
            Ok(expected_nicknames(vec!["One", "Two", "Three"])),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_four_nicknames() {
        assert_compile(
            &nicknames(vec!["One", "Two", "Three", "Four"]),
            Err(validation(
                "nickname_count",
                Some("flash"),
                "nicknameCandidates must contain 1 through 3 entries",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_empty_nickname() {
        assert_compile(
            &nicknames(vec![""]),
            Err(validation(
                "empty_nickname",
                Some("flash"),
                "nickname must be nonempty",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_whitespace_only_nickname() {
        let mut request = nicknames(vec!["   "]);
        request.catalog_models = vec![catalog("unrelated-model", true)];
        assert_compile(
            &request,
            Err(validation(
                "empty_nickname",
                Some("flash"),
                "nickname must contain non-whitespace characters",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_all_empty_description_variants_before_early_returns() {
        let cases = [
            (SubagentVersion::V1, true, ""),
            (SubagentVersion::V1, true, " \t\r\n "),
            (SubagentVersion::V2, false, ""),
            (SubagentVersion::V2, false, " \t\r\n "),
        ];
        let actual = cases
            .into_iter()
            .map(|(version, enabled, description)| {
                let mut profile = profile("flash", "DeepSeek-V4-Flash");
                profile.enabled = enabled;
                profile.overrides.description = Some(s(description));
                let mut request = request(Some(config(
                    SelectionPolicy::Balanced,
                    vec![valid(profile)],
                )));
                request.subagent_version = version;
                compile_subagent_v2_profiles(&request)
            })
            .collect::<Vec<_>>();

        assert_eq!(actual.len(), 4);
        assert!(
            actual.iter().all(|result| matches!(
                result,
                Err(CompileError::Validation { code, profile_key, .. })
                    if code == "empty_description"
                        && profile_key.as_deref() == Some("flash")
            )),
            "empty and whitespace description overrides must both fail in V1 and disabled V2 before early returns, got {actual:#?}"
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_all_empty_developer_instruction_variants_before_early_returns() {
        let cases = [
            (SubagentVersion::V1, true, ""),
            (SubagentVersion::V1, true, " \t\r\n "),
            (SubagentVersion::V2, false, ""),
            (SubagentVersion::V2, false, " \t\r\n "),
        ];
        let actual = cases
            .into_iter()
            .map(|(version, enabled, instructions)| {
                let mut profile = profile("flash", "DeepSeek-V4-Flash");
                profile.enabled = enabled;
                profile.overrides.developer_instructions = Some(s(instructions));
                let mut request = request(Some(config(
                    SelectionPolicy::Balanced,
                    vec![valid(profile)],
                )));
                request.subagent_version = version;
                compile_subagent_v2_profiles(&request)
            })
            .collect::<Vec<_>>();

        assert_eq!(actual.len(), 4);
        assert!(
            actual.iter().all(|result| matches!(
                result,
                Err(CompileError::Validation { code, profile_key, .. })
                    if code == "empty_developer_instructions"
                        && profile_key.as_deref() == Some("flash")
            )),
            "empty and whitespace developerInstructions overrides must both fail in V1 and disabled V2 before early returns, got {actual:#?}"
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_duplicate_nickname() {
        assert_compile(
            &nicknames(vec!["Dup", "Dup"]),
            Err(validation(
                "duplicate_nickname",
                Some("flash"),
                "nicknameCandidates must be unique",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_rejects_non_ascii_nickname_character() {
        assert_compile(
            &nicknames(vec!["Bad!"]),
            Err(validation(
                "invalid_nickname",
                Some("flash"),
                "nickname uses only ASCII alphanumeric, space, dash, underscore",
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_invalid_raw_profile_is_preserved_but_not_generated() {
        let raw = json!({"model":"broken","enabled":"yes","questionnaire":false});
        let saved = config(
            SelectionPolicy::Balanced,
            vec![ParsedProfileEntry::Invalid {
                key: s("broken"),
                raw: raw.clone(),
                validation_code: s("invalid_enabled"),
            }],
        );
        let mut expected = output(
            vec![],
            vec![status(
                "",
                None,
                ProfileStatusCode::Invalid,
                Some(DiagnosticReasonCode::Invalid),
            )],
        );
        expected.preserved_invalid_profiles = vec![raw];
        assert_compile(&request(Some(saved)), Ok(expected));
    }

    #[test]
    fn codex_subagent_v2_production_diagnostics_are_allowlisted() {
        let raw = json!({
            "model": "MODEL_SECRET_MARKER",
            "enabled": "yes",
            "apiKey": "API_KEY_SECRET",
            "taskBody": "TASK_BODY_SECRET",
            "encryptedContent": "ENCRYPTED_SECRET"
        });
        let saved = config(
            SelectionPolicy::Balanced,
            vec![ParsedProfileEntry::Invalid {
                key: s("PROFILE_KEY_SECRET_MARKER"),
                raw,
                validation_code: s("invalid_enabled"),
            }],
        );
        let output = compile_subagent_v2_profiles(&request(Some(saved)))
            .expect("compile invalid profile status");
        let value = serde_json::to_value(&output.diagnostics).expect("serialize diagnostics");
        let object = value[0].as_object().expect("diagnostic object");
        assert_eq!(
            object
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            [
                "model",
                "role",
                "profileKey",
                "policy",
                "status",
                "reasonCode"
            ]
            .into_iter()
            .collect()
        );
        let serialized = value.to_string();
        assert!(!serialized.contains("API_KEY_SECRET"));
        assert!(!serialized.contains("TASK_BODY_SECRET"));
        assert!(!serialized.contains("ENCRYPTED_SECRET"));
        assert!(!serialized.contains("MODEL_SECRET_MARKER"));
        assert!(!serialized.contains("PROFILE_KEY_SECRET_MARKER"));
        assert_eq!(value[0]["model"], Value::Null);
        assert_eq!(value[0]["role"], Value::Null);
        assert_eq!(value[0]["profileKey"], Value::Null);
    }

    #[test]
    fn codex_subagent_v2_v1_preserves_profiles_without_materializing_v2_roles() {
        let saved = config(
            SelectionPolicy::Balanced,
            vec![valid(profile("flash", "DeepSeek-V4-Flash"))],
        );
        let mut request = request(Some(saved.clone()));
        request.subagent_version = SubagentVersion::V1;
        let actual = compile_subagent_v2_profiles(&request);
        assert_eq!(request.persisted_subagent_v2, Some(saved));
        assert_eq!(
            actual,
            Ok(output(
                vec![],
                vec![status(
                    "flash",
                    Some("DeepSeek-V4-Flash"),
                    ProfileStatusCode::InactiveV1,
                    Some(DiagnosticReasonCode::InactiveV1),
                )],
            ))
        );
    }

    #[test]
    fn codex_subagent_v2_catalog_alias_change_preserves_profile_and_marks_it_unroutable() {
        let saved = config(
            SelectionPolicy::Balanced,
            vec![valid(profile("flash", "DeepSeek-V4-Flash"))],
        );
        let mut request = request(Some(saved.clone()));
        request.catalog_models = vec![catalog("deepseek-flash-alias", true)];
        let actual = compile_subagent_v2_profiles(&request);
        assert_eq!(request.persisted_subagent_v2, Some(saved));
        assert_eq!(
            actual,
            Ok(output(
                vec![],
                vec![status(
                    "flash",
                    Some("DeepSeek-V4-Flash"),
                    ProfileStatusCode::Unroutable,
                    Some(DiagnosticReasonCode::Unroutable),
                )],
            ))
        );
    }

    #[test]
    fn codex_subagent_v2_enabled_routable_profile_generates_role_and_status() {
        generated_for_profile(
            profile("flash", "DeepSeek-V4-Flash"),
            role(
                "flash",
                "flash",
                DESC_BALANCED_REPOSITORY,
                INSTRUCTIONS_BALANCED_REPOSITORY,
                vec![s("Flash")],
                Option::<CodexReasoningEffort>::None,
            ),
        );
    }

    #[test]
    fn codex_subagent_v2_disabled_profile_is_retained_but_generates_no_role() {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.enabled = false;
        assert_compile(
            &request(Some(config(SelectionPolicy::Balanced, vec![valid(p)]))),
            Ok(output(
                vec![],
                vec![status(
                    "flash",
                    Some("DeepSeek-V4-Flash"),
                    ProfileStatusCode::Disabled,
                    Some(DiagnosticReasonCode::Disabled),
                )],
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_unroutable_profile_is_retained_but_generates_no_role() {
        let mut request = request(Some(config(
            SelectionPolicy::Balanced,
            vec![valid(profile("flash", "DeepSeek-V4-Flash"))],
        )));
        request.catalog_models = vec![catalog("DeepSeek-V4-Flash", false)];
        assert_compile(
            &request,
            Ok(output(
                vec![],
                vec![status(
                    "flash",
                    Some("DeepSeek-V4-Flash"),
                    ProfileStatusCode::Unroutable,
                    Some(DiagnosticReasonCode::Unroutable),
                )],
            )),
        );
    }

    #[test]
    fn codex_subagent_v2_missing_config_preserves_legacy_managed_role_behavior() {
        let mut expected = output(vec![], vec![]);
        expected.legacy_managed_roles_preserved = true;
        assert_compile(&request(None), Ok(expected));
    }

    #[test]
    fn codex_subagent_v2_explicit_init_has_exact_flash_and_pro_presets() {
        let mut flash = profile("deepseek-v4-flash", "deepseek-v4-flash");
        flash.strengths = vec![
            TaskStrength::LongContextReading,
            TaskStrength::RepositoryExploration,
            TaskStrength::EvidenceCollection,
            TaskStrength::Summarization,
            TaskStrength::Testing,
        ];
        flash.preference = Preference::Preferred;
        flash.input_modalities = Some(vec![InputModality::Text]);
        let mut pro = profile("deepseek-v4-pro", "deepseek-v4-pro");
        pro.strengths = vec![
            TaskStrength::ComplexDebugging,
            TaskStrength::ArchitectureDesign,
            TaskStrength::ComplexImplementation,
            TaskStrength::HighRiskReview,
            TaskStrength::Testing,
        ];
        pro.optimization = Optimization::Quality;
        pro.write_scope = WriteScope::ComplexChanges;
        pro.preference = Preference::Preferred;
        pro.input_modalities = Some(vec![InputModality::Text]);
        assert_eq!(
            initialize_legacy_subagent_v2(),
            Ok(config(
                SelectionPolicy::Balanced,
                vec![valid(flash), valid(pro)],
            ))
        );
    }
}
