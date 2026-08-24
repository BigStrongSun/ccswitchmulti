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
