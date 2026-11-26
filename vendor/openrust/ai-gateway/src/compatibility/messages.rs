use std::collections::BTreeMap;

use openrust::support::Str;
use serde_json::{Map, Value, json};

/// 将 Anthropic 的无状态文本请求转换成 OpenAI Responses。
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
            "model" | "messages" | "system" | "max_tokens" => {}
            "stream" | "temperature" | "top_p" if !value.is_null() => {
                target.insert(key.clone(), value.clone());
            }
            _ if value.is_null() => {}
            _ => return Err("当前 Messages 到 Responses 转换只支持文本、流及基本采样参数"),
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
    let input = messages
        .iter()
        .map(|message| {
            if message.as_object().is_none_or(|message| {
                message
                    .keys()
                    .any(|key| !matches!(key.as_str(), "role" | "content"))
            }) {
                return Err("消息字段不受协议转换支持");
            }
            let role = message["role"]
                .as_str()
                .filter(|role| matches!(*role, "user" | "assistant"))
                .ok_or("消息 role 不受支持")?;
            let text = parts(&message["content"])?;
            // Responses 的普通文本输入接受 user 和 assistant 两种角色。
            Ok(json!({"type":"message","role":role,"content":text.join("\n")}))
        })
        .collect::<Result<Vec<_>, _>>()?;
    target.insert("input".into(), input.into());
    Ok(Value::Object(target))
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
}

impl Message {
    pub(crate) fn new(model: String) -> Self {
        Self {
            id: format!("msg_{}", Str::ulid()),
            model,
        }
    }

    pub(crate) fn buffered(&self, response: &Value) -> Option<Value> {
        let reason = finish(response)?;
        let mut blocks = Vec::new();
        for item in response["output"].as_array()? {
            match item["type"].as_str()? {
                "reasoning" => {}
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
    next_index: u64,
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
            | "response.output_item.done"
            | "response.output_text.done"
            | "response.refusal.delta"
            | "response.refusal.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done" => vec![],
            "response.output_item.added" => {
                if !matches!(
                    event["item"]["type"].as_str(),
                    Some("message" | "reasoning")
                ) {
                    return None;
                }
                vec![]
            }
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
                let reason = finish(response)?;
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
