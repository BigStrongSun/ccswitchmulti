//! Provider-boundary compatibility for Codex Multi-Agent V2 messages.

use crate::proxy::error::ProxyError;
use base64::Engine as _;
use serde_json::{json, Value};

/// Project Codex-private `agent_message` items into third-party Responses input.
///
pub(crate) fn project_codex_agent_messages_for_third_party(
    body: &mut Value,
) -> Result<usize, ProxyError> {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return Ok(0);
    };

    let mut changed = 0;
    for item in input {
        if item.get("type").and_then(Value::as_str) != Some("agent_message") {
            continue;
        }

        let Some(content) = item.get("content").and_then(Value::as_array) else {
            return Err(unreadable_agent_payload_error());
        };
        let mut projected_content = Vec::with_capacity(content.len());
        for part in content {
            match part.get("type").and_then(Value::as_str) {
                Some("encrypted_content") => {
                    let encrypted_content = part
                        .get("encrypted_content")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if looks_like_codex_opaque_encrypted_content(encrypted_content) {
                        return Err(opaque_agent_payload_error());
                    }
                    if !encrypted_content.is_empty() {
                        projected_content.push(json!({
                            "type": "input_text",
                            "text": encrypted_content
                        }));
                    }
                }
                Some("output_text") => {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        projected_content.push(json!({"type": "input_text", "text": text}));
                    }
                }
                Some("input_text" | "input_image" | "input_file" | "input_audio") => {
                    projected_content.push(part.clone());
                }
                _ => {}
            }
        }
        if projected_content.is_empty() {
            return Err(unreadable_agent_payload_error());
        }

        *item = json!({
            "type": "message",
            "role": "user",
            "content": projected_content
        });
        changed += 1;
    }

    Ok(changed)
}

pub(crate) fn looks_like_codex_opaque_encrypted_content(value: &str) -> bool {
    if value.len() < 64 || !value.is_ascii() {
        return false;
    }
    [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ]
    .into_iter()
    .any(|engine| {
        engine
            .decode(value)
            .is_ok_and(|decoded| decoded.len() >= 32)
    })
}

fn opaque_agent_payload_error() -> ProxyError {
    ProxyError::InvalidRequest(
        "third-party child cannot read encrypted Codex agent payload; use a mixed-router non-reserved agents namespace so the official parent emits plaintext"
            .to_string(),
    )
}

fn unreadable_agent_payload_error() -> ProxyError {
    ProxyError::InvalidRequest(
        "third-party child received a Codex agent message without readable content".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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
        let mut fernet_token = vec![0_u8; 96];
        fernet_token[0] = 0x80;
        let opaque = URL_SAFE_NO_PAD.encode(fernet_token);
        assert!(
            opaque.starts_with("gAAAAA"),
            "fixture must match the live Fernet prefix"
        );
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

    #[test]
    fn projected_agent_message_reaches_chat_as_user_text() {
        let task = "Message Type: NEW_TASK\nPayload:\nCHAT_NONCE_19";
        let mut request = json!({
            "model": "deepseek-v4-flash",
            "input": [{
                "type": "agent_message",
                "author": "/root",
                "recipient": "/root/deepseek",
                "content": [{"type": "input_text", "text": task}]
            }]
        });

        project_codex_agent_messages_for_third_party(&mut request).unwrap();
        let chat = super::super::transform_codex_chat::responses_to_chat_completions(request)
            .expect("projected request should convert to Chat");

        assert_eq!(chat["messages"], json!([{"role": "user", "content": task}]));
    }
}
