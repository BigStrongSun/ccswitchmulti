//! Hosted `image_generation` bridge primitives.

use crate::proxy::error::ProxyError;
use crate::proxy::json_canonical::{canonical_json_string, short_sha256_hex};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};

pub(crate) const IMAGE_GENERATION_FUNCTION_NAME: &str = "generate_image";

const MAX_PROMPT_CHARS: usize = 2_000;
const MAX_ERROR_CHARS: usize = 500;
const ALLOWED_FORMATS: &[&str] = &["png", "jpeg", "webp"];
const ALLOWED_QUALITIES: &[&str] = &["low", "medium", "high"];
const ARTIFACT_DIR: &str = "cc-switch-hosted-tools";

/// Codex 入站 hosted `image_generation` 工具的安全子集配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HostedImageGenerationConfig {
    pub(crate) size: Option<String>,
    pub(crate) quality: Option<String>,
    pub(crate) format: Option<String>,
    pub(crate) background: Option<String>,
}

/// 第三方 Chat 模型发起 `generate_image` function call 时的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageGenerationArguments {
    pub(crate) prompt: String,
    pub(crate) size: Option<String>,
    pub(crate) quality: Option<String>,
    pub(crate) format: Option<String>,
}

/// 已落盘并规整的生成图片结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageGenerationResult {
    pub(crate) prompt: String,
    pub(crate) revised_prompt: String,
    pub(crate) mime_type: String,
    pub(crate) artifact_path: String,
    pub(crate) size: Option<String>,
    pub(crate) quality: Option<String>,
    pub(crate) format: String,
    pub(crate) openai_output_id: Option<String>,
}

/// 把单个 Responses hosted tool 定义规整成桥接配置。
pub(crate) fn config_from_tool(tool: &Value) -> HostedImageGenerationConfig {
    HostedImageGenerationConfig {
        size: tool.get("size").and_then(optional_string),
        quality: tool
            .get("quality")
            .and_then(|value| allowed_string(value, ALLOWED_QUALITIES)),
        format: tool
            .get("format")
            .or_else(|| tool.get("output_format"))
            .and_then(|value| allowed_string(value, ALLOWED_FORMATS)),
        background: tool.get("background").and_then(optional_string),
    }
}

/// 生成第三方 Chat 上游可理解的 `generate_image` function tool。
pub(crate) fn chat_tool_definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": IMAGE_GENERATION_FUNCTION_NAME,
            "description": "Generate an image using OpenAI hosted image generation. Returns the local artifact path and image metadata.",
            "parameters": {
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Detailed image generation prompt."
                    },
                    "size": {
                        "type": "string",
                        "enum": ["1024x1024", "1024x1536", "1536x1024"],
                        "description": "Image dimensions."
                    },
                    "quality": {
                        "type": "string",
                        "enum": ["low", "medium", "high"],
                        "description": "Rendering quality."
                    },
                    "format": {
                        "type": "string",
                        "enum": ["png", "jpeg", "webp"],
                        "description": "File output format."
                    }
                },
                "required": ["prompt"],
                "additionalProperties": false
            }
        }
    })
}

