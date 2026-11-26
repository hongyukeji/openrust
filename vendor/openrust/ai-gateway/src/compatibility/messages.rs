use std::collections::{BTreeMap, HashSet};

use openrust::support::Str;
use serde_json::{Map, Value, json};

use super::tools::{
    ToolHistory, allowed_fields, arguments, message_choice, tool_definition, tool_id, tool_name,
};

/// 将 Anthropic 的无状态请求转换成 OpenAI Responses。
pub(crate) fn to_responses(request: Map<String, Value>) -> Result<Value, &'static str> {
    let max_tokens = request
        .get("max_tokens")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or("max_tokens 必须是正整数")?;
    let mut target = Map::from_iter([
        ("model".into(), request["model"].clone()),
        ("max_output_tokens".into(), json!(max_tokens)),
        ("store".into(), Value::Bool(false)),
    ]);
    for (key, value) in &request {
        match key.as_str() {
            "model" | "messages" | "system" | "max_tokens" | "tools" | "tool_choice" => {}
            "stream" | "temperature" | "top_p" if !value.is_null() => {
                target.insert(key.clone(), value.clone());
            }
            _ if value.is_null() => {}
            _ => return Err("当前 Messages 到 Responses 转换不支持该请求字段"),
        }
    }
    if let Some(tools) = request.get("tools").filter(|tools| !tools.is_null()) {
        let tools = tools.as_array().ok_or("tools 必须是数组")?;
        if !tools.is_empty() {
            let definitions = tools
                .iter()
                .map(|tool| tool_definition(tool, false))
                .collect::<Result<Vec<_>, _>>()?;
            target.insert("tools".into(), definitions.into());
            if let Some(choice) = request
                .get("tool_choice")
                .filter(|choice| !choice.is_null())
            {
                let (choice, parallel) = message_choice(choice)?;
                target.insert("tool_choice".into(), choice);
                if let Some(parallel) = parallel {
                    target.insert("parallel_tool_calls".into(), parallel.into());
                }
            }
        }
    }
    if let Some(system) = request.get("system").filter(|value| !value.is_null()) {
        target.insert("instructions".into(), json!(parts(system)?.join("\n\n")));
    }
    let messages = request
        .get("messages")
        .and_then(Value::as_array)
        .filter(|messages| !messages.is_empty())
        .ok_or("至少提供一条消息")?;
    let mut input = Vec::new();
    let mut tools = ToolHistory::default();
    for message in messages {
        if !allowed_fields(message, &["role", "content"]) {
            return Err("消息字段不受协议转换支持");
        }
        let role = message["role"]
            .as_str()
            .filter(|role| matches!(*role, "user" | "assistant"))
            .ok_or("消息 role 不受支持")?;
        let content = &message["content"];
        if let Some(text) = content.as_str() {
            input.push(json!({"type":"message","role":role,"content":text}));
            continue;
        }
        let blocks = content.as_array().ok_or("消息内容必须是文本或文本块")?;
        let mut texts = Vec::new();
        for block in blocks {
            if block["type"] == "text" {
                if !allowed_fields(block, &["type", "text"]) {
                    return Err("文本块包含暂未支持的字段");
                }
                texts.push(block["text"].as_str().ok_or("文本块缺少 text")?);
                continue;
            }
            push_text(&mut input, role, &mut texts);
            match block["type"].as_str() {
                Some("tool_use")
                    if role == "assistant"
                        && allowed_fields(block, &["type", "id", "name", "input"]) =>
                {
                    let id = tool_id(&block["id"]).ok_or("缺少有效的工具调用 ID")?;
                    let name = tool_name(&block["name"]).ok_or("工具名称无效")?;
                    if !block["input"].is_object() {
                        return Err("工具参数必须是 JSON 对象");
                    }
                    tools.call(id)?;
                    input.push(json!({"type":"function_call","call_id":id,"name":name,"arguments":block["input"].to_string()}));
                }
                Some("tool_result")
                    if role == "user"
                        && allowed_fields(
                            block,
                            &["type", "tool_use_id", "content", "is_error"],
                        ) =>
                {
                    let id = tool_id(&block["tool_use_id"]).ok_or("缺少有效的工具调用 ID")?;
                    let mut output = if block["content"].is_null() {
                        String::new()
                    } else {
                        parts(&block["content"])?.join("\n")
                    };
                    if !block["is_error"].is_null() {
                        if !block["is_error"].is_boolean() {
                            return Err("工具结果 is_error 必须是布尔值");
                        }
                        if block["is_error"] == true {
                            output.insert_str(0, "Tool error: ");
                        }
                    }
                    tools.result(id)?;
                    input.push(json!({"type":"function_call_output","call_id":id,"output":output}));
                }
                _ => return Err("当前转换只接收文本及客户端函数工具块"),
            }
        }
        push_text(&mut input, role, &mut texts);
    }
    tools.finish()?;
    if input.is_empty() {
        return Err("至少提供一条消息");
    }
    target.insert("input".into(), input.into());
    Ok(Value::Object(target))
}

