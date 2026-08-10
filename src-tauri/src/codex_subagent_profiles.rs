//! RED-only contracts for the future Codex V2 capability-profile backend.
//!
//! The module has one crate-root `cfg(test)` identity. The sentinels deliberately have no
//! production caller; Task 3 can promote these request/result shapes directly to production.

use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PersistedV2 {
    schema_version: u8,
    selection_policy: &'static str,
    profiles: Vec<PersistedProfile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PersistedProfile {
    key: &'static str,
    model: &'static str,
    enabled: bool,
    strengths: Vec<&'static str>,
    optimization: &'static str,
    write_scope: &'static str,
    preference: &'static str,
    reasoning_effort: &'static str,
    overrides: Overrides,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Overrides {
    role_name: Option<&'static str>,
    description: Option<&'static str>,
    developer_instructions: Option<&'static str>,
    nickname_candidates: Option<Vec<&'static str>>,
    model_reasoning_effort: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CatalogModel {
    model: &'static str,
    provider_kind: &'static str,
    routable: bool,
    context_window: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompileRequest {
    subagent_version: &'static str,
    persisted_subagent_v2: Option<PersistedV2>,
    catalog_models: Vec<CatalogModel>,
    occupied_role_names: Vec<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
struct CompileOutput {
    generated_roles: Vec<GeneratedRole>,
    profile_statuses: Vec<ProfileStatus>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, PartialEq, Eq)]
struct GeneratedRole {
    requested_role_name: &'static str,
    effective_role_name: &'static str,
    description: &'static str,
    developer_instructions: &'static str,
    nickname_candidates: Vec<&'static str>,
    model: &'static str,
    model_provider: &'static str,
    effort: &'static str,
    context_window: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct ProfileStatus {
    key: &'static str,
    model: &'static str,
    status: &'static str,
    reason: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
struct Diagnostic {
    model: &'static str,
    role: Option<&'static str>,
    policy: &'static str,
    status: &'static str,
    reason: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
enum CompileError {
    NotImplemented,
    Validation {
        code: &'static str,
        profile_key: Option<&'static str>,
        detail: &'static str,
    },
}

type CompileResult = Result<CompileOutput, CompileError>;

#[derive(Debug, PartialEq, Eq)]
struct DiagnosticSource {
    model: &'static str,
    role: &'static str,
    policy: &'static str,
    status: &'static str,
    reason: &'static str,
    api_key: &'static str,
    task_body: &'static str,
    encrypted_content: &'static str,
}

fn parse_persisted_subagent_v2_for_red_test(_raw: &Value) -> Result<PersistedV2, CompileError> {
    Err(CompileError::NotImplemented)
}

fn compile_subagent_v2_profiles_for_red_test(_request: &CompileRequest) -> CompileResult {
    Err(CompileError::NotImplemented)
}

fn initialize_legacy_subagent_v2_for_red_test() -> Result<PersistedV2, CompileError> {
    Err(CompileError::NotImplemented)
}

fn sanitize_subagent_v2_diagnostic_for_red_test(
    _source: &DiagnosticSource,
) -> Result<Diagnostic, CompileError> {
    Err(CompileError::NotImplemented)
}

fn assert_parse_eq(raw: Value, expected: Result<PersistedV2, CompileError>) {
    assert_eq!(parse_persisted_subagent_v2_for_red_test(&raw), expected);
}

fn assert_compile_eq(request: &CompileRequest, expected: CompileResult) {
    assert_eq!(compile_subagent_v2_profiles_for_red_test(request), expected);
}

fn profile(key: &'static str, model: &'static str) -> PersistedProfile {
    PersistedProfile {
        key,
        model,
        enabled: true,
        strengths: vec!["repository_exploration"],
        optimization: "speed",
        write_scope: "read_only",
        preference: "eligible",
        reasoning_effort: "auto",
        overrides: Overrides::default(),
    }
}

fn config(policy: &'static str, profiles: Vec<PersistedProfile>) -> PersistedV2 {
    PersistedV2 {
        schema_version: 1,
        selection_policy: policy,
        profiles,
    }
}

fn raw_profile(strengths: Value) -> Value {
    json!({ "schemaVersion": 1, "profiles": { "flash": { "model": "DeepSeek-V4-Flash", "enabled": true, "questionnaire": { "taskStrengths": strengths, "optimization": "speed", "writeScope": "read_only", "preference": "eligible", "reasoningEffort": "auto" } } } })
}

fn flash_catalog() -> CatalogModel {
    CatalogModel {
        model: "DeepSeek-V4-Flash",
        provider_kind: "third_party",
        routable: true,
        context_window: 1_000_000,
    }
}

fn request(config: PersistedV2) -> CompileRequest {
    CompileRequest {
        subagent_version: "v2",
        persisted_subagent_v2: Some(config),
        catalog_models: vec![flash_catalog()],
        occupied_role_names: vec![],
    }
}

fn validation(
    code: &'static str,
    key: Option<&'static str>,
    detail: &'static str,
) -> Result<PersistedV2, CompileError> {
    Err(CompileError::Validation {
        code,
        profile_key: key,
        detail,
    })
}

fn compile_validation(
    code: &'static str,
    key: Option<&'static str>,
    detail: &'static str,
) -> CompileResult {
    Err(CompileError::Validation {
        code,
        profile_key: key,
        detail,
    })
}

fn generated(
    requested: &'static str,
    effective: &'static str,
    description: &'static str,
    instructions: &'static str,
    nicknames: Vec<&'static str>,
    effort: &'static str,
) -> GeneratedRole {
    GeneratedRole {
        requested_role_name: requested,
        effective_role_name: effective,
        description,
        developer_instructions: instructions,
        nickname_candidates: nicknames,
        model: "DeepSeek-V4-Flash",
        model_provider: "codex_model_router_v2",
        effort,
        context_window: 1_000_000,
    }
}

fn output(roles: Vec<GeneratedRole>, statuses: Vec<ProfileStatus>) -> CompileOutput {
    CompileOutput {
        generated_roles: roles,
        profile_statuses: statuses,
        diagnostics: vec![],
    }
}

#[test]
fn codex_subagent_v2_defaults_omitted_selection_policy_to_balanced() {
    let raw = json!({ "schemaVersion": 1, "profiles": {} });
    assert_parse_eq(raw, Ok(config("balanced", vec![])));
}

#[test]
fn codex_subagent_v2_rejects_missing_schema_version() {
    assert_parse_eq(
        json!({ "selectionPolicy": "balanced", "profiles": {} }),
        validation("missing_schema_version", None, "schemaVersion is required"),
    );
}

#[test]
fn codex_subagent_v2_rejects_non_v1_schema_version() {
    assert_parse_eq(
        json!({ "schemaVersion": 2, "profiles": {} }),
        validation(
            "unsupported_schema_version",
            None,
            "schemaVersion must equal 1",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_illegal_questionnaire_enum() {
    assert_parse_eq(
        json!({ "schemaVersion": 1, "profiles": { "flash": { "model": "DeepSeek-V4-Flash", "enabled": true, "questionnaire": { "taskStrengths": ["testing"], "optimization": "fastest", "writeScope": "read_only", "preference": "eligible", "reasoningEffort": "auto" } } } }),
        validation(
            "invalid_optimization",
            Some("flash"),
            "optimization is not an allowed enum member",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_missing_questionnaire_field() {
    assert_parse_eq(
        json!({ "schemaVersion": 1, "profiles": { "flash": { "model": "DeepSeek-V4-Flash", "enabled": true, "questionnaire": { "taskStrengths": ["testing"], "optimization": "speed", "preference": "eligible", "reasoningEffort": "auto" } } } }),
        validation(
            "missing_write_scope",
            Some("flash"),
            "writeScope is required",
        ),
    );
}

#[test]
fn codex_subagent_v2_round_trips_all_override_fields() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.overrides = Overrides {
        role_name: Some("flash-reader"),
        description: Some("Manual selection text."),
        developer_instructions: Some("Read only."),
        nickname_candidates: Some(vec!["Flash Reader"]),
        model_reasoning_effort: Some("xhigh"),
    };
    assert_parse_eq(
        json!({ "schemaVersion": 1, "profiles": { "flash": { "model": "DeepSeek-V4-Flash", "enabled": true, "questionnaire": { "taskStrengths": ["repository_exploration"], "optimization": "speed", "writeScope": "read_only", "preference": "eligible", "reasoningEffort": "auto" }, "overrides": { "roleName": "flash-reader", "description": "Manual selection text.", "developerInstructions": "Read only.", "nicknameCandidates": ["Flash Reader"], "modelReasoningEffort": "xhigh" } } } }),
        Ok(config("balanced", vec![p])),
    );
}

#[test]
fn codex_subagent_v2_rejects_zero_strengths() {
    assert_parse_eq(
        raw_profile(json!([])),
        validation(
            "strength_count",
            Some("flash"),
            "taskStrengths must contain 1 through 5 members",
        ),
    );
}
#[test]
fn codex_subagent_v2_accepts_one_strength() {
    assert_compile_eq(
        &request(config(
            "balanced",
            vec![profile("flash", "DeepSeek-V4-Flash")],
        )),
        Ok(output(
            vec![],
            vec![ProfileStatus {
                key: "flash",
                model: "DeepSeek-V4-Flash",
                status: "routable",
                reason: None,
            }],
        )),
    );
}
#[test]
fn codex_subagent_v2_accepts_five_strengths() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.strengths = vec![
        "long_context_reading",
        "repository_exploration",
        "evidence_collection",
        "summarization",
        "testing",
    ];
    assert_compile_eq(
        &request(config("balanced", vec![p])),
        Ok(output(
            vec![],
            vec![ProfileStatus {
                key: "flash",
                model: "DeepSeek-V4-Flash",
                status: "routable",
                reason: None,
            }],
        )),
    );
}
#[test]
fn codex_subagent_v2_rejects_six_strengths() {
    assert_parse_eq(
        raw_profile(json!([
            "long_context_reading",
            "repository_exploration",
            "evidence_collection",
            "summarization",
            "testing",
            "complex_debugging"
        ])),
        validation(
            "strength_count",
            Some("flash"),
            "taskStrengths must contain 1 through 5 members",
        ),
    );
}
#[test]
fn codex_subagent_v2_rejects_duplicate_strength() {
    assert_parse_eq(
        raw_profile(json!(["testing", "testing"])),
        validation(
            "duplicate_task_strength",
            Some("flash"),
            "taskStrengths members must be unique",
        ),
    );
}
#[test]
fn codex_subagent_v2_rejects_unknown_strength() {
    assert_parse_eq(
        raw_profile(json!(["unknown"])),
        validation(
            "unknown_task_strength",
            Some("flash"),
            "taskStrengths contains an unknown enum member",
        ),
    );
}

#[test]
fn codex_subagent_v2_nfkc_collision_rejects_all_profiles_and_emits_no_roles() {
    let mut a = profile("Ｆｏｏ", "Ｆｏｏ");
    let b = profile("foo", "foo");
    a.overrides.role_name = Some("fullwidth");
    assert_compile_eq(
        &request(config("balanced", vec![a, b])),
        Ok(output(
            vec![],
            vec![
                ProfileStatus {
                    key: "Ｆｏｏ",
                    model: "Ｆｏｏ",
                    status: "collision",
                    reason: Some("normalized key foo conflicts"),
                },
                ProfileStatus {
                    key: "foo",
                    model: "foo",
                    status: "collision",
                    reason: Some("normalized key foo conflicts"),
                },
            ],
        )),
    );
}

#[test]
fn codex_subagent_v2_default_case_fold_collision_preserves_original_model_spelling() {
    assert_compile_eq(
        &request(config(
            "balanced",
            vec![profile("Straße", "Straße"), profile("STRASSE", "STRASSE")],
        )),
        Ok(output(
            vec![],
            vec![
                ProfileStatus {
                    key: "Straße",
                    model: "Straße",
                    status: "collision",
                    reason: Some("normalized key strasse conflicts"),
                },
                ProfileStatus {
                    key: "STRASSE",
                    model: "STRASSE",
                    status: "collision",
                    reason: Some("normalized key strasse conflicts"),
                },
            ],
        )),
    );
}

fn effort_case(
    strength: &'static str,
    optimization: &'static str,
    override_effort: Option<&'static str>,
    expected: &'static str,
) {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.strengths = vec![strength];
    p.optimization = optimization;
    p.overrides.model_reasoning_effort = override_effort;
    assert_compile_eq(
        &request(config("balanced", vec![p])),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "generated",
                "generated",
                vec!["Flash"],
                expected,
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_architecture_effort_is_high() {
    effort_case("architecture_design", "quality", None, "high");
}
#[test]
fn codex_subagent_v2_speed_reading_effort_is_low() {
    effort_case("repository_exploration", "speed", None, "low");
}
#[test]
fn codex_subagent_v2_speed_testing_effort_is_medium() {
    effort_case("testing", "speed", None, "medium");
}
#[test]
fn codex_subagent_v2_explicit_xhigh_effort_wins() {
    effort_case("architecture_design", "quality", Some("xhigh"), "xhigh");
}

#[test]
fn codex_subagent_v2_balanced_generates_unbiased_third_party_selection_text() {
    assert_compile_eq(
        &request(config(
            "balanced",
            vec![profile("flash", "DeepSeek-V4-Flash")],
        )),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "Balanced selection: matching eligible profile.",
                "generated",
                vec!["Flash"],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_official_first_keeps_generated_role_on_router_provider() {
    assert_compile_eq(&request(config("official_first", vec![profile("flash", "DeepSeek-V4-Flash")])), Ok(output(vec![generated("flash", "flash", "Official-first selection: eligible third-party profile is not promoted for high-risk work.", "generated", vec!["Flash"], "low")], vec![])));
}
#[test]
fn codex_subagent_v2_third_party_first_changes_only_selection_guidance() {
    assert_compile_eq(
        &request(config(
            "third_party_first",
            vec![profile("flash", "DeepSeek-V4-Flash")],
        )),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "Third-party-first selection: matching eligible profile is promoted.",
                "generated",
                vec!["Flash"],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_preferred_profile_overrides_official_first_bias() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.preference = "preferred";
    assert_compile_eq(
        &request(config("official_first", vec![p])),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "Official-first selection: explicitly preferred profile overrides provider bias.",
                "generated",
                vec!["Flash"],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_fallback_profile_is_never_promoted() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.preference = "fallback";
    assert_compile_eq(
        &request(config("third_party_first", vec![p])),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "Fallback profile is never promoted.",
                "generated",
                vec!["Flash"],
                "low",
            )],
            vec![],
        )),
    );
}

#[test]
fn codex_subagent_v2_manual_description_replaces_generated_text() {
    let mut with_override = profile("flash", "DeepSeek-V4-Flash");
    with_override.overrides.description = Some("Manual only.");
    with_override.overrides.developer_instructions = Some("Keep this override.");
    assert_compile_eq(
        &request(config("balanced", vec![with_override])),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "Manual only.",
                "Keep this override.",
                vec!["Flash"],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_restoring_description_keeps_other_overrides() {
    let mut restored = profile("flash", "DeepSeek-V4-Flash");
    restored.overrides.developer_instructions = Some("Keep this override.");
    assert_compile_eq(
        &request(config("balanced", vec![restored])),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "Balanced selection: matching eligible profile.",
                "Keep this override.",
                vec!["Flash"],
                "low",
            )],
            vec![],
        )),
    );
}

#[test]
fn codex_subagent_v2_rejects_empty_and_builtin_role_names() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.overrides.role_name = Some("!!!");
    assert_compile_eq(
        &request(config("balanced", vec![p])),
        compile_validation(
            "empty_role_name",
            Some("flash"),
            "normalized role name is empty",
        ),
    );
}
#[test]
fn codex_subagent_v2_rejects_builtin_role_name() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.overrides.role_name = Some("Worker");
    assert_compile_eq(
        &request(config("balanced", vec![p])),
        compile_validation("builtin_role_name", Some("flash"), "worker is reserved"),
    );
}
#[test]
fn codex_subagent_v2_normalizes_foo_mixed_separators() {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.overrides.role_name = Some("Foo__-- Bar");
    assert_compile_eq(
        &request(config("balanced", vec![p])),
        Ok(output(
            vec![generated(
                "Foo__-- Bar",
                "foo-bar",
                "generated",
                "generated",
                vec!["Flash"],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_dedupes_occupied_role_names_from_request_environment() {
    let mut r = request(config(
        "balanced",
        vec![profile("flash", "DeepSeek-V4-Flash")],
    ));
    r.occupied_role_names = vec!["review", "ccswitch-review", "ccswitch-review-2"];
    r.persisted_subagent_v2
        .as_mut()
        .expect("fixture config")
        .profiles[0]
        .overrides
        .role_name = Some("review");
    assert_compile_eq(
        &r,
        Ok(output(
            vec![generated(
                "review",
                "ccswitch-review-3",
                "generated",
                "generated",
                vec!["Flash"],
                "low",
            )],
            vec![],
        )),
    );
}

fn nickname_request(candidates: Vec<&'static str>) -> CompileRequest {
    let mut p = profile("flash", "DeepSeek-V4-Flash");
    p.overrides.nickname_candidates = Some(candidates);
    request(config("balanced", vec![p]))
}
#[test]
fn codex_subagent_v2_rejects_zero_nicknames() {
    assert_compile_eq(
        &nickname_request(vec![]),
        compile_validation(
            "nickname_count",
            Some("flash"),
            "nicknameCandidates must contain 1 through 3 entries",
        ),
    );
}
#[test]
fn codex_subagent_v2_accepts_one_nickname() {
    assert_compile_eq(
        &nickname_request(vec!["One"]),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "generated",
                "generated",
                vec!["One"],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_accepts_three_nicknames() {
    assert_compile_eq(
        &nickname_request(vec!["One", "Two", "Three"]),
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "generated",
                "generated",
                vec!["One", "Two", "Three"],
                "low",
            )],
            vec![],
        )),
    );
}
#[test]
fn codex_subagent_v2_rejects_four_nicknames() {
    assert_compile_eq(
        &nickname_request(vec!["One", "Two", "Three", "Four"]),
        compile_validation(
            "nickname_count",
            Some("flash"),
            "nicknameCandidates must contain 1 through 3 entries",
        ),
    );
}
#[test]
fn codex_subagent_v2_rejects_empty_nickname() {
    assert_compile_eq(
        &nickname_request(vec![""]),
        compile_validation("empty_nickname", Some("flash"), "nickname must be nonempty"),
    );
}
#[test]
fn codex_subagent_v2_rejects_duplicate_nickname() {
    assert_compile_eq(
        &nickname_request(vec!["Dup", "Dup"]),
        compile_validation(
            "duplicate_nickname",
            Some("flash"),
            "nicknameCandidates must be unique",
        ),
    );
}
#[test]
fn codex_subagent_v2_rejects_invalid_nickname_characters() {
    assert_compile_eq(
        &nickname_request(vec!["Bad!"]),
        compile_validation(
            "invalid_nickname",
            Some("flash"),
            "nickname uses only ASCII alphanumeric, space, dash, underscore",
        ),
    );
}

#[test]
fn codex_subagent_v2_v1_preserves_profiles_but_generates_no_v2_roles() {
    let saved = config("balanced", vec![profile("flash", "DeepSeek-V4-Flash")]);
    let mut r = request(saved.clone());
    r.subagent_version = "v1";
    assert_eq!(r.persisted_subagent_v2, Some(saved));
    assert_compile_eq(
        &r,
        Ok(output(
            vec![],
            vec![ProfileStatus {
                key: "flash",
                model: "DeepSeek-V4-Flash",
                status: "inactive_v1",
                reason: Some("V2 profiles are preserved"),
            }],
        )),
    );
}
#[test]
fn codex_subagent_v2_v2_generates_only_enabled_routable_profiles() {
    let mut disabled = profile("disabled", "disabled-model");
    disabled.enabled = false;
    let unroutable = profile("unroutable", "missing-model");
    let mut r = request(config(
        "balanced",
        vec![profile("flash", "DeepSeek-V4-Flash"), disabled, unroutable],
    ));
    r.catalog_models.push(CatalogModel {
        model: "disabled-model",
        provider_kind: "third_party",
        routable: true,
        context_window: 1,
    });
    assert_compile_eq(
        &r,
        Ok(output(
            vec![generated(
                "flash",
                "flash",
                "Balanced selection: matching eligible profile.",
                "generated",
                vec!["Flash"],
                "low",
            )],
            vec![
                ProfileStatus {
                    key: "flash",
                    model: "DeepSeek-V4-Flash",
                    status: "routable",
                    reason: None,
                },
                ProfileStatus {
                    key: "disabled",
                    model: "disabled-model",
                    status: "disabled",
                    reason: Some("disabled"),
                },
                ProfileStatus {
                    key: "unroutable",
                    model: "missing-model",
                    status: "unroutable",
                    reason: Some("not in catalog"),
                },
            ],
        )),
    );
}
#[test]
fn codex_subagent_v2_alias_refresh_changes_catalog_not_persisted_profile() {
    let saved = config("balanced", vec![profile("flash-key", "DeepSeek-V4-Flash")]);
    let mut r = request(saved.clone());
    r.catalog_models[0].model = "deepseek-flash-alias";
    assert_eq!(r.persisted_subagent_v2, Some(saved));
    assert_compile_eq(
        &r,
        Ok(output(
            vec![],
            vec![ProfileStatus {
                key: "flash-key",
                model: "DeepSeek-V4-Flash",
                status: "unroutable",
                reason: Some("catalog alias changed"),
            }],
        )),
    );
}

#[test]
fn codex_subagent_v2_missing_config_retains_legacy_behavior_without_auto_initialization() {
    let r = CompileRequest {
        subagent_version: "v2",
        persisted_subagent_v2: None,
        catalog_models: vec![flash_catalog()],
        occupied_role_names: vec![],
    };
    assert_compile_eq(&r, Ok(output(vec![], vec![])));
}
#[test]
fn codex_subagent_v2_explicit_legacy_initialize_returns_exact_flash_and_pro_presets() {
    let mut flash = profile("deepseek-v4-flash", "deepseek-v4-flash");
    flash.strengths = vec![
        "long_context_reading",
        "repository_exploration",
        "evidence_collection",
        "summarization",
        "testing",
    ];
    flash.reasoning_effort = "medium";
    let mut pro = profile("deepseek-v4-pro", "deepseek-v4-pro");
    pro.strengths = vec![
        "complex_debugging",
        "architecture_design",
        "complex_implementation",
        "high_risk_review",
        "testing",
    ];
    pro.optimization = "quality";
    pro.write_scope = "complex_changes";
    pro.reasoning_effort = "high";
    assert_eq!(
        initialize_legacy_subagent_v2_for_red_test(),
        Ok(config("balanced", vec![flash, pro]))
    );
}

#[test]
fn codex_subagent_v2_diagnostic_sanitizer_allows_only_metadata() {
    let source = DiagnosticSource {
        model: "DeepSeek-V4-Flash",
        role: "flash",
        policy: "balanced",
        status: "routable",
        reason: "enabled",
        api_key: "secret",
        task_body: "secret task",
        encrypted_content: "secret ciphertext",
    };
    assert_eq!(
        sanitize_subagent_v2_diagnostic_for_red_test(&source),
        Ok(Diagnostic {
            model: "DeepSeek-V4-Flash",
            role: Some("flash"),
            policy: "balanced",
            status: "routable",
            reason: Some("enabled")
        })
    );
}
