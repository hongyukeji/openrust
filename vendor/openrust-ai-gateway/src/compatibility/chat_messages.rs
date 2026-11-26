use std::time::{SystemTime, UNIX_EPOCH};

use openrust::support::Str;
use serde_json::{Map, Value, json};

use super::{responses, tools::allowed_fields};

pub(crate) fn to_messages(data: Map<String, Value>) -> Result<Value, &'static str> {
    let messages = data
        .get("messages")
        .and_then(Value::as_array)
        .filter(|messages| !messages.is_empty())
        .ok_or("messages 必须包含对话消息")?;
    let mut normalized = Map::new();
    let mut stop = None;
    let mut has_limit = false;
    for (key, value) in &data {
        match key.as_str() {
            "messages" | "stream_options" => {}
            "model" | "stream" | "temperature" | "top_p" | "parallel_tool_calls" => {
                normalized.insert(key.clone(), value.clone());
            }
            "max_tokens" | "max_completion_tokens" if !value.is_null() => {
                if has_limit {
                    return Err("只能指定一种最大生成 Token 参数");
                }
                has_limit = true;
                normalized.insert("max_output_tokens".into(), value.clone());
            }
            "tools" if !value.is_null() => {
                let tools = value.as_array().ok_or("tools 必须是数组")?;
                let tools = tools
                    .iter()
                    .map(tool_definition)
                    .collect::<Result<Vec<_>, _>>()?;
                normalized.insert("tools".into(), tools.into());
            }
            "tool_choice" if !value.is_null() => {
                let choice = if value.is_string() {
                    value.clone()
                } else if allowed_fields(value, &["type", "function"])
                    && value["type"] == "function"
                    && allowed_fields(&value["function"], &["name"])
                {
                    json!({"type":"function","name":value["function"]["name"]})
                } else {
                    return Err("不支持该 Chat Completions 工具选择");
                };
                normalized.insert("tool_choice".into(), choice);
            }
            "stop" if !value.is_null() => {
                let stops = if value.as_str().is_some_and(|stop| !stop.is_empty()) {
                    vec![value.clone()]
                } else {
                    value
                        .as_array()
                        .filter(|items| {
                            !items.is_empty()
                                && items
                                    .iter()
                                    .all(|item| item.as_str().is_some_and(|stop| !stop.is_empty()))
                        })
                        .ok_or("stop 必须是文本或文本数组")?
                        .clone()
                };
                stop = Some(stops);
            }
            "response_format" if allowed_fields(value, &["type"]) && value["type"] == "text" => {}
            "n" if value.as_u64() == Some(1) => {}
            "presence_penalty" | "frequency_penalty" if value.as_f64() == Some(0.0) => {}
            _ if value.is_null() => {}
            _ => return Err("当前 Chat Completions 到 Messages 转换不支持该请求字段"),
        }
    }
    let mut input = Vec::new();
    for message in messages {
        normalize_message(message, &mut input)?;
    }
    normalized.insert("input".into(), input.into());
    let mut request = responses::to_messages(normalized)?;
    if let Some(stop) = stop {
        request["stop_sequences"] = stop.into();
    }
    Ok(request)
}

pub(super) fn include_usage(data: &Map<String, Value>) -> Result<bool, &'static str> {
    let Some(options) = data.get("stream_options").filter(|value| !value.is_null()) else {
        return Ok(false);
    };
    if !allowed_fields(options, &["include_usage"])
        || data.get("stream") != Some(&Value::Bool(true))
    {
        return Err("stream_options 只接受流式请求的 include_usage");
    }
    match options.get("include_usage") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err("include_usage 必须是布尔值"),
    }
}

fn tool_definition(tool: &Value) -> Result<Value, &'static str> {
    if !allowed_fields(tool, &["type", "function"]) || tool["type"] != "function" {
        return Err("协议转换只支持客户端函数工具");
    }
    let mut function = tool["function"]
        .as_object()
        .ok_or("函数工具缺少 function")
        .cloned()?;
    if function.insert("type".into(), "function".into()).is_some() {
        return Err("function 中不支持 type 字段");
    }
    Ok(function.into())
}

