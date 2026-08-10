//! RED-only typed contract for the future Codex V2 capability-profile compiler.
//!
//! This is declared once from crate root under `cfg(test)`. Task 3 can promote this exact module
//! to production without changing its identity. The sentinel deliberately has no production caller.

use std::collections::BTreeMap;

use serde_json::{json, Value};

#[derive(Debug, PartialEq, Eq)]
enum CompileError {
    NotImplemented,
    Validation {
        code: &'static str,
        profile_key: Option<&'static str>,
        detail: &'static str,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct CompileOutput {
    persisted_profiles: Vec<ProfileStatus>,
    generated_roles: Vec<GeneratedRole>,
    initialized: Option<InitializedPresets>,
    diagnostics: DiagnosticPayload,
}

#[derive(Debug, PartialEq, Eq)]
struct ProfileStatus {
    key: &'static str,
    model: &'static str,
    status: &'static str,
    reason: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
struct GeneratedRole {
    name: &'static str,
    model: &'static str,
    provider: &'static str,
    effort: &'static str,
    description: &'static str,
    nicknames: Vec<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
struct InitializedPresets {
    selection_policy: &'static str,
    flash: &'static str,
    pro: &'static str,
}

#[derive(Debug, PartialEq, Eq)]
struct DiagnosticPayload {
    fields: BTreeMap<&'static str, &'static str>,
}

type CompileResult = Result<CompileOutput, CompileError>;

/// Test-only wished-for compiler boundary. Its intentional failure prevents an `Ok(null)` or an
/// untyped placeholder from satisfying this RED suite.
fn compile_subagent_v2_profiles_for_red_test(_settings: &Value) -> CompileResult {
    Err(CompileError::NotImplemented)
}

fn diagnostics(entries: &[(&'static str, &'static str)]) -> DiagnosticPayload {
    DiagnosticPayload {
        fields: entries.iter().copied().collect(),
    }
}

fn assert_compile_eq(input: Value, expected: CompileResult) {
    let actual = compile_subagent_v2_profiles_for_red_test(&input);
    assert_eq!(actual, expected);
}

fn profile(
    model: &'static str,
    status: &'static str,
    reason: Option<&'static str>,
) -> ProfileStatus {
    ProfileStatus {
        key: model,
        model,
        status,
        reason,
    }
}

fn empty_output(profiles: Vec<ProfileStatus>) -> CompileOutput {
    CompileOutput {
        persisted_profiles: profiles,
        generated_roles: vec![],
        initialized: None,
        diagnostics: diagnostics(&[]),
    }
}

fn v2_profile(strengths: Value, overrides: Value) -> Value {
    json!({
        "codexRouting": {
            "subagentVersion": "v2",
            "subagentV2": {
                "schemaVersion": 1,
                "selectionPolicy": "balanced",
                "profiles": { "flash": {
                    "model": "DeepSeek-V4-Flash",
                    "enabled": true,
                    "questionnaire": {
                        "taskStrengths": strengths,
                        "optimization": "speed",
                        "writeScope": "read_only",
                        "preference": "eligible",
                        "reasoningEffort": "auto"
                    },
                    "overrides": overrides
                }}
            }
        }
    })
}

fn v2_profile_with_policy(policy: &'static str, preference: &'static str) -> Value {
    let mut input = v2_profile(json!(["testing"]), json!({}));
    input["codexRouting"]["subagentV2"]["selectionPolicy"] = json!(policy);
    input["codexRouting"]["subagentV2"]["profiles"]["flash"]["questionnaire"]["preference"] =
        json!(preference);
    input
}

fn validation(
    code: &'static str,
    profile_key: Option<&'static str>,
    detail: &'static str,
) -> CompileResult {
    Err(CompileError::Validation {
        code,
        profile_key,
        detail,
    })
}

#[test]
fn codex_subagent_v2_rejects_non_v1_schema_version() {
    let mut input = v2_profile(json!(["repository_exploration"]), json!({}));
    input["codexRouting"]["subagentV2"]["schemaVersion"] = json!(2);
    assert_compile_eq(
        input,
        validation(
            "unsupported_schema_version",
            None,
            "schemaVersion must equal 1",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_illegal_questionnaire_enum() {
    let mut input = v2_profile(json!(["repository_exploration"]), json!({}));
    input["codexRouting"]["subagentV2"]["profiles"]["flash"]["questionnaire"]["optimization"] =
        json!("fastest");
    assert_compile_eq(
        input,
        validation(
            "invalid_optimization",
            Some("flash"),
            "optimization is not an allowed enum member",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_missing_required_questionnaire_field() {
    let mut input = v2_profile(json!(["repository_exploration"]), json!({}));
    input["codexRouting"]["subagentV2"]["profiles"]["flash"]["questionnaire"]
        .as_object_mut()
        .expect("fixture questionnaire")
        .remove("writeScope");
    assert_compile_eq(
        input,
        validation(
            "missing_write_scope",
            Some("flash"),
            "writeScope is required",
        ),
    );
}

#[test]
fn codex_subagent_v2_defaults_only_documented_fields_and_round_trips_overrides() {
    let input = v2_profile(
        json!(["repository_exploration"]),
        json!({
            "roleName": "flash-reader",
            "description": "Manual selection text.",
            "developerInstructions": "Read only.",
            "nicknameCandidates": ["Flash Reader"],
            "modelReasoningEffort": "xhigh"
        }),
    );
    assert_compile_eq(
        input,
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![GeneratedRole {
                name: "flash-reader",
                model: "DeepSeek-V4-Flash",
                provider: "codex_model_router_v2",
                effort: "xhigh",
                description: "Manual selection text.",
                nicknames: vec!["Flash Reader"],
            }],
            initialized: None,
            diagnostics: diagnostics(&[("model", "DeepSeek-V4-Flash"), ("status", "routable")]),
        }),
    );
}

#[test]
fn codex_subagent_v2_rejects_zero_strengths() {
    assert_compile_eq(
        v2_profile(json!([]), json!({})),
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
        v2_profile(json!(["testing"]), json!({})),
        Ok(empty_output(vec![profile(
            "DeepSeek-V4-Flash",
            "routable",
            None,
        )])),
    );
}

#[test]
fn codex_subagent_v2_accepts_five_strengths() {
    assert_compile_eq(
        v2_profile(
            json!([
                "long_context_reading",
                "repository_exploration",
                "evidence_collection",
                "summarization",
                "testing"
            ]),
            json!({}),
        ),
        Ok(empty_output(vec![profile(
            "DeepSeek-V4-Flash",
            "routable",
            None,
        )])),
    );
}

#[test]
fn codex_subagent_v2_rejects_six_strengths() {
    assert_compile_eq(
        v2_profile(
            json!([
                "long_context_reading",
                "repository_exploration",
                "evidence_collection",
                "summarization",
                "testing",
                "complex_debugging"
            ]),
            json!({}),
        ),
        validation(
            "strength_count",
            Some("flash"),
            "taskStrengths must contain 1 through 5 members",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_duplicate_and_unknown_strengths() {
    assert_compile_eq(
        v2_profile(json!(["testing", "testing", "unknown"]), json!({})),
        validation(
            "invalid_task_strengths",
            Some("flash"),
            "taskStrengths must be unique known enum members",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_unknown_strength() {
    assert_compile_eq(
        v2_profile(json!(["unknown"]), json!({})),
        validation(
            "unknown_task_strength",
            Some("flash"),
            "taskStrengths contains an unknown enum member",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_all_nfkc_collisions_and_keeps_original_models() {
    let mut input = v2_profile(json!(["testing"]), json!({}));
    let profiles = input["codexRouting"]["subagentV2"]["profiles"]
        .as_object_mut()
        .expect("fixture profiles");
    let fullwidth = profiles.remove("flash").expect("fixture profile");
    profiles.insert("Ｆｏｏ".to_string(), fullwidth);
    profiles.insert("foo".to_string(), json!({
        "model": "foo", "enabled": true,
        "questionnaire": { "taskStrengths": ["testing"], "optimization": "speed", "writeScope": "read_only", "preference": "eligible", "reasoningEffort": "auto" }
    }));
    assert_compile_eq(
        input,
        Ok(empty_output(vec![
            ProfileStatus {
                key: "Ｆｏｏ",
                model: "DeepSeek-V4-Flash",
                status: "collision",
                reason: Some("normalized key foo conflicts"),
            },
            ProfileStatus {
                key: "foo",
                model: "foo",
                status: "collision",
                reason: Some("normalized key foo conflicts"),
            },
        ])),
    );
}

#[test]
fn codex_subagent_v2_rejects_all_default_case_fold_collisions() {
    let input = json!({ "codexRouting": { "subagentV2": {
        "schemaVersion": 1, "selectionPolicy": "balanced", "profiles": {
            "Straße": { "model": "Straße", "enabled": true, "questionnaire": { "taskStrengths": ["testing"], "optimization": "speed", "writeScope": "read_only", "preference": "eligible", "reasoningEffort": "auto" } },
            "STRASSE": { "model": "STRASSE", "enabled": true, "questionnaire": { "taskStrengths": ["testing"], "optimization": "speed", "writeScope": "read_only", "preference": "eligible", "reasoningEffort": "auto" } }
        }
    }}});
    assert_compile_eq(
        input,
        Ok(empty_output(vec![
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
        ])),
    );
}

fn effort_expected(effort: &'static str) -> CompileResult {
    Ok(CompileOutput {
        persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
        generated_roles: vec![GeneratedRole {
            name: "flash",
            model: "DeepSeek-V4-Flash",
            provider: "codex_model_router_v2",
            effort,
            description: "generated",
            nicknames: vec!["Flash"],
        }],
        initialized: None,
        diagnostics: diagnostics(&[]),
    })
}

#[test]
fn codex_subagent_v2_effort_architecture_is_high() {
    assert_compile_eq(
        v2_profile(json!(["architecture_design"]), json!({})),
        effort_expected("high"),
    );
}

#[test]
fn codex_subagent_v2_effort_speed_read_explore_evidence_summarize_is_low() {
    assert_compile_eq(
        v2_profile(
            json!([
                "long_context_reading",
                "repository_exploration",
                "evidence_collection",
                "summarization"
            ]),
            json!({}),
        ),
        effort_expected("low"),
    );
}

#[test]
fn codex_subagent_v2_effort_speed_with_testing_is_medium() {
    assert_compile_eq(
        v2_profile(json!(["repository_exploration", "testing"]), json!({})),
        effort_expected("medium"),
    );
}

#[test]
fn codex_subagent_v2_explicit_effort_xhigh_wins() {
    assert_compile_eq(
        v2_profile(
            json!(["architecture_design"]),
            json!({ "modelReasoningEffort": "xhigh" }),
        ),
        effort_expected("xhigh"),
    );
}

#[test]
fn codex_subagent_v2_policy_balanced_has_no_provider_bias() {
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({})),
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![GeneratedRole {
                name: "balanced",
                model: "DeepSeek-V4-Flash",
                provider: "codex_model_router_v2",
                effort: "medium",
                description: "balanced: no provider bias",
                nicknames: vec!["Balanced"],
            }],
            initialized: None,
            diagnostics: diagnostics(&[("policy", "balanced")]),
        }),
    );
}

#[test]
fn codex_subagent_v2_policy_official_first_keeps_high_risk_official_unless_preferred() {
    assert_compile_eq(
        v2_profile(json!(["high_risk_review"]), json!({})),
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![GeneratedRole {
                name: "official-control",
                model: "official",
                provider: "openai",
                effort: "high",
                description: "official_first high-risk control",
                nicknames: vec!["Official"],
            }],
            initialized: None,
            diagnostics: diagnostics(&[("policy", "official_first"), ("preference", "eligible")]),
        }),
    );
}

#[test]
fn codex_subagent_v2_policy_third_party_first_promotes_preferred_or_eligible_not_fallback() {
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({})),
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![GeneratedRole {
                name: "third-party",
                model: "DeepSeek-V4-Flash",
                provider: "codex_model_router_v2",
                effort: "medium",
                description: "third_party_first preferred/eligible",
                nicknames: vec!["Third Party"],
            }],
            initialized: None,
            diagnostics: diagnostics(&[
                ("policy", "third_party_first"),
                ("fallback", "never_promoted"),
            ]),
        }),
    );
}

#[test]
fn codex_subagent_v2_preferred_profile_overrides_official_first_provider_bias() {
    assert_compile_eq(
        v2_profile_with_policy("official_first", "preferred"),
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![GeneratedRole {
                name: "preferred-third-party",
                model: "DeepSeek-V4-Flash",
                provider: "codex_model_router_v2",
                effort: "medium",
                description: "preferred profile overrides official_first provider bias",
                nicknames: vec!["Preferred"],
            }],
            initialized: None,
            diagnostics: diagnostics(&[("policy", "official_first"), ("preference", "preferred")]),
        }),
    );
}

#[test]
fn codex_subagent_v2_fallback_profile_is_never_promoted_by_third_party_first() {
    assert_compile_eq(
        v2_profile_with_policy("third_party_first", "fallback"),
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![],
            initialized: None,
            diagnostics: diagnostics(&[
                ("policy", "third_party_first"),
                ("preference", "fallback"),
                ("promotion", "none"),
            ]),
        }),
    );
}

#[test]
fn codex_subagent_v2_manual_description_exactly_replaces_generated_policy_text() {
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "description": "Manual only. Exclude generated policy selection text." }),
        ),
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![GeneratedRole {
                name: "flash",
                model: "DeepSeek-V4-Flash",
                provider: "codex_model_router_v2",
                effort: "medium",
                description: "Manual only. Exclude generated policy selection text.",
                nicknames: vec!["Flash"],
            }],
            initialized: None,
            diagnostics: diagnostics(&[("generated_policy_text", "absent")]),
        }),
    );
}

