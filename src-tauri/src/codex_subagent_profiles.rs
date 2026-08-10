//! RED-only, production-shaped Codex V2 profile compiler contracts.

use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
struct CodexSubagentV2 {
    schema_version: u8,
    selection_policy: String,
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
    strengths: Vec<String>,
    optimization: String,
    write_scope: String,
    preference: String,
    reasoning_effort: String,
    overrides: Overrides,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Overrides {
    role_name: Option<String>,
    description: Option<String>,
    developer_instructions: Option<String>,
    nickname_candidates: Option<Vec<String>>,
    model_reasoning_effort: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogModel {
    model: String,
    provider_kind: String,
    routable: bool,
    context_window: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct CompileRequest {
    subagent_version: String,
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
    effort: String,
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
    policy: String,
    status: ProfileStatusCode,
    reason_code: Option<DiagnosticReasonCode>,
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
    policy: String,
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
        strengths: vec![s("repository_exploration")],
        optimization: s("speed"),
        write_scope: s("read_only"),
        preference: s("eligible"),
        reasoning_effort: s("auto"),
        overrides: Overrides::default(),
    }
}
fn config(policy: &str, profiles: Vec<PersistedProfileEntry>) -> CodexSubagentV2 {
    CodexSubagentV2 {
        schema_version: 1,
        selection_policy: s(policy),
        profiles,
    }
}
fn catalog(model: &str, routable: bool) -> CatalogModel {
    CatalogModel {
        model: s(model),
        provider_kind: s("third_party"),
        routable,
        context_window: 1_000_000,
    }
}
fn request(config: Option<CodexSubagentV2>) -> CompileRequest {
    CompileRequest {
        subagent_version: s("v2"),
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
    effort: &str,
) -> GeneratedRole {
    GeneratedRole {
        requested_role_name: s(requested),
        effective_role_name: s(effective),
        description: s(description),
        developer_instructions: s(instructions),
        nickname_candidates: nicknames,
        model: s("DeepSeek-V4-Flash"),
        model_provider: s("codex_model_router_v2"),
        effort: s(effort),
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
    }
}
fn assert_parse(raw: Value, expected: Result<CodexSubagentV2, CompileError>) {
    assert_eq!(parse_persisted_subagent_v2_for_red_test(&raw), expected);
}
fn assert_compile(request: &CompileRequest, expected: CompileResult) {
    assert_eq!(compile_subagent_v2_profiles_for_red_test(request), expected);
}
fn raw_profile(strengths: Value) -> Value {
    json!({ "schemaVersion": 1, "profiles": { "flash": { "model": "DeepSeek-V4-Flash", "enabled": true, "questionnaire": { "taskStrengths": strengths, "optimization": "speed", "writeScope": "read_only", "preference": "eligible", "reasoningEffort": "auto" } } } })
}

#[test]
fn codex_subagent_v2_defaults_only_missing_selection_policy() {
    assert_parse(
        json!({"schemaVersion":1,"profiles":{}}),
        Ok(config("balanced", vec![])),
    );
}
#[test]
fn codex_subagent_v2_rejects_missing_schema_version() {
    assert_parse(
        json!({"profiles":{}}),
        Err(validation(
            "missing_schema_version",
            None,
            "schemaVersion is required",
        )),
    );
}
#[test]
fn codex_subagent_v2_rejects_invalid_enum_and_missing_questionnaire_field() {
    assert_parse(
        json!({"schemaVersion":1,"profiles":{"flash":{"model":"m","enabled":true,"questionnaire":{"taskStrengths":["testing"],"optimization":"fastest","preference":"eligible","reasoningEffort":"auto"}}}}),
        Err(validation(
            "invalid_optimization",
            Some("flash"),
            "optimization is not an allowed enum member",
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
        model_reasoning_effort: Some(s("xhigh")),
    };
    assert_parse(
        json!({"schemaVersion":1,"profiles":{"flash":{"model":"DeepSeek-V4-Flash","enabled":true,"questionnaire":{"taskStrengths":["repository_exploration"],"optimization":"speed","writeScope":"read_only","preference":"eligible","reasoningEffort":"auto"},"overrides":{"roleName":"flash-reader","description":"Manual.","developerInstructions":"Read only.","nicknameCandidates":["Flash Reader"],"modelReasoningEffort":"xhigh"}}}}),
        Ok(config("balanced", vec![valid(p)])),
    );
}

#[test]
fn codex_subagent_v2_strength_count_and_membership_boundaries() {
    assert_parse(
        raw_profile(json!([])),
        Err(validation(
            "strength_count",
            Some("flash"),
            "taskStrengths must contain 1 through 5 members",
        )),
    );
    assert_parse(
        raw_profile(json!(["testing", "testing"])),
        Err(validation(
            "duplicate_task_strength",
            Some("flash"),
            "taskStrengths members must be unique",
        )),
    );
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
        &request(Some(config("balanced", vec![valid(a), valid(b)]))),
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
            "balanced",
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

fn effort_case(strength: &str, optimization: &str, override_effort: Option<&str>, expected: &str) {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.strengths = vec![s(strength)];
    p.optimization = s(optimization);
    p.overrides.model_reasoning_effort = override_effort.map(s);
    assert_compile(
        &request(Some(config("balanced", vec![valid(p)]))),
        Ok(output(
            vec![role(
                "flash",
                "flash",
                "generated",
                "generated",
                vec![s("Flash")],
                expected,
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_effort_truth_table() {
    effort_case("architecture_design", "quality", None, "high");
    effort_case("repository_exploration", "speed", None, "low");
    effort_case("testing", "speed", None, "medium");
    effort_case("architecture_design", "quality", Some("xhigh"), "xhigh");
}

fn policy_case(policy: &str, preference: &str, description: &str) {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.preference = s(preference);
    assert_compile(
        &request(Some(config(policy, vec![valid(p)]))),
        Ok(output(
            vec![role(
                "flash",
                "flash",
                description,
                "generated",
                vec![s("Flash")],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_policy_semantics_keep_router_provider() {
    policy_case(
        "balanced",
        "eligible",
        "Balanced selection: matching eligible profile.",
    );
    policy_case("official_first","eligible","Official-first selection: eligible third-party profile is not promoted for high-risk work.");
    policy_case(
        "third_party_first",
        "eligible",
        "Third-party-first selection: matching eligible profile is promoted.",
    );
    policy_case(
        "official_first",
        "preferred",
        "Official-first selection: explicitly preferred profile overrides provider bias.",
    );
    policy_case(
        "third_party_first",
        "fallback",
        "Fallback profile is never promoted.",
    );
}

#[test]
fn codex_subagent_v2_manual_description_and_field_restore_keep_other_override() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.overrides.description = Some(s("Manual."));
    p.overrides.developer_instructions = Some(s("Keep."));
    assert_compile(
        &request(Some(config("balanced", vec![valid(p)]))),
        Ok(output(
            vec![role(
                "flash",
                "flash",
                "Manual.",
                "Keep.",
                vec![s("Flash")],
                "low",
            )],
            vec![],
        )),
    );
    let mut restored = profile("flash", "DeepSeek-V4-Flash");
    restored.overrides.developer_instructions = Some(s("Keep."));
    assert_compile(
        &request(Some(config("balanced", vec![valid(restored)]))),
        Ok(output(
            vec![role(
                "flash",
                "flash",
                "Balanced selection: matching eligible profile.",
                "Keep.",
                vec![s("Flash")],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_role_normalization_and_request_occupancy() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.overrides.role_name = Some(s("Foo__-- Bar"));
    assert_compile(
        &request(Some(config("balanced", vec![valid(p)]))),
        Ok(output(
            vec![role(
                "Foo__-- Bar",
                "foo-bar",
                "generated",
                "generated",
                vec![s("Flash")],
                "low",
            )],
            vec![],
        )),
    );
    let mut q = profile("flash", "DeepSeek-V4-Flash");
    q.overrides.role_name = Some(s("review"));
    let mut r = request(Some(config("balanced", vec![valid(q)])));
    r.occupied_role_names = vec![s("review"), s("ccswitch-review"), s("ccswitch-review-2")];
    assert_compile(
        &r,
        Ok(output(
            vec![role(
                "review",
                "ccswitch-review-3",
                "generated",
                "generated",
                vec![s("Flash")],
                "low",
            )],
            vec![],
        )),
    );
}

fn nicknames(values: Vec<&str>) -> CompileRequest {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.overrides.nickname_candidates = Some(values.into_iter().map(s).collect());
    request(Some(config("balanced", vec![valid(p)])))
}
#[test]
fn codex_subagent_v2_nickname_contract() {
    assert_compile(
        &nicknames(vec![]),
        Err(validation(
            "nickname_count",
            Some("flash"),
            "nicknameCandidates must contain 1 through 3 entries",
        )),
    );
    assert_compile(
        &nicknames(vec!["One"]),
        Ok(output(
            vec![role(
                "flash",
                "flash",
                "generated",
                "generated",
                vec![s("One")],
                "low",
            )],
            vec![],
        )),
    );
    assert_compile(
        &nicknames(vec!["One", "Two", "Three"]),
        Ok(output(
            vec![role(
                "flash",
                "flash",
                "generated",
                "generated",
                vec![s("One"), s("Two"), s("Three")],
                "low",
            )],
            vec![],
        )),
    );
    assert_compile(
        &nicknames(vec!["One", "Two", "Three", "Four"]),
        Err(validation(
            "nickname_count",
            Some("flash"),
            "nicknameCandidates must contain 1 through 3 entries",
        )),
    );
    assert_compile(
        &nicknames(vec![""]),
        Err(validation(
            "empty_nickname",
            Some("flash"),
            "nickname must be nonempty",
        )),
    );
    assert_compile(
        &nicknames(vec!["Dup", "Dup"]),
        Err(validation(
            "duplicate_nickname",
            Some("flash"),
            "nicknameCandidates must be unique",
        )),
    );
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
        "balanced",
        vec![PersistedProfileEntry::Invalid {
            key: s("broken"),
            raw: raw.clone(),
            validation_code: s("invalid_enabled"),
        }],
    );
    let expected = CompileOutput {
        generated_roles: vec![],
        profile_statuses: vec![status(
            "broken",
            None,
            ProfileStatusCode::Invalid,
            Some(DiagnosticReasonCode::Invalid),
        )],
        preserved_invalid_profiles: vec![raw],
        diagnostics: vec![],
    };
    assert_compile(&request(Some(saved)), Ok(expected));
}
#[test]
fn codex_subagent_v2_lifecycle_v1_v2_and_alias_preserve_persisted_data() {
    let saved = config(
        "balanced",
        vec![valid(profile("flash", "DeepSeek-V4-Flash"))],
    );
    let mut v1 = request(Some(saved.clone()));
    v1.subagent_version = s("v1");
    assert_eq!(v1.persisted_subagent_v2, Some(saved.clone()));
    assert_compile(
        &v1,
        Ok(output(
            vec![],
            vec![status(
                "flash",
                Some("DeepSeek-V4-Flash"),
                ProfileStatusCode::InactiveV1,
                Some(DiagnosticReasonCode::InactiveV1),
            )],
        )),
    );
    let mut alias = request(Some(saved.clone()));
    alias.catalog_models = vec![catalog("deepseek-flash-alias", true)];
    assert_eq!(alias.persisted_subagent_v2, Some(saved));
    assert_compile(
        &alias,
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
fn codex_subagent_v2_missing_config_is_legacy_and_explicit_init_has_exact_presets() {
    assert_compile(&request(None), Ok(output(vec![], vec![])));
    let mut flash = profile("deepseek-v4-flash", "deepseek-v4-flash");
    flash.strengths = vec![
        s("long_context_reading"),
        s("repository_exploration"),
        s("evidence_collection"),
        s("summarization"),
        s("testing"),
    ];
    flash.reasoning_effort = s("medium");
    let mut pro = profile("deepseek-v4-pro", "deepseek-v4-pro");
    pro.strengths = vec![
        s("complex_debugging"),
        s("architecture_design"),
        s("complex_implementation"),
        s("high_risk_review"),
        s("testing"),
    ];
    pro.optimization = s("quality");
    pro.write_scope = s("complex_changes");
    pro.reasoning_effort = s("high");
    assert_eq!(
        initialize_legacy_subagent_v2_for_red_test(),
        Ok(config("balanced", vec![valid(flash), valid(pro)]))
    );
}
#[test]
fn codex_subagent_v2_diagnostic_sanitizer_uses_reason_code_not_arbitrary_text() {
    let source = DiagnosticSource {
        model: s("DeepSeek-V4-Flash"),
        role: s("flash"),
        policy: s("balanced"),
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
        policy: s("balanced"),
        status: ProfileStatusCode::Unroutable,
        reason_code: Some(DiagnosticReasonCode::Unroutable),
    };
    assert_eq!(
        sanitize_subagent_v2_diagnostic_for_red_test(&source),
        Ok(expected.clone())
    );
    let serialized = serde_json::to_string(&expected).expect("serialize allowlisted diagnostic");
    for marker in [
        "SECRET_REASON",
        "ARBITRARY_SECRET",
        "API_KEY_SECRET",
        "TASK_BODY_SECRET",
        "ENCRYPTED_SECRET",
    ] {
        assert!(
            !serialized.contains(marker),
            "sanitized diagnostic must not contain {marker}"
        );
    }
}