fn normalize_message(message: &Value, input: &mut Vec<Value>) -> Result<(), &'static str> {
    let role = message["role"].as_str().ok_or("消息缺少 role")?;
    match role {
        "system" | "developer" | "user" | "assistant" => {
            let fields: &[&str] = if role == "assistant" {
                &["role", "content", "tool_calls"]
            } else {
                &["role", "content"]
            };
            if !allowed_fields(message, fields) {
                return Err("消息含有当前协议转换暂未支持的字段");
            }
            let calls = message
                .get("tool_calls")
                .filter(|value| !value.is_null())
                .map(|value| value.as_array().ok_or("tool_calls 必须是数组"))
                .transpose()?
                .filter(|calls| !calls.is_empty());
            if !message["content"].is_null() {
                input.push(json!({"type":"message","role":role,"content":plain_text(&message["content"])?}));
            } else if calls.is_none() {
                return Err("消息缺少文本内容");
            }
            if let Some(calls) = calls {
                for call in calls {
                    if !allowed_fields(call, &["type", "id", "function"])
                        || call["type"] != "function"
                        || !allowed_fields(&call["function"], &["name", "arguments"])
                    {
                        return Err("只支持客户端函数调用");
                    }
                    let function = &call["function"];
                    input.push(json!({"type":"function_call","call_id":call["id"],"name":function["name"],"arguments":function["arguments"]}));
                }
            }
            Ok(())
        }
        "tool" if allowed_fields(message, &["role", "tool_call_id", "content"]) => {
            input.push(json!({"type":"function_call_output","call_id":message["tool_call_id"],"output":plain_text(&message["content"])?}));
            Ok(())
        }
        _ => Err("消息 role 或字段不受支持"),
    }
}

fn plain_text(content: &Value) -> Result<String, &'static str> {
    if let Some(content) = content.as_str() {
        return Ok(content.into());
    }
    let blocks = content.as_array().ok_or("消息内容必须为文本")?;
    let mut text = String::new();
    for block in blocks {
        if !allowed_fields(block, &["type", "text"]) || block["type"] != "text" {
            return Err("当前 Messages 转换只支持文本块");
        }
        text.push_str(block["text"].as_str().ok_or("文本块缺少 text")?);
    }
    Ok(text)
}

pub(super) struct Completion {
    id: String,
    model: String,
    created: u64,
    include_usage: bool,
}

impl Completion {
    pub(super) fn new(model: String, include_usage: bool) -> Self {
        Self {
            id: format!("chatcmpl_{}", Str::ulid()),
            model,
            created: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            include_usage,
        }
    }

    pub(super) fn buffered(self, data: &Value, allows_tools: bool) -> Option<Value> {
        let response = responses::Response::new(self.model.clone(), allows_tools.then(Map::new))
            .buffered(data)?;
        let mut text = String::new();
        let mut calls = Vec::new();
        for item in response["output"].as_array()? {
            match item["type"].as_str()? {
                "message" => {
                    for content in item["content"].as_array()? {
                        text.push_str(content["text"].as_str()?);
                    }
                }
                "function_call" => calls.push(call(item)),
                _ => return None,
            }
        }
        let mut message = json!({"role":"assistant","content":text});
        if !calls.is_empty() {
            if text.is_empty() {
                message["content"] = Value::Null;
            }
            message["tool_calls"] = json!(calls);
        }
        let finish = stop_reason(&response, !calls.is_empty())?;
        let mut output = self.object(
            "chat.completion",
            json!([{"index":0,"message":message,"finish_reason":finish}]),
        );
        if let Some(usage) = usage(&response["usage"]) {
            output["usage"] = usage;
        }
        Some(output)
    }

    fn object(&self, kind: &str, choices: Value) -> Value {
        json!({"id":self.id,"object":kind,"created":self.created,"model":self.model,"choices":choices})
    }

    fn chunk(&self, delta: Value, finish: Option<&str>) -> String {
        let mut chunk = self.object(
            "chat.completion.chunk",
            json!([{"index":0,"delta":delta,"finish_reason":finish}]),
        );
        if self.include_usage {
            chunk["usage"] = Value::Null;
        }
        format!("data: {chunk}\n\n")
    }