#[test]
fn codex_subagent_v2_rejects_empty_and_builtin_role_names() {
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({ "roleName": "!!!" })),
        validation(
            "empty_role_name",
            Some("flash"),
            "normalized role name is empty",
        ),
    );
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({ "roleName": "Worker" })),
        validation("builtin_role_name", Some("flash"), "worker is reserved"),
    );
}

#[test]
fn codex_subagent_v2_rejects_builtin_default_role_name() {
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({ "roleName": "default" })),
        validation("builtin_role_name", Some("flash"), "default is reserved"),
    );
}

#[test]
fn codex_subagent_v2_normalizes_mixed_role_separators_and_dedupes_occupied_names() {
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "roleName": "Foo__-- Bar", "occupiedRoleNames": ["review", "ccswitch-review", "ccswitch-review-2"], "requestedRoleName": "review" }),
        ),
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![GeneratedRole {
                name: "ccswitch-review-3",
                model: "DeepSeek-V4-Flash",
                provider: "codex_model_router_v2",
                effort: "medium",
                description: "generated",
                nicknames: vec!["Flash"],
            }],
            initialized: None,
            diagnostics: diagnostics(&[
                ("normalizedFoo", "foo-bar"),
                ("effectiveRoleName", "ccswitch-review-3"),
            ]),
        }),
    );
}

