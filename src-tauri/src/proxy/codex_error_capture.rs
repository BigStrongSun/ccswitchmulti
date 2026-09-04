use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::LazyLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureControl {
    capture_id: uuid::Uuid,
    session_id: uuid::Uuid,
    expires_at: i64,
    max_events: u32,
}

pub(crate) fn redact(value: &str, limit: usize) -> String {
    static PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
        [
            r"(?s)-----BEGIN [^-]*PRIVATE KEY-----.*?(?:-----END [^-]*PRIVATE KEY-----|$)",
            r"(?i)\b(?:bearer|basic)\s+[^\s;,]+",
            r#"(?i)\b(?:set-cookie|cookie)["']?\s*[=:][^\r\n]*"#,
            r#"(?i)\b(?:api[_-]?key|access[_-]?token|refresh[_-]?token|password|cookie|authorization)["']?\s*[=:]\s*[^\r\n;,]+"#,
            r"\bsk-[A-Za-z0-9_-]+",
            r"\beyJ[A-Za-z0-9_-]*\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+",
            r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}",
            r#"https?://[^\s<>"']+"#,
        ].into_iter().map(|pattern| regex::Regex::new(pattern).expect("static redaction pattern")).collect()
    });
    let mut output: String = value.chars().take(65536).collect();
    for pattern in PATTERNS.iter() {
        output = pattern.replace_all(&output, "[redacted]").into_owned();
    }
    let truncated = output.chars().count() > limit;
    output = output.chars().take(limit).collect();
    if truncated {
        output.push_str(" [truncated]");
    }
    output
}

pub(crate) fn capture_at(root: &Path, session: &str, event: &str, error: &Value, now: i64) {
    let _ = try_capture(root, session, event, error, now);
}

