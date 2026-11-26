use serde_json::{Value, json};

use crate::{AiError, AiResult, StepResponse, ToolCall, ToolDefinition, ToolStep, Usage};

pub(crate) fn chat_request(
    mut body: Value,
    tools: &[ToolDefinition],
    history: &[ToolStep],
) -> AiResult<Value> {
    let messages = body["messages"]
        .as_array_mut()
        .ok_or(AiError::InvalidResponse)?;
    for step in history {
        let calls = step.calls.iter().map(|call| json!({
            "id":call.id, "type":"function",
            "function":{"name":call.name,"arguments":Value::Object(call.arguments.clone()).to_string()},
        })).collect::<Vec<_>>();
        let mut assistant = json!({"role":"assistant","content":step.text,"tool_calls":calls});
        if let Some(reasoning) = step
            .provider_content
            .as_ref()
            .and_then(|content| content.get("reasoning_content"))
            .and_then(Value::as_str)
        {
            assistant["reasoning_content"] = reasoning.into();
        }
        messages.push(assistant);
        messages.extend(step.results.iter().map(|result| {
            json!({
                "role":"tool","tool_call_id":result.id,"content":result.result,
            })
        }));
    }
    body["tools"] = tools.iter().map(|tool| json!({
        "type":"function", "function":{"name":tool.name,"description":tool.description,"parameters":tool.schema},
    })).collect::<Vec<_>>().into();
    Ok(body)
}

pub(crate) fn responses_request(
    mut body: Value,
    tools: &[ToolDefinition],
    history: &[ToolStep],
) -> AiResult<Value> {
    if !history.is_empty() {
        let mut input = match body["input"].take() {
            Value::String(text) => vec![json!({"type":"message","role":"user","content":text})],
            Value::Array(input) => input,
            _ => return Err(AiError::InvalidResponse),
        };
        for step in history {
            if let Some(text) = &step.text {
                input.push(json!({"type":"message","role":"assistant","content":text}));
            }
            input.extend(step.calls.iter().map(|call| {
                json!({
                    "type":"function_call", "call_id":call.id,
                    "name":call.name, "arguments":Value::Object(call.arguments.clone()).to_string(),
                })
            }));
            input.extend(step.results.iter().map(|result| {
                json!({
                    "type":"function_call_output", "call_id":result.id, "output":result.result,
                })
            }));
        }
        body["input"] = input.into();
    }
    body["tools"] = tools.iter().map(|tool| json!({
        "type":"function","name":tool.name,"description":tool.description,"parameters":tool.schema,"strict":false,
    })).collect::<Vec<_>>().into();
    Ok(body)
}

pub(crate) fn chat_step(data: Value) -> AiResult<StepResponse> {
    let message = data["choices"]
        .get(0)
        .and_then(|choice| choice.get("message"))
        .ok_or(AiError::InvalidResponse)?;
    if message["refusal"]
        .as_str()
        .is_some_and(|value| !value.is_empty())
    {
        return Err(AiError::Refused);
    }
    let mut tool_calls = Vec::new();
    if let Some(calls) = message.get("tool_calls").filter(|calls| !calls.is_null()) {
        for call in calls.as_array().ok_or(AiError::InvalidResponse)? {
            if call["type"] != "function" {
                return Err(AiError::InvalidResponse);
            }
            tool_calls.push(parse_call(&call["id"], &call["function"])?);
        }
    }
    Ok(StepResponse {
        model: data["model"].as_str().map(str::to_owned),
        text: message["content"].as_str().map(str::to_owned),
        finish_reason: if tool_calls.is_empty() {
            crate::FinishReason::from_wire(data["choices"][0]["finish_reason"].as_str())
        } else {
            crate::FinishReason::ToolCalls
        },
        reasoning: String::new(),
        usage: Usage::from_openai(&data["usage"]),
        citations: Vec::new(),
        tool_calls,
        provider_content: message["reasoning_content"]
            .as_str()
            .map(|reasoning| json!({"reasoning_content": reasoning})),
    })
}

pub(crate) fn responses_step(data: Value) -> AiResult<StepResponse> {
    if data["status"]
        .as_str()
        .is_some_and(|status| status != "completed")
    {
        return Err(AiError::InvalidResponse);
    }
    let mut text = None::<String>;
    let mut citations = Vec::new();
    let mut reasoning = Vec::new();
    let mut tool_calls = Vec::new();
    for item in data["output"].as_array().ok_or(AiError::InvalidResponse)? {
        match item["type"].as_str() {
            Some("function_call") => tool_calls.push(parse_call(&item["call_id"], item)?),
            Some("reasoning") => {
                reasoning.extend(crate::text::public_summary_text(&item["summary"]))
            }
            Some("message") => {
                for part in item["content"].as_array().ok_or(AiError::InvalidResponse)? {
                    match part["type"].as_str() {
                        Some("output_text") => {
                            text.get_or_insert_default()
                                .push_str(part["text"].as_str().ok_or(AiError::InvalidResponse)?);
                            citations
                                .extend(crate::UrlCitation::from_annotations(&part["annotations"]));
                        }
                        Some("refusal") => return Err(AiError::Refused),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(StepResponse {
        model: data["model"].as_str().map(str::to_owned),
        text,
        reasoning: reasoning.join("\n\n"),
        finish_reason: if tool_calls.is_empty() {
            crate::FinishReason::from_wire(data["status"].as_str())
        } else {
            crate::FinishReason::ToolCalls
        },
        usage: Usage::from_openai(&data["usage"]),
        citations,
        tool_calls,
        provider_content: None,
    })
}

fn parse_call(id: &Value, function: &Value) -> AiResult<ToolCall> {
    let id = id.as_str().ok_or(AiError::InvalidResponse)?;
    let name = function["name"].as_str().ok_or(AiError::InvalidResponse)?;
    let arguments = function["arguments"]
        .as_str()
        .ok_or(AiError::InvalidToolArguments)?;
    let arguments = serde_json::from_str(arguments).map_err(|_| AiError::InvalidToolArguments)?;
    Ok(ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    })
}