#[test]
fn codex_subagent_v2_validates_nickname_count_empty_duplicate_and_characters() {
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({ "nicknameCandidates": [] })),
        validation(
            "nickname_count",
            Some("flash"),
            "nicknameCandidates must contain 1 through 3 entries",
        ),
    );
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "nicknameCandidates": ["One", "Two", "Three", "Four"] }),
        ),
        validation(
            "nickname_count",
            Some("flash"),
            "nicknameCandidates must contain 1 through 3 entries",
        ),
    );
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({ "nicknameCandidates": [""] })),
        validation("empty_nickname", Some("flash"), "nickname must be nonempty"),
    );
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "nicknameCandidates": ["Dup", "Dup"] }),
        ),
        validation(
            "duplicate_nickname",
            Some("flash"),
            "nicknameCandidates must be unique",
        ),
    );
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "nicknameCandidates": ["Bad!"] }),
        ),
        validation(
            "invalid_nickname",
            Some("flash"),
            "nickname uses only ASCII alphanumeric, space, dash, underscore",
        ),
    );
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "nicknameCandidates": ["One", "Two", "Three"] }),
        ),
        Ok(empty_output(vec![profile(
            "DeepSeek-V4-Flash",
            "routable",
            None,
        )])),
    );
}

#[test]
fn codex_subagent_v2_accepts_one_valid_nickname() {
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({ "nicknameCandidates": ["One"] })),
        Ok(empty_output(vec![profile(
            "DeepSeek-V4-Flash",
            "routable",
            None,
        )])),
    );
}