    fn final_usage(&self, response_usage: &Value) -> Option<String> {
        if !self.include_usage {
            return None;
        }
        let usage = usage(response_usage)?;
        let mut chunk = self.object("chat.completion.chunk", json!([]));
        chunk["usage"] = usage;
        Some(format!("data: {chunk}\n\n"))
    }
}

fn call(item: &Value) -> Value {
    json!({"id":item["call_id"],"type":"function","function":{"name":item["name"],"arguments":item["arguments"]}})
}

fn stop_reason(response: &Value, tools: bool) -> Option<&'static str> {
    match response["status"].as_str()? {
        "completed" if tools => Some("tool_calls"),
        "completed" => Some("stop"),
        "incomplete" => match response["incomplete_details"]["reason"].as_str()? {
            "max_output_tokens" => Some("length"),
            "content_filter" => Some("content_filter"),
            _ => None,
        },
        _ => None,
    }
}

fn usage(data: &Value) -> Option<Value> {
    let mut usage = json!({"prompt_tokens":data["input_tokens"].as_u64()?,
        "completion_tokens":data["output_tokens"].as_u64()?,"total_tokens":data["total_tokens"].as_u64()?});
    if let Some(cached) = data["input_tokens_details"]["cached_tokens"].as_u64() {
        usage["prompt_tokens_details"] = json!({"cached_tokens":cached});
    }
    if let Some(reasoning) = data["output_tokens_details"]["reasoning_tokens"].as_u64() {
        usage["completion_tokens_details"] = json!({"reasoning_tokens":reasoning});
    }
    Some(usage)
}

pub(crate) struct StreamState {
    response: responses::Response,
    provider_stream: responses::StreamState,
    completion: Completion,
    tools: std::collections::BTreeMap<u64, usize>,
}

impl StreamState {
    pub(crate) fn new(model: String, include_usage: bool, allows_tools: bool) -> Self {
        Self {
            response: responses::Response::new(model.clone(), allows_tools.then(Map::new)),
            provider_stream: responses::StreamState::default(),
            completion: Completion::new(model, include_usage),
            tools: std::collections::BTreeMap::new(),
        }
    }

    pub(crate) fn convert(&mut self, data: &Value) -> Option<(Vec<String>, bool)> {
        let (events, terminal) = self.provider_stream.convert(&mut self.response, data)?;
        let mut chunks = Vec::new();
        for event in events {
            let payload = &event.payload;
            let delta = match event.kind {
                "response.created" => json!({"role":"assistant", "content":""}),
                "response.content_part.added"
                    if payload["part"]["text"]
                        .as_str()
                        .is_some_and(|text| !text.is_empty()) =>
                {
                    json!({"content":payload["part"]["text"]})
                }
                "response.output_text.delta" => json!({"content":payload["delta"]}),
                "response.output_item.added" if payload["item"]["type"] == "function_call" => {
                    let output_index = payload["output_index"].as_u64()?;
                    let index = self.tools.len();
                    if self.tools.insert(output_index, index).is_some() {
                        return None;
                    }
                    let mut call = call(&payload["item"]);
                    call["index"] = index.into();
                    json!({"tool_calls":[call]})
                }
                "response.function_call_arguments.delta" => {
                    let index = self.tools.get(&payload["output_index"].as_u64()?)?;
                    json!({"tool_calls":[{"index":index,"function":{"arguments":payload["delta"]}}]})
                }
                "response.completed" | "response.incomplete" => {
                    let response = &payload["response"];
                    let reason = stop_reason(response, !self.tools.is_empty())?;
                    chunks.push(self.completion.chunk(json!({}), Some(reason)));
                    chunks.extend(self.completion.final_usage(&response["usage"]));
                    chunks.push("data: [DONE]\n\n".into());
                    continue;
                }
                _ => continue,
            };
            chunks.push(self.completion.chunk(delta, None));
        }
        Some((chunks, terminal))
    }
}
