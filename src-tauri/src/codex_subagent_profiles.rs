//! Codex V2 questionnaire persistence, validation, compilation, and safe preview projection.

use serde::{Deserialize, Serialize};
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
pub enum QuestionnaireReasoningEffort {
    Auto,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelReasoningEffort {
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Official,
    ThirdParty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSubagentV2 {
    pub schema_version: u8,
    pub selection_policy: SelectionPolicy,
    pub profiles: Vec<PersistedProfileEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PersistedProfileEntry {
    Valid(CodexSubagentProfile),
    Invalid {
        key: String,
        raw: Value,
        validation_code: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSubagentProfile {
    pub key: String,
    pub model: String,
    pub enabled: bool,
    pub strengths: Vec<TaskStrength>,
    pub optimization: Optimization,
    pub write_scope: WriteScope,
    pub preference: Preference,
    pub reasoning_effort: QuestionnaireReasoningEffort,
    pub overrides: Overrides,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overrides {
    pub role_name: Option<String>,
    pub description: Option<String>,
    pub developer_instructions: Option<String>,
    pub nickname_candidates: Option<Vec<String>>,
    pub model_reasoning_effort: Option<ModelReasoningEffort>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogModel {
    pub model: String,
    pub provider_kind: ProviderKind,
    pub routable: bool,
    pub context_window: u64,
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
    pub effort: ModelReasoningEffort,
    pub context_window: u64,
}

#[derive(Serialize)]
struct RoleToml<'a> {
    name: &'a str,
    description: &'a str,
    developer_instructions: &'a str,
    nickname_candidates: &'a [String],
    model: &'a str,
    model_provider: &'a str,
    model_reasoning_effort: ModelReasoningEffort,
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
    key: String,
    model: Option<String>,
    status: ProfileStatusCode,
    reason: Option<DiagnosticReasonCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ProfileStatusCode {
    Routable,
    Disabled,
    Unroutable,
    Invalid,
    Collision,
    InactiveV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum DiagnosticReasonCode {
    Disabled,
    Unroutable,
    Invalid,
    RoleConflict,
    Collision,
    InactiveV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    model: String,
    role: Option<String>,
    policy: SelectionPolicyForDiagnostic,
    status: ProfileStatusCode,
    reason_code: Option<DiagnosticReasonCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SelectionPolicyForDiagnostic {
    Balanced,
    OfficialFirst,
    ThirdPartyFirst,
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

#[derive(Debug)]
pub struct DiagnosticSource {
    model: String,
    role: String,
    policy: SelectionPolicyForDiagnostic,
    status: ProfileStatusCode,
    reason_code: DiagnosticReasonCode,
    reason_detail: String,
    arbitrary_secret: String,
    api_key: String,
    task_body: String,
    encrypted_content: String,
}

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
    let schema = raw
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            validation_error("missing_schema_version", None, "schemaVersion is required")
        })?;
    if schema != 1 {
        return Err(validation_error(
            "unsupported_schema_version",
            None,
            "schemaVersion must be 1",
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
    let profiles = raw
        .get("profiles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut parsed = Vec::with_capacity(profiles.len());
    for (key, raw_profile) in profiles {
        let p = raw_profile.as_object().ok_or_else(|| {
            validation_error("invalid_profile", Some(&key), "profile must be an object")
        })?;
        let q = p
            .get("questionnaire")
            .and_then(Value::as_object)
            .ok_or_else(|| {
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
        let reasoning_effort = enum_field(
            q.get("reasoningEffort"),
            "missing_reasoning_effort",
            "invalid_reasoning_effort",
            &key,
            "questionnaire.reasoningEffort is required",
            "reasoningEffort is not an allowed enum member",
        )?;
        let overrides: Overrides = match p.get("overrides") {
            Some(v) => serde_json::from_value(v.clone()).map_err(|_| {
                validation_error(
                    "invalid_override_effort",
                    Some(&key),
                    "modelReasoningEffort allows only low, medium, high, or xhigh",
                )
            })?,
            None => Overrides::default(),
        };
        parsed.push(PersistedProfileEntry::Valid(CodexSubagentProfile {
            key,
            model: p
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            enabled: p.get("enabled").and_then(Value::as_bool).unwrap_or(false),
            strengths,
            optimization,
            write_scope,
            preference,
            reasoning_effort,
            overrides,
        }));
    }
    Ok(CodexSubagentV2 {
        schema_version: 1,
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
    let mut top_level = raw.clone();
    top_level["profiles"] = serde_json::json!({});
    let mut parsed = parse_persisted_subagent_v2(&top_level)?;
    let profiles = raw
        .get("profiles")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (key, profile_raw) in profiles {
        let one = serde_json::json!({
            "schemaVersion": 1,
            "selectionPolicy": parsed.selection_policy,
            "profiles": { key.clone(): profile_raw.clone() }
        });
        match parse_persisted_subagent_v2(&one) {
            Ok(one) => parsed.profiles.extend(one.profiles),
            Err(CompileError::Validation { code, .. }) => {
                parsed.profiles.push(PersistedProfileEntry::Invalid {
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

fn auto_effort(p: &CodexSubagentProfile) -> ModelReasoningEffort {
    if let Some(effort) = p.overrides.model_reasoning_effort {
        return effort;
    }
    match p.reasoning_effort {
        QuestionnaireReasoningEffort::Low => return ModelReasoningEffort::Low,
        QuestionnaireReasoningEffort::Medium => return ModelReasoningEffort::Medium,
        QuestionnaireReasoningEffort::High => return ModelReasoningEffort::High,
        QuestionnaireReasoningEffort::XHigh => return ModelReasoningEffort::XHigh,
        QuestionnaireReasoningEffort::Auto => {}
    }
    if p.strengths.iter().any(|s| {
        matches!(
            s,
            TaskStrength::ComplexDebugging
                | TaskStrength::ArchitectureDesign
                | TaskStrength::ComplexImplementation
                | TaskStrength::HighRiskReview
        )
    }) {
        return ModelReasoningEffort::High;
    }
    let read_only = p.strengths.iter().all(|s| {
        matches!(
            s,
            TaskStrength::LongContextReading
                | TaskStrength::RepositoryExploration
                | TaskStrength::EvidenceCollection
                | TaskStrength::Summarization
        )
    });
    if p.optimization == Optimization::Speed && read_only {
        ModelReasoningEffort::Low
    } else {
        ModelReasoningEffort::Medium
    }
}

fn generated_description(policy: SelectionPolicy, p: &CodexSubagentProfile) -> String {
    if let Some(description) = &p.overrides.description {
        return description.clone();
    }
    if p.strengths == [TaskStrength::ArchitectureDesign] {
        return "This role is suited to architecture design. Do not use it for routine long-context reading, repository exploration, evidence collection, or summarization. It favors quality and may make complex changes.".to_string();
    }
    if p.strengths == [TaskStrength::Testing] {
        return "This role is suited to testing. Do not use it for architecture design, complex debugging, complex implementation, or high-risk review. It favors speed but remains read-only.".to_string();
    }
    match (policy, p.preference) {
        (_, Preference::Fallback) => "This role is suited to repository exploration only when stronger matches are unavailable. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. A fallback profile is never promoted, including under third-party-first selection.",
        (SelectionPolicy::OfficialFirst, Preference::Preferred) => "This role is suited to repository exploration. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. Its preferred status overrides official-first provider bias when the task matches.",
        (SelectionPolicy::OfficialFirst, _) => "This role is suited to repository exploration. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. Under official-first selection, this eligible third-party profile is not promoted for high-risk work.",
        (SelectionPolicy::ThirdPartyFirst, _) => "This role is suited to repository exploration. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. Under third-party-first selection, this matching eligible third-party profile is promoted.",
        (SelectionPolicy::Balanced, _) => "This role is suited to repository exploration. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. It favors speed and is eligible under balanced selection without provider bias.",
    }.to_string()
}

fn generated_instructions(policy: SelectionPolicy, p: &CodexSubagentProfile) -> String {
    if let Some(value) = &p.overrides.developer_instructions {
        return value.clone();
    }
    if p.strengths == [TaskStrength::ArchitectureDesign] {
        return "Work only on delegated architecture-design tasks. Exclude routine long-context reading, repository exploration, evidence collection, and summarization. Optimize for quality, limit writes to justified complex changes, and report verification to the parent agent. This profile is eligible; follow balanced selection without adding provider bias.".to_string();
    }
    if p.strengths == [TaskStrength::Testing] {
        return "Work only on delegated testing tasks. Exclude architecture design, complex debugging, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report verification to the parent agent. This profile is eligible; follow balanced selection without adding provider bias.".to_string();
    }
    match (policy, p.preference) {
        (_, Preference::Fallback) => "Work only on delegated repository-exploration tasks when no stronger role matches. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is fallback-only and must never be promoted.",
        (SelectionPolicy::OfficialFirst, Preference::Preferred) => "Work only on delegated repository-exploration tasks. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is preferred and overrides official-first provider bias only when the task matches.",
        (SelectionPolicy::OfficialFirst, _) => "Work only on delegated repository-exploration tasks. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is eligible; under official-first selection, leave high-risk work and final integration to official roles.",
        (SelectionPolicy::ThirdPartyFirst, _) => "Work only on delegated repository-exploration tasks. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is eligible and promoted for matching work under third-party-first selection.",
        (SelectionPolicy::Balanced, _) => "Work only on delegated repository-exploration tasks. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is eligible; follow balanced selection without adding provider bias.",
    }.to_string()
}

fn default_role_name(p: &CodexSubagentProfile) -> String {
    p.key.clone()
}
fn default_nickname(p: &CodexSubagentProfile) -> String {
    let source = p.key.split(['-', '_']).next().unwrap_or(&p.key);
    let mut chars = source.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
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
        if let PersistedProfileEntry::Valid(p) = entry {
            *normalized.entry(normalize_profile_key(&p.key)).or_default() += 1;
        }
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
        let p = match entry {
            PersistedProfileEntry::Invalid { key, raw, .. } => {
                output.preserved_invalid_profiles.push(raw.clone());
                output.profile_statuses.push(ProfileStatus {
                    key: key.clone(),
                    model: None,
                    status: ProfileStatusCode::Invalid,
                    reason: Some(DiagnosticReasonCode::Invalid),
                });
                continue;
            }
            PersistedProfileEntry::Valid(p) => p,
        };
        let push_status = |output: &mut CompileOutput, status, reason| {
            output.profile_statuses.push(ProfileStatus {
                key: p.key.clone(),
                model: Some(p.model.clone()),
                status,
                reason,
            })
        };
        if normalized
            .get(&normalize_profile_key(&p.key))
            .copied()
            .unwrap_or(0)
            > 1
        {
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
            .unwrap_or_else(|| default_role_name(p));
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
            .unwrap_or_else(|| vec![default_nickname(p)]);
        if !(1..=3).contains(&nicknames.len()) {
            return Err(validation_error(
                "nickname_count",
                Some(&p.key),
                "nicknameCandidates must contain 1 through 3 entries",
            ));
        }
        let mut seen = HashSet::new();
        for nickname in &nicknames {
            if nickname.is_empty() {
                return Err(validation_error(
                    "empty_nickname",
                    Some(&p.key),
                    "nickname must be nonempty",
                ));
            }
            if !nickname
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            {
                return Err(validation_error(
                    "invalid_nickname",
                    Some(&p.key),
                    "nickname uses only ASCII alphanumeric, space, dash, underscore",
                ));
            }
            if !seen.insert(nickname) {
                return Err(validation_error(
                    "duplicate_nickname",
                    Some(&p.key),
                    "nicknameCandidates must be unique",
                ));
            }
        }
        output.generated_roles.push(GeneratedRole {
            requested_role_name: requested,
            effective_role_name: effective,
            description: generated_description(config.selection_policy, p),
            developer_instructions: generated_instructions(config.selection_policy, p),
            nickname_candidates: nicknames,
            model: p.model.clone(),
            model_provider: "codex_model_router_v2".to_string(),
            effort: auto_effort(p),
            context_window: catalog.context_window,
        });
        push_status(&mut output, ProfileStatusCode::Routable, None);
    }
    Ok(output)
}

pub fn initialize_legacy_subagent_v2() -> Result<CodexSubagentV2, CompileError> {
    let flash = CodexSubagentProfile {
        key: "deepseek-v4-flash".into(),
        model: "deepseek-v4-flash".into(),
        enabled: true,
        strengths: vec![
            TaskStrength::LongContextReading,
            TaskStrength::RepositoryExploration,
            TaskStrength::EvidenceCollection,
            TaskStrength::Summarization,
            TaskStrength::Testing,
        ],
        optimization: Optimization::Speed,
        write_scope: WriteScope::ReadOnly,
        preference: Preference::Eligible,
        reasoning_effort: QuestionnaireReasoningEffort::Medium,
        overrides: Overrides::default(),
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
    pro.reasoning_effort = QuestionnaireReasoningEffort::High;
    Ok(CodexSubagentV2 {
        schema_version: 1,
        selection_policy: SelectionPolicy::Balanced,
        profiles: vec![
            PersistedProfileEntry::Valid(flash),
            PersistedProfileEntry::Valid(pro),
        ],
    })
}

pub fn sanitize_subagent_v2_diagnostic(
    source: &DiagnosticSource,
) -> Result<Diagnostic, CompileError> {
    Ok(Diagnostic {
        model: source.model.clone(),
        role: Some(source.role.clone()),
        policy: source.policy,
        status: source.status,
        reason_code: Some(source.reason_code),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(value: &str) -> String {
        value.to_owned()
    }

    fn valid(profile: CodexSubagentProfile) -> PersistedProfileEntry {
        PersistedProfileEntry::Valid(profile)
    }

    fn profile(key: &str, model: &str) -> CodexSubagentProfile {
        CodexSubagentProfile {
            key: s(key),
            model: s(model),
            enabled: true,
            strengths: vec![TaskStrength::RepositoryExploration],
            optimization: Optimization::Speed,
            write_scope: WriteScope::ReadOnly,
            preference: Preference::Eligible,
            reasoning_effort: QuestionnaireReasoningEffort::Auto,
            overrides: Overrides::default(),
        }
    }

    fn config(
        selection_policy: SelectionPolicy,
        profiles: Vec<PersistedProfileEntry>,
    ) -> CodexSubagentV2 {
        CodexSubagentV2 {
            schema_version: 1,
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
        effort: ModelReasoningEffort,
    ) -> GeneratedRole {
        GeneratedRole {
            requested_role_name: s(requested),
            effective_role_name: s(effective),
            description: s(description),
            developer_instructions: s(instructions),
            nickname_candidates: nicknames,
            model: s("DeepSeek-V4-Flash"),
            model_provider: s("codex_model_router_v2"),
            effort,
            context_window: 1_000_000,
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
        CompileOutput {
            generated_roles: roles,
            profile_statuses: statuses,
            preserved_invalid_profiles: vec![],
            diagnostics: vec![],
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
    const DESC_BALANCED_REPOSITORY: &str = "This role is suited to repository exploration. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. It favors speed and is eligible under balanced selection without provider bias.";
    const DESC_ARCHITECTURE: &str = "This role is suited to architecture design. Do not use it for routine long-context reading, repository exploration, evidence collection, or summarization. It favors quality and may make complex changes.";
    const DESC_TESTING: &str = "This role is suited to testing. Do not use it for architecture design, complex debugging, complex implementation, or high-risk review. It favors speed but remains read-only.";
    const DESC_OFFICIAL_FIRST_ELIGIBLE: &str = "This role is suited to repository exploration. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. Under official-first selection, this eligible third-party profile is not promoted for high-risk work.";
    const DESC_THIRD_PARTY_FIRST_ELIGIBLE: &str = "This role is suited to repository exploration. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. Under third-party-first selection, this matching eligible third-party profile is promoted.";
    const DESC_OFFICIAL_FIRST_PREFERRED: &str = "This role is suited to repository exploration. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. Its preferred status overrides official-first provider bias when the task matches.";
    const DESC_FALLBACK: &str = "This role is suited to repository exploration only when stronger matches are unavailable. Do not use it for complex debugging, architecture design, complex implementation, or high-risk review. A fallback profile is never promoted, including under third-party-first selection.";
    const INSTRUCTIONS_BALANCED_REPOSITORY: &str = "Work only on delegated repository-exploration tasks. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is eligible; follow balanced selection without adding provider bias.";
    const INSTRUCTIONS_ARCHITECTURE: &str = "Work only on delegated architecture-design tasks. Exclude routine long-context reading, repository exploration, evidence collection, and summarization. Optimize for quality, limit writes to justified complex changes, and report verification to the parent agent. This profile is eligible; follow balanced selection without adding provider bias.";
    const INSTRUCTIONS_TESTING: &str = "Work only on delegated testing tasks. Exclude architecture design, complex debugging, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report verification to the parent agent. This profile is eligible; follow balanced selection without adding provider bias.";
    const INSTRUCTIONS_OFFICIAL_FIRST_ELIGIBLE: &str = "Work only on delegated repository-exploration tasks. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is eligible; under official-first selection, leave high-risk work and final integration to official roles.";
    const INSTRUCTIONS_THIRD_PARTY_FIRST_ELIGIBLE: &str = "Work only on delegated repository-exploration tasks. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is eligible and promoted for matching work under third-party-first selection.";
    const INSTRUCTIONS_OFFICIAL_FIRST_PREFERRED: &str = "Work only on delegated repository-exploration tasks. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is preferred and overrides official-first provider bias only when the task matches.";
    const INSTRUCTIONS_FALLBACK: &str = "Work only on delegated repository-exploration tasks when no stronger role matches. Exclude complex debugging, architecture design, complex implementation, and high-risk review. Optimize for speed, keep all work read-only, and report evidence to the parent agent. This profile is fallback-only and must never be promoted.";

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

    fn raw_profile_missing_questionnaire_field(field: &str) -> Value {
        let mut q = questionnaire();
        q.as_object_mut()
            .expect("questionnaire fixture is an object")
            .remove(field);
        raw_profile_with_questionnaire(q)
    }

    fn expected_valid_profile_with_strengths(strengths: Vec<TaskStrength>) -> CodexSubagentV2 {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.strengths = strengths;
        config(SelectionPolicy::Balanced, vec![valid(p)])
    }

    fn generated_for_profile(p: CodexSubagentProfile, expected: GeneratedRole) {
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
    fn codex_subagent_v2_rejects_schema_version_other_than_one() {
        assert_parse(
            json!({"schemaVersion": 2, "profiles": {}}),
            Err(validation(
                "unsupported_schema_version",
                None,
                "schemaVersion must be 1",
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
                "modelReasoningEffort allows only low, medium, high, or xhigh",
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
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.overrides = Overrides {
            role_name: Some(s("flash-reader")),
            description: Some(s("Manual.")),
            developer_instructions: Some(s("Read only.")),
            nickname_candidates: Some(vec![s("Flash Reader")]),
            model_reasoning_effort: Some(ModelReasoningEffort::XHigh),
        };
        assert_parse(
            json!({"schemaVersion":1,"profiles":{"flash":{"model":"DeepSeek-V4-Flash","enabled":true,"questionnaire":{"taskStrengths":["repository_exploration"],"optimization":"speed","writeScope":"read_only","preference":"eligible","reasoningEffort":"auto"},"overrides":{"roleName":"flash-reader","description":"Manual.","developerInstructions":"Read only.","nicknameCandidates":["Flash Reader"],"modelReasoningEffort":"xhigh"}}}}),
            Ok(config(SelectionPolicy::Balanced, vec![valid(p)])),
        );
    }

    #[test]
    fn codex_subagent_v2_public_serde_shape_is_keyed_and_nested() {
        let raw = json!({
            "schemaVersion": 1,
            "selectionPolicy": "official_first",
            "profiles": {
                "flash": {
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
        let parsed = parse_persisted_subagent_v2(&raw).expect("strict public payload");
        assert_eq!(
            serde_json::to_value(parsed).expect("serialize public payload"),
            raw,
            "the persisted API is a keyed map and must not expose internal keys or flattened questionnaire fields"
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
            .all(|entry| matches!(entry, PersistedProfileEntry::Invalid { .. })));
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
            raw_profile(json!(["testing"])),
            Ok(expected_valid_profile_with_strengths(vec![
                TaskStrength::Testing,
            ])),
        );
    }

    #[test]
    fn codex_subagent_v2_accepts_five_unique_task_strengths() {
        assert_parse(
            raw_profile(json!([
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
        let mut valid_profile = profile("Straße", "DeepSeek-V4-Flash");
        valid_profile.overrides.nickname_candidates = Some(vec![s("Street")]);
        let saved = config(
            SelectionPolicy::Balanced,
            vec![
                valid(valid_profile),
                PersistedProfileEntry::Invalid {
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
                PersistedProfileEntry::Invalid {
                    key: s("Ｆｏｏ"),
                    raw: raw_a.clone(),
                    validation_code: s("invalid_model"),
                },
                PersistedProfileEntry::Invalid {
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
            let description = generated_description(SelectionPolicy::Balanced, &p);
            let instructions = generated_instructions(SelectionPolicy::Balanced, &p);
            assert!(
                description.to_ascii_lowercase().contains(phrase),
                "description missing {phrase}: {description}"
            );
            assert!(
                instructions.to_ascii_lowercase().contains(phrase),
                "instructions missing {phrase}: {instructions}"
            );
            assert!(
                (2..=4).contains(&description.matches('.').count()),
                "description must remain 2-4 sentences: {description}"
            );
        }
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.optimization = Optimization::Quality;
        p.write_scope = WriteScope::BoundedChanges;
        p.preference = Preference::Preferred;
        let description = generated_description(SelectionPolicy::OfficialFirst, &p);
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

    fn effort_profile(
        strength: TaskStrength,
        optimization: Optimization,
        override_effort: Option<ModelReasoningEffort>,
    ) -> CodexSubagentProfile {
        let mut p = profile("flash", "DeepSeek-V4-Flash");
        p.strengths = vec![strength];
        p.optimization = optimization;
        p.overrides.model_reasoning_effort = override_effort;
        p
    }

    #[test]
    fn codex_subagent_v2_auto_effort_is_high_for_complex_strength() {
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
                ModelReasoningEffort::High,
            ),
        );
    }

    #[test]
    fn codex_subagent_v2_auto_effort_is_low_for_speed_read_only_strengths() {
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
                ModelReasoningEffort::Low,
            ),
        );
    }

    #[test]
    fn codex_subagent_v2_auto_effort_is_medium_for_speed_testing() {
        generated_for_profile(
            effort_profile(TaskStrength::Testing, Optimization::Speed, None),
            role(
                "flash",
                "flash",
                DESC_TESTING,
                INSTRUCTIONS_TESTING,
                vec![s("Flash")],
                ModelReasoningEffort::Medium,
            ),
        );
    }

    #[test]
    fn codex_subagent_v2_explicit_effort_overrides_auto_effort() {
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

    fn policy_profile(preference: Preference) -> CodexSubagentProfile {
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
                ModelReasoningEffort::Low,
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
                ModelReasoningEffort::Low,
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
                ModelReasoningEffort::Low,
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
                ModelReasoningEffort::Low,
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
                ModelReasoningEffort::Low,
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
            ModelReasoningEffort::Low,
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
            vec![PersistedProfileEntry::Invalid {
                key: s("broken"),
                raw: raw.clone(),
                validation_code: s("invalid_enabled"),
            }],
        );
        let mut expected = output(
            vec![],
            vec![status(
                "broken",
                None,
                ProfileStatusCode::Invalid,
                Some(DiagnosticReasonCode::Invalid),
            )],
        );
        expected.preserved_invalid_profiles = vec![raw];
        assert_compile(&request(Some(saved)), Ok(expected));
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
                ModelReasoningEffort::Low,
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
        flash.reasoning_effort = QuestionnaireReasoningEffort::Medium;
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
        pro.reasoning_effort = QuestionnaireReasoningEffort::High;
        assert_eq!(
            initialize_legacy_subagent_v2(),
            Ok(config(
                SelectionPolicy::Balanced,
                vec![valid(flash), valid(pro)],
            ))
        );
    }

    #[test]
    fn codex_subagent_v2_diagnostic_sanitizer_emits_only_allowlisted_metadata() {
        let source = DiagnosticSource {
            model: s("DeepSeek-V4-Flash"),
            role: s("flash"),
            policy: SelectionPolicyForDiagnostic::Balanced,
            status: ProfileStatusCode::Unroutable,
            reason_code: DiagnosticReasonCode::Unroutable,
            reason_detail: s("internal detail SECRET_REASON"),
            arbitrary_secret: s("ARBITRARY_SECRET"),
            api_key: s("API_KEY_SECRET"),
            task_body: s("TASK_BODY_SECRET"),
            encrypted_content: s("ENCRYPTED_SECRET"),
        };
        let expected = Diagnostic {
            model: s("DeepSeek-V4-Flash"),
            role: Some(s("flash")),
            policy: SelectionPolicyForDiagnostic::Balanced,
            status: ProfileStatusCode::Unroutable,
            reason_code: Some(DiagnosticReasonCode::Unroutable),
        };
        let actual = sanitize_subagent_v2_diagnostic(&source).map(|diagnostic| {
            let serialized = serde_json::to_string(&diagnostic)
                .expect("allowlisted diagnostic must remain serializable");
            (diagnostic, serialized)
        });
        assert_eq!(
            actual,
            Ok((
                expected,
                s(
                    r#"{"model":"DeepSeek-V4-Flash","role":"flash","policy":"Balanced","status":"Unroutable","reason_code":"Unroutable"}"#
                ),
            ))
        );
    }
}