#[test]
fn codex_subagent_v2_accepts_three_valid_nicknames() {
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "nicknameCandidates": ["One", "Two", "Three"] }),
        ),
        Ok(empty_output(vec![profile(
            "DeepSeek-V4-Flash",
            "routable",
            None,
        )])),
    );
}

#[test]
fn codex_subagent_v2_rejects_four_nicknames() {
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "nicknameCandidates": ["One", "Two", "Three", "Four"] }),
        ),
        validation(
            "nickname_count",
            Some("flash"),
            "nicknameCandidates must contain 1 through 3 entries",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_empty_nickname() {
    assert_compile_eq(
        v2_profile(json!(["testing"]), json!({ "nicknameCandidates": [""] })),
        validation("empty_nickname", Some("flash"), "nickname must be nonempty"),
    );
}

#[test]
fn codex_subagent_v2_rejects_duplicate_nickname() {
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "nicknameCandidates": ["Dup", "Dup"] }),
        ),
        validation(
            "duplicate_nickname",
            Some("flash"),
            "nicknameCandidates must be unique",
        ),
    );
}

#[test]
fn codex_subagent_v2_rejects_invalid_nickname_characters() {
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "nicknameCandidates": ["Bad!"] }),
        ),
        validation(
            "invalid_nickname",
            Some("flash"),
            "nickname uses only ASCII alphanumeric, space, dash, underscore",
        ),
    );
}

