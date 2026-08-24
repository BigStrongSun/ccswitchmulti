use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedFieldEvidence {
    pub path: String,
    pub value_kind: String,
    pub byte_length: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedProbeEvidence {
    pub status_code: u16,
    pub paths: Vec<String>,
    pub fields: Vec<RedactedFieldEvidence>,
    pub event_types: Vec<String>,
}

pub fn redact_json_probe_response(status_code: u16, response: &Value) -> RedactedProbeEvidence {
    let mut fields = Vec::new();
    collect_allowlisted_fields(response, "", &mut fields);
    build_evidence(status_code, fields, Vec::new())
}

pub fn redact_sse_probe_response(status_code: u16, sse_body: &str) -> RedactedProbeEvidence {
    let mut fields = Vec::new();
    let mut event_types = Vec::new();

    for frame in sse_body.split("\n\n") {
        let mut event_name = None;
        let mut data_lines = Vec::new();
        for line in frame.lines() {
            if let Some(value) = line.strip_prefix("event:") {
                event_name = Some(value.trim().to_owned());
            } else if let Some(value) = line.strip_prefix("data:") {
                data_lines.push(value.trim_start());
            }
        }

        if data_lines.is_empty() {
            continue;
        }

        let data = data_lines.join("\n");
        if data == "[DONE]" {
            event_types.push("done".to_owned());
            continue;
        }

        event_types.push(event_name.unwrap_or_else(|| "data".to_owned()));
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            collect_allowlisted_fields(&value, "", &mut fields);
        }
    }

    build_evidence(status_code, fields, event_types)
}

fn build_evidence(
    status_code: u16,
    fields: Vec<RedactedFieldEvidence>,
    event_types: Vec<String>,
) -> RedactedProbeEvidence {
    let paths = fields
        .iter()
        .map(|field| field.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    RedactedProbeEvidence {
        status_code,
        paths,
        fields,
        event_types,
    }
}

fn collect_allowlisted_fields(value: &Value, path: &str, fields: &mut Vec<RedactedFieldEvidence>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let child_path = join_path(path, key);
                collect_allowlisted_fields(child, &child_path, fields);
            }
        }
        Value::Array(items) => {
            let item_path = format!("{path}[]");
            for item in items {
                collect_allowlisted_fields(item, &item_path, fields);
            }
        }
        _ if is_allowlisted_path(path) => fields.push(redact_field(path, value)),
        _ => {}
    }
}

fn join_path(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        key.to_owned()
    } else {
        format!("{parent}.{key}")
    }
}

fn is_allowlisted_path(path: &str) -> bool {
    matches!(
        path,
        "choices[].message.reasoning_content"
            | "choices[].message.reasoning"
            | "choices[].message.reasoning_details"
            | "choices[].message.content"
            | "choices[].message.tool_calls[].function.arguments"
            | "choices[].delta.reasoning_content"
            | "choices[].delta.reasoning"
            | "choices[].delta.reasoning_details"
            | "choices[].delta.content"
            | "choices[].delta.tool_calls[].function.arguments"
            | "output[].content[].text"
            | "output[].content[].summary"
            | "output[].content[].encrypted_content"
    )
}

fn redact_field(path: &str, value: &Value) -> RedactedFieldEvidence {
    let bytes = match value {
        Value::String(text) => text.as_bytes().to_vec(),
        _ => serde_json::to_vec(value).expect("serde_json values serialize"),
    };
    let value_kind = match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    RedactedFieldEvidence {
        path: path.to_owned(),
        value_kind: value_kind.to_string(),
        byte_length: bytes.len(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}