fn push_text(input: &mut Vec<Value>, role: &str, texts: &mut Vec<&str>) {
    if !texts.is_empty() {
        input.push(json!({"type":"message","role":role,"content":texts.join("\n")}));
        texts.clear();
    }
}

fn parts(content: &Value) -> Result<Vec<&str>, &'static str> {
    if let Some(text) = content.as_str() {
        return Ok(vec![text]);
    }
    content
        .as_array()
        .ok_or("消息内容必须是文本或文本块")?
        .iter()
        .map(|part| {
            if part["type"] != "text"
                || part.as_object().is_none_or(|part| {
                    part.keys()
                        .any(|key| !matches!(key.as_str(), "type" | "text"))
                })
            {
                return Err("当前转换只接收纯文本块");
            }
            part["text"].as_str().ok_or("文本块缺少 text")
        })
        .collect()
}

pub(crate) struct Message {
    id: String,
    model: String,
    allows_tools: bool,
}

impl Message {
    pub(crate) fn new(model: String, allows_tools: bool) -> Self {
        Self {
            id: format!("msg_{}", Str::ulid()),
            model,
            allows_tools,
        }
    }

    pub(crate) fn buffered(&self, response: &Value) -> Option<Value> {
        let mut reason = finish(response)?;
        let mut blocks = Vec::new();
        let mut calls = std::collections::HashSet::new();
        for item in response["output"].as_array()? {
            match item["type"].as_str()? {
                "reasoning" => {}
                "function_call"
                    if self.allows_tools && matches!(reason, "end_turn" | "tool_use") =>
                {
                    if item["status"]
                        .as_str()
                        .is_some_and(|status| status != "completed")
                    {
                        return None;
                    }
                    let id = tool_id(&item["call_id"])?;
                    let name = tool_name(&item["name"])?;
                    if !calls.insert(id) {
                        return None;
                    }
                    let input = arguments(&item["arguments"])?;
                    blocks.push(json!({"type":"tool_use","id":id,"name":name,"input":input}));
                    reason = "tool_use";
                }
                "message" => {
                    for content in item["content"].as_array()? {
                        match content["type"].as_str()? {
                            "output_text" => blocks.push(text_block(content["text"].as_str()?)),
                            "refusal" if reason == "refusal" => {}
                            _ => return None,
                        }
                    }
                }
                _ => return None,
            }
        }
        if blocks.is_empty() && reason == "end_turn" {
            return None;
        }
        Some(self.message(blocks, Some(reason), usage(&response["usage"])))
    }

    fn message(&self, content: Vec<Value>, reason: Option<&str>, usage: Value) -> Value {
        json!({
            "id":self.id, "model":self.model, "type":"message", "role":"assistant",
            "content":content, "stop_reason":reason, "stop_sequence":null, "usage":usage,
        })
    }
}

fn text_block(text: &str) -> Value {
    json!({"type":"text","text":text})
}