#[test]
fn codex_subagent_v2_lifecycle_preserves_persisted_profiles_and_materializes_only_enabled_routable_v2(
) {
    assert_compile_eq(
        json!({ "codexRouting": { "subagentVersion": "v1", "subagentV2": "profiles preserved" }, "catalogAfterAliasRefresh": "technical data only" }),
        Ok(CompileOutput {
            persisted_profiles: vec![
                profile("DeepSeek-V4-Flash", "disabled", Some("disabled")),
                profile("missing-alias", "unroutable", Some("not in catalog")),
                profile("invalid", "invalid", Some("validation error")),
            ],
            generated_roles: vec![],
            initialized: None,
            diagnostics: diagnostics(&[("mode", "v1"), ("aliasRefresh", "preserved")]),
        }),
    );
}

#[test]
fn codex_subagent_v2_mode_materializes_only_enabled_routable_profiles() {
    assert_compile_eq(
        json!({
            "codexRouting": { "subagentVersion": "v2", "subagentV2": "profiles preserved" },
            "profiles": ["enabled-routable", "disabled", "unroutable", "invalid"]
        }),
        Ok(CompileOutput {
            persisted_profiles: vec![
                profile("enabled-routable", "routable", None),
                profile("disabled", "disabled", Some("disabled")),
                profile("unroutable", "unroutable", Some("not in catalog")),
                profile("invalid", "invalid", Some("validation error")),
            ],
            generated_roles: vec![GeneratedRole {
                name: "enabled-routable",
                model: "enabled-routable",
                provider: "codex_model_router_v2",
                effort: "medium",
                description: "generated",
                nicknames: vec!["Enabled"],
            }],
            initialized: None,
            diagnostics: diagnostics(&[("mode", "v2")]),
        }),
    );
}

#[test]
fn codex_subagent_v2_legacy_initialization_has_exact_flash_and_pro_presets() {
    assert_compile_eq(json!({ "codexRouting": { "subagentVersion": "v2" } }), Ok(CompileOutput { persisted_profiles: vec![], generated_roles: vec![], initialized: Some(InitializedPresets { selection_policy: "balanced", flash: "speed|read_only|eligible|medium|long_context_reading,repository_exploration,evidence_collection,summarization,testing", pro: "quality|complex_changes|eligible|high|complex_debugging,architecture_design,complex_implementation,high_risk_review,testing" }), diagnostics: diagnostics(&[("legacy", "initialized")]) }));
}

#[test]
fn codex_subagent_v2_diagnostics_expose_only_allowed_metadata() {
    assert_compile_eq(
        v2_profile(
            json!(["testing"]),
            json!({ "apiKey": "secret", "taskBody": "secret", "encryptedContent": "secret", "arbitrarySecret": "secret" }),
        ),
        Ok(CompileOutput {
            persisted_profiles: vec![profile("DeepSeek-V4-Flash", "routable", None)],
            generated_roles: vec![],
            initialized: None,
            diagnostics: diagnostics(&[
                ("model", "DeepSeek-V4-Flash"),
                ("role", "flash"),
                ("policy", "balanced"),
                ("status", "routable"),
            ]),
        }),
    );
}
