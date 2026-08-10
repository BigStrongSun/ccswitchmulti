//! RED contract tests for the planned questionnaire-driven Codex subagent V2 compiler.
//!
//! This module is test-only. `compile_subagent_v2_profiles_for_red_test` is a deliberately
//! unimplemented sentinel: production code must replace it with the real backend compiler,
//! not make these assertions pass by changing test fixtures.

use super::{render_codex_managed_agent_toml, CodexCatalogModelSpec};
use serde_json::{json, Value};

const V2_COMPILER_MISSING: &str = "Codex V2 capability-profile compiler is not implemented";

/// Test-only wished-for backend boundary. It has no production caller.
fn compile_subagent_v2_profiles_for_red_test(_settings: &Value) -> Result<Value, &'static str> {
    Err(V2_COMPILER_MISSING)
}

fn assert_v2_compiler_contract(settings: Value, contract: &str) {
    let result = compile_subagent_v2_profiles_for_red_test(&settings);
    assert!(
        result.is_ok(),
        "{contract}: {V2_COMPILER_MISSING}; RED must fail until the backend owns this contract"
    );
}

fn flash_profile(overrides: Value) -> Value {
    json!({
        "schemaVersion": 1,
        "selectionPolicy": "balanced",
        "profiles": {
            "deepseek-v4-flash": {
                "model": "DeepSeek-V4-Flash",
                "enabled": true,
                "questionnaire": {
                    "taskStrengths": ["repository_exploration", "summarization"],
                    "optimization": "speed",
                    "writeScope": "read_only",
                    "preference": "eligible",
                    "reasoningEffort": "auto"
                },
                "overrides": overrides
            }
        }
    })
}

#[test]
fn codex_subagent_v2_schema_rejects_invalid_values_and_defaults_only_the_documented_fields() {
    assert_v2_compiler_contract(
        json!({
            "codexRouting": {
                "subagentV2": {
                    "schemaVersion": 1,
                    "selectionPolicy": "balanced",
                    "profiles": {
                        "flash": {
                            "model": "deepseek-v4-flash",
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
                }
            }
        }),
        "schemaVersion=1, enum rejection/defaults, questionnaire, and field overrides must deserialize exactly",
    );
}

#[test]
fn codex_subagent_v2_validates_strengths_profile_keys_and_preserves_visible_model_spelling() {
    assert_v2_compiler_contract(
        json!({
            "codexRouting": {
                "subagentV2": {
                    "schemaVersion": 1,
                    "selectionPolicy": "balanced",
                    "profiles": {
                        "Ｆｏｏ": flash_profile(json!({}))["profiles"]["deepseek-v4-flash"].clone(),
                        "foo": flash_profile(json!({}))["profiles"]["deepseek-v4-flash"].clone(),
                        "invalid-strengths": {
                            "model": "Straße",
                            "enabled": true,
                            "questionnaire": {
                                "taskStrengths": ["testing", "testing", "unknown"],
                                "optimization": "speed",
                                "writeScope": "read_only",
                                "preference": "eligible",
                                "reasoningEffort": "auto"
                            }
                        }
                    }
                }
            }
        }),
        "taskStrengths must be 1-5 unique enum values; trim+NFKC+Default-Case-Folding collisions (including fullwidth Foo and Straße/STRASSE) reject every colliding profile, generate no role, and preserve profile.model spelling",
    );
}

#[test]
fn codex_subagent_v2_compiles_effort_policy_and_field_overrides_deterministically() {
    assert_v2_compiler_contract(
        flash_profile(json!({
            "roleName": "  Foo__-- Bar ",
            "description": "Manual description replaces every generated selection-policy sentence.",
            "developerInstructions": "Use the supplied evidence only.",
            "nicknameCandidates": ["Flash Scout", "flash-scout_2"],
            "modelReasoningEffort": "xhigh"
        })),
        "complex/debug/architecture/high-risk strengths -> high; speed plus only read/explore/evidence/summarize -> low; otherwise medium; explicit modelReasoningEffort wins; balanced/official_first/third_party_first must respect preferred, eligible, and never-promoted fallback; manual description replaces generated policy text",
    );
}

#[test]
fn codex_subagent_v2_normalizes_roles_nicknames_and_lifecycle_without_overwriting_users() {
    assert_v2_compiler_contract(
        json!({
            "codexRouting": {
                "subagentVersion": "v2",
                "subagentV2": flash_profile(json!({
                    "roleName": "review",
                    "nicknameCandidates": ["Review One", "Review-Two", "review_three"]
                }))
            },
            "existingUserRoles": ["review", "ccswitch-review", "ccswitch-review-2"],
            "catalogAfterAliasRefresh": [{ "model": "deepseek-v4-flash-alias" }]
        }),
        "role names must ASCII-normalize and reject built-ins, then dedupe review -> ccswitch-review -> ccswitch-review-2 without overwriting user files; nicknames must be 1-3 unique nonempty ASCII alphanumeric/space/dash/underscore values; disabled/unroutable profiles persist without roles; catalog refresh preserves profile data; V1 preserves V2 but materializes none, while V2 emits enabled+routable profiles only",
    );
}

#[test]
fn codex_subagent_v2_initializes_legacy_settings_and_redacts_diagnostics() {
    assert_v2_compiler_contract(
        json!({
            "legacySettingsWithoutSubagentV2": true,
            "diagnosticInput": {
                "model": "deepseek-v4-pro",
                "apiKey": "must-not-appear",
                "taskBody": "must-not-appear",
                "encryptedContent": "must-not-appear",
                "arbitrarySecret": "must-not-appear"
            }
        }),
        "legacy settings must retain legacy managed roles and deterministically initialize the exact Flash speed/read_only/eligible/medium [long_context_reading, repository_exploration, evidence_collection, summarization, testing] and Pro quality/complex_changes/eligible/high [complex_debugging, architecture_design, complex_implementation, high_risk_review, testing] presets; diagnostics may contain only model/role/policy/status metadata",
    );
}

#[test]
fn codex_subagent_v2_current_managed_agent_path_cannot_honor_questionnaire_or_manual_description() {
    let configured = flash_profile(json!({
        "description": "Only investigate protocol evidence supplied by the user; never select this role for implementation.",
        "developerInstructions": "Do not edit any files.",
        "modelReasoningEffort": "xhigh"
    }));
    let profile = &configured["profiles"]["deepseek-v4-flash"];
    let spec = CodexCatalogModelSpec {
        model: profile["model"]
            .as_str()
            .expect("fixture model")
            .to_string(),
        upstream_model: None,
        display_name: "Configured Flash".to_string(),
        context_window: 1_000_000,
        text_only: false,
        is_default: false,
        supports_parallel_tool_calls: None,
        input_modalities: None,
        base_instructions: None,
    };

    // This is the real current managed-role renderer, which calls the hardcoded
    // codex_agent_description_for_model() path rather than a test double.
    let rendered = render_codex_managed_agent_toml("configured-flash", &spec);
    assert!(
        rendered.contains(
            profile["overrides"]["description"]
                .as_str()
                .expect("fixture manual description")
        ),
        "current hardcoded managed-role rendering must be replaced by the configured V2 profile compiler so manual description/questionnaire selection is observable"
    );
    assert!(
        rendered.contains(r#"model_reasoning_effort = \"xhigh\""#),
        "configured explicit modelReasoningEffort must override the hardcoded model-derived effort"
    );
}
