//! Provider-boundary compatibility for Codex Multi-Agent V2 messages.

use crate::proxy::error::ProxyError;
use serde_json::Value;

/// Project Codex-private `agent_message` items into third-party Responses input.
///
/// The RED implementation intentionally leaves the request unchanged; the tests
/// below define plaintext delivery, legacy recovery, and opaque fail-closed rules.
pub(crate) fn project_codex_agent_messages_for_third_party(
    _body: &mut Value,
) -> Result<usize, ProxyError> {
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use serde_json::json;

    #[test]
    fn projects_plaintext_agent_message_to_standard_user_message() {
        let task = "Message Type: NEW_TASK\nTask name: /root/qwen\nSender: /root\nPayload:\nNONCE_7F3 read Cargo.toml";
        let mut request = json!({
            "input": [
                {
                    "type": "message",
                    "role": "developer",
                    "content": [{"type": "input_text", "text": "keep"}]
                },
                {
                    "type": "agent_message",
                    "author": "/root",
                    "recipient": "/root/qwen",
                    "content": [{"type": "input_text", "text": task}]
                }
            ]
        });

        let changed = project_codex_agent_messages_for_third_party(&mut request).unwrap();

        assert_eq!(changed, 1);
        assert_eq!(request["input"][0]["role"], "developer");
        assert_eq!(
            request["input"][1],
            json!({
                "type": "message",
                "role": "user",
                "content": [{"type": "input_text", "text": task}]
            })
        );
    }

    #[test]
    fn recovers_legacy_plaintext_mislabeled_as_encrypted_content() {
        let mut request = json!({
            "input": [{
                "type": "agent_message",
                "author": "/root/worker",
                "recipient": "/root",
                "content": [
                    {"type": "input_text", "text": "Message Type: FINAL_ANSWER\nPayload:\n"},
                    {"type": "encrypted_content", "encrypted_content": "已完成，结果为 42。"}
                ]
            }]
        });

        let changed = project_codex_agent_messages_for_third_party(&mut request).unwrap();

        assert_eq!(changed, 1);
        assert_eq!(request["input"][0]["type"], "message");
        assert_eq!(request["input"][0]["role"], "user");
        assert_eq!(
            request["input"][0]["content"],
            json!([
                {"type": "input_text", "text": "Message Type: FINAL_ANSWER\nPayload:\n"},
                {"type": "input_text", "text": "已完成，结果为 42。"}
            ])
        );
    }

    #[test]
    fn rejects_opaque_agent_ciphertext_without_echoing_it() {
        let opaque = URL_SAFE_NO_PAD.encode([7_u8; 96]);
        let mut request = json!({
            "input": [{
                "type": "agent_message",
                "author": "/root",
                "recipient": "/root/deepseek",
                "content": [
                    {"type": "input_text", "text": "Message Type: NEW_TASK\nTask name: /root/deepseek\nSender: /root\nPayload:\n"},
                    {"type": "encrypted_content", "encrypted_content": opaque}
                ]
            }]
        });

        let error = project_codex_agent_messages_for_third_party(&mut request)
            .expect_err("opaque OpenAI task content must fail closed");
        let message = error.to_string();

        assert!(message.contains("third-party child cannot read encrypted Codex agent payload"));
        assert!(!message.contains(&opaque));
    }
}