/// 解析第三方模型传回的 `generate_image` function arguments。
///
/// 模型参数优先，缺省值回落到 Codex 原始 hosted tool 配置。
pub(crate) fn parse_arguments(
    arguments: &str,
    config: &HostedImageGenerationConfig,
) -> ImageGenerationArguments {
    let parsed = serde_json::from_str::<Value>(arguments).unwrap_or_else(|_| {
        json!({
            "prompt": arguments
        })
    });
    let prompt = parsed
        .get("prompt")
        .or_else(|| parsed.get("description"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| truncate_text(arguments, MAX_PROMPT_CHARS));
    let size = parsed
        .get("size")
        .or_else(|| parsed.get("dimensions"))
        .and_then(optional_string)
        .or_else(|| config.size.clone());
    let quality = parsed
        .get("quality")
        .and_then(|value| allowed_string(value, ALLOWED_QUALITIES))
        .or_else(|| config.quality.clone());
    let format = parsed
        .get("format")
        .or_else(|| parsed.get("output_format"))
        .and_then(|value| allowed_string(value, ALLOWED_FORMATS))
        .or_else(|| config.format.clone());

    ImageGenerationArguments {
        prompt: truncate_text(&prompt, MAX_PROMPT_CHARS),
        size,
        quality,
        format,
    }
}

/// 把 OpenAI Responses 返回规整成第三方模型可消费的稳定结果。
pub(crate) fn result_from_openai_response(
    args: &ImageGenerationArguments,
    response: &Value,
) -> Result<ImageGenerationResult, ProxyError> {
    let items = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ProxyError::TransformError(
                "OpenAI hosted image_generation response is missing output".to_string(),
            )
        })?;
    let call = items
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("image_generation_call"))
        .ok_or_else(|| {
            ProxyError::TransformError(
                "OpenAI hosted image_generation response is missing image_generation_call"
                    .to_string(),
            )
        })?;
    let result = call.get("result").and_then(Value::as_str).ok_or_else(|| {
        ProxyError::TransformError(
            "OpenAI hosted image_generation_call is missing result".to_string(),
        )
    })?;
    let image_bytes = decode_base64_image(result)?;
    let format = call
        .get("output_format")
        .or_else(|| call.get("format"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .or_else(|| args.format.clone())
        .filter(|value| ALLOWED_FORMATS.contains(&value.as_str()))
        .unwrap_or_else(|| "png".to_string());
    let artifact_path = save_image_artifact(&image_bytes, format_to_ext(&format))?;
    let openai_output_id = call
        .get("id")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    Ok(ImageGenerationResult {
        prompt: args.prompt.clone(),
        revised_prompt: call
            .get("revised_prompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        mime_type: format_to_mime(&format).to_string(),
        artifact_path,
        size: call
            .get("size")
            .and_then(optional_string)
            .or_else(|| args.size.clone()),
        quality: call
            .get("quality")
            .and_then(|value| allowed_string(value, ALLOWED_QUALITIES))
            .or_else(|| args.quality.clone()),
        format,
        openai_output_id,
    })
}

/// 构造回填给第三方 Chat 模型的 tool message content。
pub(crate) fn result_to_tool_content(result: &ImageGenerationResult) -> String {
    canonical_json_string(&json!({
        "prompt": result.prompt,
        "revised_prompt": result.revised_prompt,
        "mime_type": result.mime_type,
        "artifact_path": result.artifact_path,
        "size": result.size,
        "quality": result.quality,
        "format": result.format,
        "openai_output_id": result.openai_output_id
    }))
}

/// 构造可让模型继续回答的工具错误输出。
pub(crate) fn error_tool_content(prompt: &str, message: &str) -> String {
    canonical_json_string(&json!({
        "prompt": truncate_text(prompt, MAX_PROMPT_CHARS),
        "artifact_path": "",
        "error": truncate_text(message, MAX_ERROR_CHARS)
    }))
}

/// 对 prompt 生成短 hash，日志只记录 hash 不记录原文。
pub(crate) fn prompt_hash(prompt: &str) -> String {
    short_sha256_hex(prompt.as_bytes())
}

fn optional_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn allowed_string(value: &Value, allowed: &[&str]) -> Option<String> {
    optional_string(value).filter(|value| allowed.contains(&value.as_str()))
}

fn decode_base64_image(value: &str) -> Result<Vec<u8>, ProxyError> {
    let trimmed = value.trim();
    let encoded = if let Some(rest) = trimmed.strip_prefix("data:image/") {
        rest.split_once(';')
            .and_then(|(_, base64_part)| base64_part.strip_prefix("base64,"))
            .unwrap_or(rest)
    } else {
        trimmed
    };
    STANDARD.decode(encoded).map_err(|e| {
        ProxyError::TransformError(format!(
            "Failed to decode OpenAI hosted image_generation result: {e}"
        ))
    })
}

fn save_image_artifact(bytes: &[u8], ext: &str) -> Result<String, ProxyError> {
    let dir = std::env::temp_dir().join(ARTIFACT_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| {
        ProxyError::Internal(format!(
            "Failed to create hosted tool artifact directory {}: {e}",
            dir.display()
        ))
    })?;
    let file_name = format!("ccsm_img_{}.{}", uuid::Uuid::new_v4().simple(), ext);
    let path = dir.join(file_name);
    std::fs::write(&path, bytes).map_err(|e| {
        ProxyError::Internal(format!(
            "Failed to write hosted tool image artifact {}: {e}",
            path.display()
        ))
    })?;
    Ok(path.to_string_lossy().into_owned())
}

fn format_to_mime(format: &str) -> &'static str {
    match format {
        "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

fn format_to_ext(format: &str) -> &'static str {
    match format {
        "jpeg" => "jpg",
        "webp" => "webp",
        _ => "png",
    }
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let mut truncated = normalized.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_from_tool_keeps_safe_fields_only() {
        let config = config_from_tool(&json!({
            "type": "image_generation",
            "size": "1024x1024",
            "quality": "high",
            "format": "png",
            "background": "transparent",
            "unknown": "drop"
        }));

        assert_eq!(config.size.as_deref(), Some("1024x1024"));
        assert_eq!(config.quality.as_deref(), Some("high"));
        assert_eq!(config.format.as_deref(), Some("png"));
        assert_eq!(config.background.as_deref(), Some("transparent"));
    }

    #[test]
    fn parse_arguments_merges_original_tool_config() {
        let config = HostedImageGenerationConfig {
            size: Some("1024x1536".to_string()),
            quality: Some("high".to_string()),
            format: Some("jpeg".to_string()),
            background: None,
        };
        let args = parse_arguments(r#"{"prompt":"a robot","size":"1536x1024"}"#, &config);

        assert_eq!(args.prompt, "a robot");
        assert_eq!(args.size.as_deref(), Some("1536x1024"));
        assert_eq!(args.quality.as_deref(), Some("high"));
        assert_eq!(args.format.as_deref(), Some("jpeg"));
    }

    #[test]
    fn result_from_openai_response_saves_artifact() {
        let response = json!({
            "output": [{
                "id": "ig_test",
                "type": "image_generation_call",
                "status": "completed",
                "result": "aGVsbG8=",
                "revised_prompt": "a robot in the rain",
                "size": "1024x1024",
                "quality": "high",
                "output_format": "png"
            }]
        });
        let args = ImageGenerationArguments {
            prompt: "a robot".to_string(),
            size: Some("1024x1024".to_string()),
            quality: Some("high".to_string()),
            format: Some("png".to_string()),
        };

        let result = result_from_openai_response(&args, &response).unwrap();
        let content: Value = serde_json::from_str(&result_to_tool_content(&result)).unwrap();

        assert_eq!(content["mime_type"], "image/png");
        assert_eq!(content["revised_prompt"], "a robot in the rain");
        assert_eq!(content["openai_output_id"], "ig_test");
        assert!(std::path::Path::new(content["artifact_path"].as_str().unwrap()).exists());
    }

    #[test]
    fn result_from_openai_response_accepts_data_url() {
        let response = json!({
            "output": [{
                "id": "ig_data_url",
                "type": "image_generation_call",
                "status": "completed",
                "result": "data:image/jpeg;base64,aGVsbG8=",
                "output_format": "jpeg"
            }]
        });
        let args = ImageGenerationArguments {
            prompt: "photo".to_string(),
            size: None,
            quality: None,
            format: None,
        };

        let result = result_from_openai_response(&args, &response).unwrap();

        assert_eq!(result.mime_type, "image/jpeg");
        assert!(result.artifact_path.ends_with(".jpg"));
    }
}