fn finish(response: &Value) -> Option<&'static str> {
    let refused = response["output"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["content"]
                .as_array()
                .is_some_and(|content| content.iter().any(|part| part["type"] == "refusal"))
        })
    });
    if refused {
        return Some("refusal");
    }
    match response["status"].as_str()? {
        "completed" => Some("end_turn"),
        "incomplete" if response["incomplete_details"]["reason"] == "max_output_tokens" => {
            Some("max_tokens")
        }
        "incomplete" if response["incomplete_details"]["reason"] == "content_filter" => {
            Some("refusal")
        }
        _ => None,
    }
}

/// Anthropic 对未缓存、读取缓存和写入缓存的输入分别计数。
fn usage(data: &Value) -> Value {
    let cached = data["input_tokens_details"]["cached_tokens"].as_u64();
    let created = data["cache_creation_input_tokens"].as_u64();
    let input = data["input_tokens"].as_u64().map(|input| {
        input
            .saturating_sub(cached.unwrap_or_default())
            .saturating_sub(created.unwrap_or_default())
    });
    let output = data["output_tokens"].as_u64();
    json!({
        "input_tokens":input, "output_tokens":output,
        "cache_read_input_tokens":cached, "cache_creation_input_tokens":created,
    })
}

#[derive(Default)]
pub(crate) struct StreamState {
    started: bool,
    blocks: BTreeMap<(u64, u64), u64>,
    tools: BTreeMap<u64, ToolBlock>,
    call_ids: HashSet<String>,
    next_index: u64,
}

struct ToolBlock {
    index: u64,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
    done: bool,
}

impl ToolBlock {
    fn finish_arguments(&mut self, arguments: &str) -> Option<Vec<String>> {
        let completed = serde_json::from_str::<Map<String, Value>>(arguments).ok()?;
        let frames = if self.arguments.is_empty() {
            self.arguments = arguments.into();
            vec![tool_delta(self.index, arguments)]
        } else {
            if serde_json::from_str::<Map<String, Value>>(&self.arguments).ok()? != completed {
                return None;
            }
            Vec::new()
        };
        self.done = true;
        Some(frames)
    }
}

fn tool_delta(index: u64, arguments: &str) -> String {
    frame(
        "content_block_delta",
        json!({"index":index,"delta":{"type":"input_json_delta","partial_json":arguments}}),
    )
}

