use super::{build_logical_probe_request, ProbeCase};

#[test]
fn baseline_json_is_a_bounded_static_responses_request() {
    let request =
        build_logical_probe_request(ProbeCase::BaselineJson, "qwen3.8", "ignored-by-baseline");

    assert_eq!(request["model"], "qwen3.8");
    assert_eq!(request["stream"], false);
    assert_eq!(request["store"], false);
    assert_eq!(request["max_output_tokens"], 1024);
    assert_eq!(
        request["input"][0]["content"][0]["text"],
        "CCSM protocol compatibility probe. Solve 17 + 25 internally. Reply only CCSM_PROTOCOL_BASELINE_OK."
    );

    for forbidden in [
        "instructions",
        "tools",
        "tool_choice",
        "response_format",
        "previous_response_id",
        "conversation",
        "reasoning",
        "reasoning_effort",
        "temperature",
        "top_p",
        "seed",
        "metadata",
    ] {
        assert!(request.get(forbidden).is_none(), "unexpected {forbidden}");
    }
}

#[test]
fn baseline_sse_keeps_the_same_semantics_and_only_enables_streaming() {
    let json_request = build_logical_probe_request(ProbeCase::BaselineJson, "qwen3.8", "unused");
    let sse_request = build_logical_probe_request(ProbeCase::BaselineSse, "qwen3.8", "unused");

    assert_eq!(sse_request["stream"], true);
    assert_eq!(sse_request["model"], json_request["model"]);
    assert_eq!(sse_request["input"], json_request["input"]);
    assert_eq!(sse_request["store"], false);
    assert_eq!(sse_request["max_output_tokens"], 1024);
}

#[test]
fn tool_continuation_reserves_a_non_streaming_shell_without_new_user_input() {
    let request = build_logical_probe_request(ProbeCase::ToolContinuationJson, "qwen3.8", "unused");

    assert_eq!(request["model"], "qwen3.8");
    assert_eq!(request["stream"], false);
    assert_eq!(request["store"], false);
    assert_eq!(request["max_output_tokens"], 1024);
    assert!(request.get("input").is_none());
    assert!(request.get("tools").is_none());
    assert!(request.get("tool_choice").is_none());
}

#[test]
fn forced_tool_request_uses_one_function_without_competing_custom_tools() {
    let request = build_logical_probe_request(ProbeCase::ForcedToolSse, "qwen3.8", "run-4f8ad2d0");

    assert_eq!(request["stream"], true);
    assert_eq!(request["tools"].as_array().map(Vec::len), Some(1));
    assert_eq!(request["tools"][0]["type"], "function");
    assert_eq!(
        request["tools"][0]["name"],
        "ccsm_protocol_compatibility_probe"
    );
    assert!(request["tools"][0].get("strict").is_none());
    assert_eq!(
        request["tools"][0]["parameters"]["oneOf"][0]["required"],
        serde_json::json!(["nonce", "mode"])
    );
    assert!(request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|tool| tool["type"] == "function"));
    assert_eq!(request["tool_choice"], "auto");
    assert!(request["input"][0]["content"][0]["text"]
        .as_str()
        .is_some_and(|text| {
            text.contains("run-4f8ad2d0")
                && text.contains("ccsm_protocol_compatibility_probe")
                && text.contains("Do not answer with text")
        }));
}

#[test]
fn custom_tool_admission_uses_the_official_codex_apply_patch_contract() {
    let request =
        build_logical_probe_request(ProbeCase::CustomToolAdmissionJson, "qwen3.8", "unused");

    assert_eq!(request["stream"], false);
    assert_eq!(request["tools"].as_array().map(Vec::len), Some(1));
    assert_eq!(request["tools"][0]["type"], "custom");
    assert_eq!(request["tools"][0]["name"], "apply_patch");
    assert!(request["tools"][0].get("parameters").is_none());
    assert_eq!(
        request["tools"][0]["format"],
        serde_json::json!({
            "type": "grammar",
            "syntax": "lark",
            "definition": "start: begin_patch hunk+ end_patch\nbegin_patch: \"*** Begin Patch\" LF\nend_patch: \"*** End Patch\" LF?\n\nhunk: add_hunk | delete_hunk | update_hunk\nadd_hunk: \"*** Add File: \" filename LF add_line+\ndelete_hunk: \"*** Delete File: \" filename LF\nupdate_hunk: \"*** Update File: \" filename LF change_move? change?\n\nfilename: /(.+)/\nadd_line: \"+\" /(.*)/ LF -> line\n\nchange_move: \"*** Move to: \" filename LF\nchange: (change_context | change_line)+ eof_line?\nchange_context: (\"@@\" | \"@@ \" /(.+)/) LF\nchange_line: (\"+\" | \"-\" | \" \") /(.*)/ LF\neof_line: \"*** End of File\" LF\n\n%import common.LF\n"
        })
    );
    assert_eq!(request["tool_choice"], "auto");
    assert!(request["input"][0]["content"][0]["text"]
        .as_str()
        .is_some_and(|text| text.contains("CCSM_PROTOCOL_CUSTOM_TOOL_ADMISSION_OK")));
}

#[test]
fn forced_tool_schema_covers_real_codex_dynamic_root_union_shape() {
    let request = build_logical_probe_request(ProbeCase::ForcedToolSse, "qwen3.8", "nonce-123");
    let parameters = &request["tools"][0]["parameters"];

    assert_eq!(
        parameters,
        &serde_json::json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "nonce": {"const": "nonce-123"},
                        "mode": {"const": "direct"}
                    },
                    "required": ["nonce", "mode"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "nonce": {"const": "nonce-123"},
                        "mode": {"enum": ["routed", "suggested"]},
                        "destination": {
                            "$ref": "#/$defs/probe_destination"
                        }
                    },
                    "required": ["nonce", "mode", "destination"],
                    "additionalProperties": false
                },
                {"type": "null"}
            ],
            "$defs": {
                "probe_destination": {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string"},
                        "id": {"$ref": "#/$defs/probe_identifier"}
                    }
                },
                "probe_identifier": {
                    "$ref": "#/$defs/probe_identifier_base",
                    "type": "string",
                    "format": "uuid",
                    "minLength": 1,
                    "description": "Representative Codex dynamic-tool identifier."
                },
                "probe_identifier_base": {
                    "type": "string"
                }
            }
        })
    );
}

#[test]
fn required_tool_retry_requires_a_tool_without_specifying_one() {
    let request = build_logical_probe_request(
        ProbeCase::ForcedToolRequiredSse,
        "qwen3.8",
        "nonce-required",
    );

    assert_eq!(request["stream"], true);
    assert_eq!(request["tool_choice"], "required");
    assert_eq!(request["tools"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        request["tools"][0]["name"],
        "ccsm_protocol_compatibility_probe"
    );
    assert!(request["input"][0]["content"][0]["text"]
        .as_str()
        .is_some_and(|text| text.contains("nonce-required")));
}
