use serde_json::{Map, Value};

use super::resources;

/// 展开生成配置，以便计数与生成共用缓存和资源授权。
pub(super) fn unwrap(data: &mut Map<String, Value>, alias: &str) -> Result<bool, &'static str> {
    let camel = data
        .remove("generateContentRequest")
        .filter(|value| !value.is_null());
    let snake = data
        .remove("generate_content_request")
        .filter(|value| !value.is_null());
    if camel.is_some() && snake.is_some() {
        return Err("Gemini 计数只能提供一种生成配置字段");
    }
    let Some(config) = camel.or(snake) else {
        check_model(data, alias)?;
        return Ok(false);
    };
    if data.values().any(|value| !value.is_null()) {
        return Err("Gemini 计数的生成配置与外层正文不能同时提供");
    }
    let Value::Object(mut config) = config else {
        return Err("Gemini 计数的生成配置须为对象");
    };
    check_model(&mut config, alias)?;
    if config.keys().any(|field| {
        !matches!(
            field.as_str(),
            "contents"
                | "systemInstruction"
                | "system_instruction"
                | "tools"
                | "toolConfig"
                | "tool_config"
                | "safetySettings"
                | "safety_settings"
                | "generationConfig"
                | "generation_config"
                | "cachedContent"
                | "cached_content"
        )
    }) || !resources::gemini_function_tools(config.get("tools"))
    {
        return Err("Gemini 计数仅接收内联内容、客户端函数及已授权的缓存");
    }
    *data = config;
    Ok(true)
}

fn check_model(data: &mut Map<String, Value>, alias: &str) -> Result<(), &'static str> {
    if let Some(model) = data.remove("model").filter(|value| !value.is_null())
        && model
            .as_str()
            .is_none_or(|name| name.strip_prefix("models/").unwrap_or(name) != alias)
    {
        return Err("Gemini 计数模型须与路径中的公开模型一致");
    }
    Ok(())
}

pub(super) fn valid_response(data: &Value) -> bool {
    data["totalTokens"].as_u64().is_some_and(|total| {
        i32::try_from(total).is_ok()
            && data.get("cachedContentTokenCount").is_none_or(|cached| {
                cached.is_null() || cached.as_u64().is_some_and(|cached| cached <= total)
            })
    })
}
