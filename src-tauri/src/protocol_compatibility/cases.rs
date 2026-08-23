use super::{build_logical_probe_request, ProbeCase};

#[test]
fn baseline_json_is_a_bounded_static_responses_request() {
    let request =
        build_logical_probe_request(ProbeCase::BaselineJson, "qwen3.8", "ignored-by-baseline");

    assert_eq!(request["model"], "qwen3.8");
    assert_eq!(request["stream"], false);
    assert_eq!(request["store"], false);
    assert_eq!(request["max_output_tokens"], 128);
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
    assert_eq!(sse_request["max_output_tokens"], 128);
}

#[test]
fn tool_continuation_reserves_a_non_streaming_shell_without_new_user_input() {
    let request = build_logical_probe_request(ProbeCase::ToolContinuationJson, "qwen3.8", "unused");

    assert_eq!(request["model"], "qwen3.8");
    assert_eq!(request["stream"], false);
    assert_eq!(request["store"], false);
    assert_eq!(request["max_output_tokens"], 128);
    assert!(request.get("input").is_none());
    assert!(request.get("tools").is_none());
    assert!(request.get("tool_choice").is_none());
}

#[test]
fn forced_tool_request_owns_a_single_non_strict_nonce_tool() {
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
        request["tools"][0]["parameters"]["required"],
        serde_json::json!(["nonce"])
    );
    assert_eq!(request["tool_choice"]["type"], "function");
    assert_eq!(
        request["tool_choice"]["name"],
        "ccsm_protocol_compatibility_probe"
    );
    assert!(request["input"][0]["content"][0]["text"]
        .as_str()
        .is_some_and(|text| text.contains("run-4f8ad2d0")));
}