fn try_capture(
    root: &Path,
    session: &str,
    event: &str,
    error: &Value,
    now: i64,
) -> std::io::Result<()> {
    if !matches!(event, "error" | "response.error" | "response.failed") || !error.is_object() {
        return Ok(());
    }
    let mut control_bytes = Vec::new();
    std::fs::File::open(root.join("codex-error-capture.json"))?
        .take(4097)
        .read_to_end(&mut control_bytes)?;
    if control_bytes.len() > 4096 {
        return Ok(());
    }
    let Ok(control) = serde_json::from_slice::<CaptureControl>(&control_bytes) else {
        return Ok(());
    };
    if uuid::Uuid::parse_str(session).ok() != Some(control.session_id)
        || control.expires_at <= now
        || control.expires_at.saturating_sub(now) > 1800
        || !(1..=20).contains(&control.max_events)
    {
        return Ok(());
    }
    let mut fields = serde_json::Map::new();
    for name in ["type", "code", "message", "param"] {
        if let Some(value) = error.get(name) {
            if let Some(text) = value.as_str() {
                fields.insert(
                    name.to_string(),
                    Value::String(redact(text, if name == "message" { 4000 } else { 128 })),
                );
            } else if value.is_null() {
                fields.insert(name.to_string(), Value::Null);
            }
        }
    }
    let record = serde_json::json!({
        "timestamp":now, "session_id":session, "capture_id":control.capture_id,
        "event":event, "error":fields,
        "message_sha256":format!("{:x}", Sha256::digest(error.get("message").and_then(Value::as_str).unwrap_or("").as_bytes())),
        "notice":"Selected upstream error fields only; credential redaction is best effort. Review before sharing."
    });
    let directory = root
        .join("logs")
        .join("codex-error-captures")
        .join(control.capture_id.to_string());
    std::fs::create_dir_all(&directory)?;
    let bytes = serde_json::to_vec(&record)?;
    for index in 0..control.max_events {
        let path = directory.join(format!("{index:02}.json"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(mut file) => {
                file.write_all(&bytes)?;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SESSION: &str = "01a05b75-d141-7652-ac6c-a6258653316d";
    const CAPTURE: &str = "01a05b75-d141-7652-ac6c-a6258653316e";

    fn arm(root: &Path, expires_at: i64, max_events: u64) {
        std::fs::write(
            root.join("codex-error-capture.json"),
            json!({
                "capture_id":CAPTURE, "session_id":SESSION,
                "expires_at":expires_at, "max_events":max_events
            })
            .to_string(),
        )
        .unwrap();
    }

    fn captured(root: &Path) -> Vec<Value> {
        let path = root.join("logs").join("codex-error-captures").join(CAPTURE);
        let Ok(entries) = std::fs::read_dir(path) else {
            return vec![];
        };
        entries
            .map(|entry| {
                serde_json::from_slice(&std::fs::read(entry.unwrap().path()).unwrap()).unwrap()
            })
            .collect()
    }

    #[test]
    fn capture_is_hot_enabled_scoped_expiring_and_bounded() {
        let root = tempfile::tempdir().unwrap();
        let error = json!({"type":"invalid_request_error","code":"invalid_prompt","message":"request rejected","param":"input","input":"never write this","headers":{"authorization":"secret"}});
        capture_at(root.path(), SESSION, "error", &error, 100);
        assert!(captured(root.path()).is_empty());
        arm(root.path(), 150, 2);
        capture_at(root.path(), "other", "error", &error, 100);
        capture_at(root.path(), SESSION, "response.completed", &error, 100);
        capture_at(root.path(), SESSION, "error", &error, 150);
        assert!(captured(root.path()).is_empty());
        for _ in 0..4 {
            capture_at(root.path(), SESSION, "error", &error, 100);
        }
        let records = captured(root.path());
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["error"]["message"], "request rejected");
        assert_eq!(records[0]["error"]["code"], "invalid_prompt");
        assert_eq!(records[0]["session_id"], SESSION);
        assert!(records[0]["error"].get("input").is_none());
        assert!(!records[0].to_string().contains("secret"));
    }

    #[test]
    fn capture_redacts_credentials_and_limits_message_size() {
        let root = tempfile::tempdir().unwrap();
        arm(root.path(), 150, 2);
        capture_at(
            root.path(),
            SESSION,
            "error",
            &json!({"message":"Authorization: Bearer abcdef123; api_key=hidden123 sk-testsecret eyJabc.def.ghi user@example.com\n-----BEGIN PRIVATE KEY-----\nprivatebits\n-----END PRIVATE KEY-----"}),
            100,
        );
        capture_at(
            root.path(),
            SESSION,
            "error",
            &json!({"message":"长".repeat(20000)}),
            100,
        );
        let records = captured(root.path());
        assert_eq!(records.len(), 2);
        let output = serde_json::to_string(&records).unwrap();
        for secret in [
            "abcdef123",
            "hidden123",
            "sk-testsecret",
            "eyJabc.def.ghi",
            "user@example.com",
            "privatebits",
        ] {
            assert!(!output.contains(secret), "leaked {secret}");
        }
        assert!(output.len() < 18000);
    }

    #[test]
    fn capture_rejects_invalid_control_and_io_failure_is_nonfatal() {
        let root = tempfile::tempdir().unwrap();
        for control in [
            "not-json".to_string(),
            json!({"capture_id":"../escape","session_id":SESSION,"expires_at":150,"max_events":1})
                .to_string(),
        ] {
            std::fs::write(root.path().join("codex-error-capture.json"), control).unwrap();
            capture_at(root.path(), SESSION, "error", &json!({"message":"x"}), 100);
            assert!(captured(root.path()).is_empty());
        }
        arm(root.path(), 5000, 1);
        capture_at(root.path(), SESSION, "error", &json!({"message":"x"}), 100);
        assert!(captured(root.path()).is_empty());
        arm(root.path(), 150, 0);
        capture_at(root.path(), SESSION, "error", &json!({"message":"x"}), 100);
        assert!(captured(root.path()).is_empty());
        arm(root.path(), 150, 1);
        std::fs::write(root.path().join("logs"), "blocked").unwrap();
        capture_at(root.path(), SESSION, "error", &json!({"message":"x"}), 100);
    }

    #[test]
    fn capture_redacts_all_cookie_values() {
        let root = tempfile::tempdir().unwrap();
        arm(root.path(), 150, 1);
        capture_at(
            root.path(),
            SESSION,
            "error",
            &serde_json::json!({"message":"Cookie: session=firstsecret; auth=secondsecret\nrequest rejected"}),
            100,
        );
        let records = captured(root.path());
        assert_eq!(records.len(), 1);
        let output = records[0].to_string();
        assert!(!output.contains("firstsecret"));
        assert!(!output.contains("secondsecret"));
        assert!(output.contains("request rejected"));
    }
}