impl StreamState {
    pub(crate) fn convert(
        &mut self,
        message: &Message,
        event: &Value,
    ) -> Option<(Vec<String>, bool)> {
        let frames = match event["type"].as_str()? {
            "response.created" if !self.started => {
                self.started = true;
                // 起始帧采用协议规定的初始计数，最终帧使用上游用量。
                let initial = json!({"input_tokens":0,"output_tokens":0});
                vec![frame(
                    "message_start",
                    json!({"message":message.message(vec![], None, initial)}),
                )]
            }
            "response.in_progress"
            | "response.output_text.done"
            | "response.refusal.delta"
            | "response.refusal.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done" => vec![],
            "response.output_item.added" if self.started => {
                let item = &event["item"];
                match item["type"].as_str()? {
                    "message" | "reasoning" => Vec::new(),
                    "function_call" if message.allows_tools => {
                        let output_index = event["output_index"].as_u64()?;
                        let call_id = tool_id(&item["call_id"])?;
                        if self.tools.contains_key(&output_index)
                            || !self.call_ids.insert(call_id.to_owned())
                        {
                            return None;
                        }
                        let name = tool_name(&item["name"])?;
                        let arguments = item["arguments"].as_str().unwrap_or_default();
                        let index = self.next_index;
                        self.next_index += 1;
                        self.tools.insert(
                            output_index,
                            ToolBlock {
                                index,
                                item_id: tool_id(&item["id"])?.into(),
                                call_id: call_id.into(),
                                name: name.into(),
                                arguments: arguments.into(),
                                done: false,
                            },
                        );
                        let mut frames = vec![frame(
                            "content_block_start",
                            json!({"index":index,"content_block":{"type":"tool_use","id":call_id,"name":name,"input":{}}}),
                        )];
                        if !arguments.is_empty() {
                            frames.push(tool_delta(index, arguments));
                        }
                        frames
                    }
                    _ => return None,
                }
            }
            "response.function_call_arguments.delta" => {
                let block = self.tools.get_mut(&event["output_index"].as_u64()?)?;
                if block.done || event["item_id"] != block.item_id {
                    return None;
                }
                let delta = event["delta"].as_str()?;
                block.arguments.push_str(delta);
                vec![tool_delta(block.index, delta)]
            }
            "response.function_call_arguments.done" => {
                let block = self.tools.get_mut(&event["output_index"].as_u64()?)?;
                if block.done || event["item_id"] != block.item_id {
                    return None;
                }
                block.finish_arguments(event["arguments"].as_str()?)?
            }
            "response.output_item.done" if event["item"]["type"] == "function_call" => {
                let item = &event["item"];
                let mut block = self.tools.remove(&event["output_index"].as_u64()?)?;
                if item["id"] != block.item_id
                    || item["call_id"] != block.call_id
                    || item["name"] != block.name
                    || item["status"]
                        .as_str()
                        .is_some_and(|status| status != "completed")
                {
                    return None;
                }
                let mut frames = block.finish_arguments(item["arguments"].as_str()?)?;
                frames.push(frame("content_block_stop", json!({"index":block.index})));
                frames
            }
            "response.output_item.done" => Vec::new(),
            "response.content_part.added" if self.started => {
                if event["part"]["type"] == "refusal" {
                    return Some((vec![], false));
                }
                if event["part"]["type"] != "output_text" {
                    return None;
                }
                let key = (
                    event["output_index"].as_u64()?,
                    event["content_index"].as_u64()?,
                );
                let index = self.next_index;
                if self.blocks.insert(key, index).is_some() {
                    return None;
                }
                self.next_index += 1;
                vec![frame(
                    "content_block_start",
                    json!({"index":index,"content_block":text_block(event["part"]["text"].as_str().unwrap_or_default())}),
                )]
            }
            "response.output_text.delta" => {
                let index = self.blocks.get(&(
                    event["output_index"].as_u64()?,
                    event["content_index"].as_u64()?,
                ))?;
                let text = event["delta"].as_str()?;
                vec![frame(
                    "content_block_delta",
                    json!({"index":index,"delta":{"type":"text_delta","text":text}}),
                )]
            }
            "response.content_part.done" => {
                if event["part"]["type"] == "refusal" {
                    return Some((vec![], false));
                }
                let key = (
                    event["output_index"].as_u64()?,
                    event["content_index"].as_u64()?,
                );
                let index = self.blocks.remove(&key)?;
                vec![frame("content_block_stop", json!({"index":index}))]
            }
            "response.completed" | "response.incomplete" if self.started => {
                let response = &event["response"];
                let mut reason = finish(response)?;
                if !self.tools.is_empty() {
                    return None;
                }
                if reason == "end_turn" && !self.call_ids.is_empty() {
                    reason = "tool_use";
                }
                if self.next_index == 0 && reason == "end_turn" {
                    return None;
                }
                let mut frames = std::mem::take(&mut self.blocks)
                    .into_values()
                    .map(|index| frame("content_block_stop", json!({"index":index})))
                    .collect::<Vec<_>>();
                frames.extend([
                    frame("message_delta", json!({"delta":{"stop_reason":reason,"stop_sequence":null},"usage":usage(&response["usage"])})),
                    frame("message_stop", json!({})),
                ]);
                return Some((frames, true));
            }
            _ => return None,
        };
        Some((frames, false))
    }
}

fn frame(kind: &str, mut data: Value) -> String {
    data["type"] = kind.into();
    format!("event: {kind}\ndata: {data}\n\n")
}
