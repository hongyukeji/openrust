use serde_json::{Map, Value};

use crate::{AiError, AiResult};

/// 特定供应商协议的附加 JSON 请求字段。
pub type ProviderOptions = Map<String, Value>;

const RESERVED: &[&str] = &[
    "model",
    "input",
    "messages",
    "contents",
    "system",
    "instructions",
    "systemInstruction",
    "system_instruction",
    "stream",
    "store",
    "tools",
    "tool_choice",
    "toolChoice",
    "toolConfig",
    "tool_config",
    "text",
    "response_format",
    "output_config",
    "max_output_tokens",
    "max_tokens",
    "max_completion_tokens",
    "temperature",
    "top_p",
];

const RESERVED_GEMINI_GENERATION: &[&str] = &[
    "maxOutputTokens",
    "max_output_tokens",
    "temperature",
    "topP",
    "top_p",
    "responseMimeType",
    "response_mime_type",
    "responseJsonSchema",
    "response_json_schema",
    "responseSchema",
    "response_schema",
];

pub(crate) fn apply_provider_options(
    body: &mut Value,
    options: Option<&ProviderOptions>,
    gemini: bool,
) -> AiResult<()> {
    let Some(options) = options.filter(|options| !options.is_empty()) else {
        return Ok(());
    };
    let body = body
        .as_object_mut()
        .ok_or(AiError::InvalidProviderOptions)?;
    for (key, value) in options {
        if key == "generationConfig" && gemini {
            let additional = value.as_object().ok_or(AiError::InvalidProviderOptions)?;
            let settings = body
                .entry(key.clone())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or(AiError::InvalidProviderOptions)?;
            for (field, value) in additional {
                if RESERVED_GEMINI_GENERATION.contains(&field.as_str())
                    || settings.contains_key(field)
                {
                    return Err(AiError::InvalidProviderOptions);
                }
                settings.insert(field.clone(), value.clone());
            }
        } else {
            if key.trim().is_empty()
                || RESERVED.contains(&key.as_str())
                || matches!(key.as_str(), "generation_config" | "generationConfig")
                || body.contains_key(key)
            {
                return Err(AiError::InvalidProviderOptions);
            }
            body.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}
