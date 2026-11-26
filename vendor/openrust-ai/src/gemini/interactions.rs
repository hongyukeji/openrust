use std::time::Duration;

use serde_json::Value;

use crate::{AiError, AiResult, Usage, connection::Connection, text::validate};

pub(super) async fn interact(
    connection: &Connection,
    timeout: Duration,
    body: Value,
) -> AiResult<Value> {
    let result: Value = validate(
        connection
            .request(Some(timeout))?
            .post("interactions", &body)
            .await,
    )?
    .json()
    .map_err(|_| AiError::InvalidResponse)?;
    if !result["error"].is_null()
        || result["status"]
            .as_str()
            .is_some_and(|status| status != "completed")
    {
        return Err(AiError::InvalidResponse);
    }
    Ok(result)
}

pub(super) fn output_blocks(data: &Value) -> impl Iterator<Item = &Value> {
    data["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|step| step["type"] == "model_output")
        .filter_map(|step| step["content"].as_array())
        .flatten()
}

pub(super) fn reported_model<'a>(data: &'a Value, requested: &'a str) -> &'a str {
    data["model"].as_str().unwrap_or(requested)
}

pub(super) fn usage(data: &Value) -> Usage {
    let usage = &data["usage"];
    Usage {
        prompt_tokens: usage["total_input_tokens"].as_u64(),
        completion_tokens: usage["total_output_tokens"].as_u64(),
        cache_read_input_tokens: usage["total_cached_tokens"].as_u64(),
        reasoning_tokens: usage["total_thought_tokens"].as_u64(),
        ..Usage::default()
    }
    .include_reasoning_output()
}
