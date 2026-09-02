use serde_json::json;

use super::{redact_json_probe_response, redact_sse_probe_response};

#[test]
fn json_evidence_keeps_only_allowlisted_structure_and_fingerprints() {
    let evidence = redact_json_probe_response(
        200,
        &json!({
            "id": "chatcmpl-secret-id",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "reasoning_content": "private reasoning text",
                    "content": "private assistant content",
                    "tool_calls": [{
                        "id": "call-secret",
                        "function": {
                            "name": "ccsm_protocol_compatibility_probe",
                            "arguments": "{\\\"nonce\\\":\\\"private-tool-argument\\\"}"
                        }
                    }]
                }
            }],
            "authorization": "Bearer private-token"
        }),
    );

    let serialized = serde_json::to_string(&evidence).unwrap();
    assert_eq!(evidence.status_code, 200);
    assert!(evidence
        .paths
        .contains(&"choices[].message.reasoning_content".to_owned()));
    assert!(evidence
        .paths
        .contains(&"choices[].message.content".to_owned()));
    assert!(evidence
        .paths
        .contains(&"choices[].message.tool_calls[].function.arguments".to_owned()));
    assert!(!serialized.contains("private reasoning text"));
    assert!(!serialized.contains("private assistant content"));
    assert!(!serialized.contains("private-tool-argument"));
    assert!(!serialized.contains("private-token"));
    assert!(!serialized.contains("chatcmpl-secret-id"));
}

#[test]
fn sse_evidence_keeps_event_types_and_redacts_delta_payloads() {
    let evidence = redact_sse_probe_response(
        200,
        "event: message\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"private thought\"}}]}\n\n\
         data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\"private nonce\"}}]}}]}\n\n\
         data: [DONE]\n\n",
    );

    let serialized = serde_json::to_string(&evidence).unwrap();
    assert_eq!(evidence.status_code, 200);
    assert_eq!(evidence.event_types, vec!["message", "data", "done"]);
    assert!(evidence
        .paths
        .contains(&"choices[].delta.reasoning_content".to_owned()));
    assert!(evidence
        .paths
        .contains(&"choices[].delta.tool_calls[].function.arguments".to_owned()));
    assert!(!serialized.contains("private thought"));
    assert!(!serialized.contains("private nonce"));
}

#[test]
fn json_evidence_records_only_numeric_probe_usage() {
    let evidence = redact_json_probe_response(
        200,
        &json!({
            "id": "response-secret-id",
            "model": "private-upstream-model",
            "usage": {
                "input_tokens": 120,
                "output_tokens": 30,
                "total_tokens": 150,
                "input_tokens_details": { "cached_tokens": 20 }
            },
            "output": [{"content": [{"type": "output_text", "text": "private text"}]}]
        }),
    );

    assert_eq!(
        evidence.usage,
        Some(super::redaction::ProbeTokenUsage {
            input_tokens: 120,
            output_tokens: 30,
            cache_read_tokens: 20,
            cache_creation_tokens: 0,
            total_tokens: 150,
        })
    );
    let serialized = serde_json::to_string(&evidence).unwrap();
    assert!(!serialized.contains("response-secret-id"));
    assert!(!serialized.contains("private-upstream-model"));
    assert!(!serialized.contains("private text"));
}

#[test]
fn sse_evidence_records_completed_response_usage() {
    let evidence = redact_sse_probe_response(
        200,
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"private\"}\n\n\
         event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":80,\"output_tokens\":20,\"total_tokens\":100}}}\n\n",
    );

    assert_eq!(
        evidence.usage.as_ref().map(|usage| usage.total_tokens),
        Some(100)
    );
    assert_eq!(
        evidence.usage.as_ref().map(|usage| usage.input_tokens),
        Some(80)
    );
    assert_eq!(
        evidence.usage.as_ref().map(|usage| usage.output_tokens),
        Some(20)
    );
}
