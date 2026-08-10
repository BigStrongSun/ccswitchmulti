//! RED-only, production-shaped Codex V2 profile compiler contracts.

use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubagentVersion {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionPolicy {
    Balanced,
    OfficialFirst,
    ThirdPartyFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskStrength {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Optimization {
    Speed,
    Balanced,
    Quality,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WriteScope {
    ReadOnly,
    BoundedChanges,
    ComplexChanges,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preference {
    Preferred,
    Eligible,
    Fallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuestionnaireReasoningEffort {
    Auto,
    Low,
    Medium,
    High,
    XHigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelReasoningEffort {
    Low,
    Medium,
    High,
    XHigh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderKind {
    Official,
    ThirdParty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CodexSubagentV2 {
    schema_version: u8,
    selection_policy: SelectionPolicy,
    profiles: Vec<PersistedProfileEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PersistedProfileEntry {
    Valid(CodexSubagentProfile),
    Invalid {
        key: String,
        raw: Value,
        validation_code: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CodexSubagentProfile {
    key: String,
    model: String,
    enabled: bool,
    strengths: Vec<TaskStrength>,
    optimization: Optimization,
    write_scope: WriteScope,
    preference: Preference,
    reasoning_effort: QuestionnaireReasoningEffort,
    overrides: Overrides,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Overrides {
    role_name: Option<String>,
    description: Option<String>,
    developer_instructions: Option<String>,
    nickname_candidates: Option<Vec<String>>,
    model_reasoning_effort: Option<ModelReasoningEffort>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogModel {
    model: String,
    provider_kind: ProviderKind,
    routable: bool,
    context_window: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompileRequest {
    subagent_version: SubagentVersion,
    persisted_subagent_v2: Option<CodexSubagentV2>,
    catalog_models: Vec<CatalogModel>,
    occupied_role_names: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct CompileOutput {
    generated_roles: Vec<GeneratedRole>,
    profile_statuses: Vec<ProfileStatus>,
    preserved_invalid_profiles: Vec<Value>,
    diagnostics: Vec<Diagnostic>,
    legacy_managed_roles_preserved: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct GeneratedRole {
    requested_role_name: String,
    effective_role_name: String,
    description: String,
    developer_instructions: String,
    nickname_candidates: Vec<String>,
    model: String,
    model_provider: String,
    effort: ModelReasoningEffort,
    context_window: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct ProfileStatus {
    key: String,
    model: Option<String>,
    status: ProfileStatusCode,
    reason: Option<DiagnosticReasonCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum ProfileStatusCode {
    Routable,
    Disabled,
    Unroutable,
    Invalid,
    Collision,
    InactiveV1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum DiagnosticReasonCode {
    Disabled,
    Unroutable,
    Invalid,
    RoleConflict,
    Collision,
    InactiveV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Diagnostic {
    model: String,
    role: Option<String>,
    policy: SelectionPolicyForDiagnostic,
    status: ProfileStatusCode,
    reason_code: Option<DiagnosticReasonCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
enum SelectionPolicyForDiagnostic {
    Balanced,
    OfficialFirst,
    ThirdPartyFirst,
}

#[derive(Debug, PartialEq, Eq)]
enum CompileError {
    NotImplemented,
    Validation {
        code: String,
        profile_key: Option<String>,
        detail: String,
    },
}

type CompileResult = Result<CompileOutput, CompileError>;

#[derive(Debug)]
struct DiagnosticSource {
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

fn parse_persisted_subagent_v2_for_red_test(_: &Value) -> Result<CodexSubagentV2, CompileError> {
    Err(CompileError::NotImplemented)
}

fn compile_subagent_v2_profiles_for_red_test(_: &CompileRequest) -> CompileResult {
    Err(CompileError::NotImplemented)
}

fn initialize_legacy_subagent_v2_for_red_test() -> Result<CodexSubagentV2, CompileError> {
    Err(CompileError::NotImplemented)
}

fn sanitize_subagent_v2_diagnostic_for_red_test(
    _: &DiagnosticSource,
) -> Result<Diagnostic, CompileError> {
    Err(CompileError::NotImplemented)
}

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
    assert_eq!(parse_persisted_subagent_v2_for_red_test(&raw), expected);
}

fn assert_compile(request: &CompileRequest, expected: CompileResult) {
    assert_eq!(compile_subagent_v2_profiles_for_red_test(request), expected);
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
    request.occupied_role_names = vec![s("REVIEW"), s("CcSwitch-Review"), s("CCSWITCH-REVIEW-2")];
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
    let actual = compile_subagent_v2_profiles_for_red_test(&request);
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
    let actual = compile_subagent_v2_profiles_for_red_test(&request);
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
        initialize_legacy_subagent_v2_for_red_test(),
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
    let actual = sanitize_subagent_v2_diagnostic_for_red_test(&source).map(|diagnostic| {
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
