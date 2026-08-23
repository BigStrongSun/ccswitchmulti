use serde_json::{json, Value};

const BASELINE_PROMPT: &str =
    "CCSM protocol compatibility probe. Solve 17 + 25 internally. Reply only CCSM_PROTOCOL_BASELINE_OK.";
const TOOL_NAME: &str = "ccsm_protocol_compatibility_probe";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeCase {
    BaselineJson,
    BaselineSse,
    ForcedToolSse,
    ToolContinuationJson,
}

pub fn build_logical_probe_request(case: ProbeCase, model: &str, nonce: &str) -> Value {
    let stream = matches!(case, ProbeCase::BaselineSse | ProbeCase::ForcedToolSse);
    let mut request = json!({
        "model": model,
        "stream": stream,
        "store": false,
        "max_output_tokens": 128,
    });

    match case {
        ProbeCase::BaselineJson | ProbeCase::BaselineSse => {
            request["input"] = probe_user_input(BASELINE_PROMPT);
        }
        ProbeCase::ForcedToolSse => {
            request["input"] = probe_user_input(&format!(
                "CCSM protocol compatibility probe. Call the provided function exactly once with nonce {nonce}. After its result, reply only CCSM_PROTOCOL_TOOL_DONE."
            ));
            request["tools"] = json!([{
                "type": "function",
                "name": TOOL_NAME,
                "description": "Internal CCSM protocol compatibility probe. Call exactly once with the supplied nonce.",
                "parameters": {
                    "type": "object",
                    "properties": { "nonce": { "type": "string" } },
                    "required": ["nonce"]
                }
            }]);
            request["tool_choice"] = json!({ "type": "function", "name": TOOL_NAME });
        }
        ProbeCase::ToolContinuationJson => {}
    }

    request
}

fn probe_user_input(text: &str) -> Value {
    json!([{
        "role": "user",
        "content": [{ "type": "input_text", "text": text }]
    }])
}

#[cfg(test)]
mod cases;
