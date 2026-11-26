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
            "id":call.wire_id(), "type":"function",
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
                "role":"tool","tool_call_id":result.wire_id(),"content":result.text(),
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
            if let Some(items) = step
                .provider_content
                .as_ref()
                .and_then(|state| state.get("responses_items"))
                .and_then(Value::as_array)
            {
                input.extend(items.iter().cloned());
            } else {
                if let Some(text) = &step.text {
                    input.push(json!({"type":"message","role":"assistant","content":text}));
                }
                input.extend(step.calls.iter().map(|call| {
                    json!({
                        "type":"function_call", "call_id":call.wire_id(),
                        "name":call.name, "arguments":Value::Object(call.arguments.clone()).to_string(),
                    })
                }));
            }
            input.extend(step.results.iter().map(|result| {
                json!({
                    "type":"function_call_output", "call_id":result.wire_id(), "output":result.text(),
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

pub(crate) fn groq_step(data: Value) -> AiResult<StepResponse> {
    if !data["error"].is_null() {
        return Err(AiError::InvalidResponse);
    }
    let reasoning = data["choices"][0]["message"]["reasoning"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let usage = groq_usage(&data);
    let mut response = chat_step(data)?;
    response.reasoning = reasoning;
    response.usage = usage;
    Ok(response)
}

pub(crate) fn groq_usage(data: &Value) -> Usage {
    let mut usage = Usage::from_openai(&data["x_groq"]["usage"]);
    usage.update(Usage::from_openai(&data["usage"]));
    usage
}

pub(crate) fn mistral_step(mut data: Value) -> AiResult<StepResponse> {
    if data["object"] == "error" || !data["error"].is_null() {
        return Err(AiError::InvalidResponse);
    }
    let choice = data
        .get_mut("choices")
        .and_then(|choices| choices.get_mut(0))
        .and_then(Value::as_object_mut)
        .ok_or(AiError::InvalidResponse)?;
    if choice
        .get("finish_reason")
        .is_some_and(|reason| reason == "error")
    {
        return Err(AiError::InvalidResponse);
    }
    if let Some(reason) = choice
        .get_mut("finish_reason")
        .filter(|reason| *reason == "model_length")
    {
        *reason = "length".into();
    }
    let mut reasoning = String::new();
    if let Some(content) = choice
        .get_mut("message")
        .and_then(|message| message.get_mut("content"))
        .filter(|content| content.is_array())
    {
        let (text, thought) = mistral_content(content);
        *content = text.into();
        reasoning = thought;
    }
    let mut response = chat_step(data)?;
    response.reasoning = reasoning;
    Ok(response)
}

pub(crate) fn mistral_content(content: &Value) -> (String, String) {
    let Some(parts) = content.as_array() else {
        return (content.as_str().unwrap_or_default().into(), String::new());
    };
    let mut text = String::new();
    let mut reasoning = String::new();
    for part in parts {
        if part["type"] == "thinking" {
            if let Some(thinking) = part["thinking"].as_array() {
                for chunk in thinking {
                    if let Some(value) = chunk["text"].as_str().or_else(|| chunk.as_str()) {
                        reasoning.push_str(value);
                    }
                }
            }
        } else if let Some(value) = part["text"].as_str().or_else(|| part.as_str()) {
            text.push_str(value);
        }
    }
    (text, reasoning)
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
    let output = data["output"].as_array().ok_or(AiError::InvalidResponse)?;
    for item in output {
        match item["type"].as_str() {
            Some("function_call") => {
                let mut call = parse_call(&item["call_id"], item)?;
                if let Some(item_id) = item["id"]
                    .as_str()
                    .filter(|id| !id.is_empty() && *id != call.id)
                {
                    call.result_id = Some(std::mem::replace(&mut call.id, item_id.to_owned()));
                }
                tool_calls.push(call);
            }
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
        provider_content: responses_replay(output),
    })
}

fn responses_replay(output: &[Value]) -> Option<Value> {
    let encrypted = |item: &Value| {
        item["type"] == "reasoning"
            && item["encrypted_content"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
    };
    if !output.iter().any(encrypted) {
        return None;
    }

    let mut items = Vec::new();
    for item in output {
        let item = match item["type"].as_str()? {
            "reasoning" if encrypted(item) => {
                let mut reasoning = json!({
                    "type":"reasoning",
                    "summary":item.get("summary").cloned().unwrap_or_else(|| json!([])),
                    "encrypted_content":item["encrypted_content"],
                });
                if let Some(id) = item.get("id") {
                    reasoning["id"] = id.clone();
                }
                reasoning
            }
            "file_search_call" => continue,
            "message"
                if item["content"].as_array().is_some_and(|parts| {
                    parts.iter().all(|part| part["type"] == "output_text")
                }) =>
            {
                item.clone()
            }
            "function_call" => item.clone(),
            _ => return None,
        };
        items.push(item);
    }
    Some(json!({"responses_items":items}))
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
        result_id: None,
    })
}
