use serde_json::json;

use super::{classify_reasoning_shape, PreToolVisibleContent, ReasoningSemantic, ReasoningSource};

#[test]
fn qwen_reasoning_content_is_readable_without_promoting_pre_tool_content() {
    let shape = classify_reasoning_shape(&[
        json!({"choices":[{"delta":{"reasoning_content":"think"}}]}),
        json!({"choices":[{"delta":{"content":"visible before tool"}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"id":"call_1"}]}}]}),
    ]);

    assert_eq!(shape.semantic, ReasoningSemantic::Readable);
    assert_eq!(shape.source, ReasoningSource::ReasoningContent);
    assert_eq!(
        shape.pre_tool_visible_content,
        PreToolVisibleContent::Present
    );
}

#[test]
fn summary_only_native_responses_evidence_stays_summary() {
    let shape = classify_reasoning_shape(&[json!({
        "output": [{
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "brief rationale"}]
        }]
    })]);

    assert_eq!(shape.semantic, ReasoningSemantic::Summary);
    assert_eq!(shape.source, ReasoningSource::NativeResponses);
}

#[test]
fn encrypted_or_mixed_evidence_never_becomes_readable() {
    let encrypted = classify_reasoning_shape(&[json!({
        "output": [{"type": "reasoning", "encrypted_content": "opaque"}]
    })]);
    let mixed = classify_reasoning_shape(&[
        json!({"choices":[{"delta":{"reasoning_content":"raw"}}]}),
        json!({"output":[{"type":"reasoning","summary":[{"text":"summary"}]}]}),
    ]);

    assert_eq!(encrypted.semantic, ReasoningSemantic::Opaque);
    assert_ne!(mixed.semantic, ReasoningSemantic::Readable);
}

#[test]
fn split_think_tags_are_readable_only_after_the_closing_boundary_arrives() {
    let shape = classify_reasoning_shape(&[
        json!({"choices":[{"delta":{"content":"<th"}}]}),
        json!({"choices":[{"delta":{"content":"ink>internal"}}]}),
        json!({"choices":[{"delta":{"content":"</think>answer"}}]}),
    ]);

    assert_eq!(shape.semantic, ReasoningSemantic::Readable);
    assert_eq!(shape.source, ReasoningSource::ThinkTags);
}

#[test]
fn ordinary_baseline_content_is_not_labeled_as_pre_tool_content_without_a_tool_call() {
    let shape = classify_reasoning_shape(&[json!({
        "choices": [{"message": {"content": "ordinary final answer"}}]
    })]);

    assert_eq!(
        shape.pre_tool_visible_content,
        PreToolVisibleContent::Absent
    );
}
