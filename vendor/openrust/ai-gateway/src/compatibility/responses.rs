use std::time::{SystemTime, UNIX_EPOCH};

use openrust::support::Str;
use serde_json::{Map, Value, json};

/// 将 Responses 的无状态文本请求转换成 Anthropic Messages。
pub(crate) fn to_messages(request: Map<String, Value>) -> Result<Value, &'static str> {
    let mut target = Map::new();
    target.insert("model".into(), request["model"].clone());
    target.insert("max_tokens".into(), json!(64_000));
    for (key, value) in &request {
        match key.as_str() {
            "model" | "input" | "instructions" | "store" | "background" => {}
            "stream" | "temperature" | "top_p" if !value.is_null() => {
                target.insert(key.clone(), value.clone());
            }
            "max_output_tokens" if !value.is_null() => {
                let count = value
                    .as_u64()
                    .filter(|count| *count > 0)
                    .ok_or("max_output_tokens 必须是正整数")?;
                target.insert("max_tokens".into(), json!(count));
            }
            _ if value.is_null() => {}
            _ => return Err("当前 Responses 到 Messages 转换只支持文本、流及基本采样参数"),
        }
    }

    let mut system = Vec::new();
    if let Some(instructions) = request.get("instructions").filter(|value| !value.is_null()) {
        system.push(
            instructions
                .as_str()
                .ok_or("instructions 必须是文本")?
                .to_owned(),
        );
    }
    let input = request.get("input").ok_or("input 必须是文本或消息数组")?;
    let mut messages = Vec::new();
    if let Some(input) = input.as_str() {
        messages.push(json!({"role":"user","content":input}));
    } else {
        let items = input.as_array().ok_or("input 必须是文本或消息数组")?;
        for item in items {
            if !matches!(item.get("type"), None | Some(Value::Null)) && item["type"] != "message" {
                return Err("input 只支持文本消息");
            }
            if item.as_object().is_none_or(|item| {
                item.keys()
                    .any(|key| !matches!(key.as_str(), "type" | "role" | "content" | "status"))
            }) {
                return Err("input 含有当前协议转换暂未支持的消息字段");
            }
            let role = item["role"].as_str().ok_or("消息缺少 role")?;
            let content = text_parts(&item["content"], role)?;
            match role {
                "user" | "assistant" => messages.push(json!({
                    "role": role,
                    "content": content.into_iter().map(|text| json!({"type":"text","text":text})).collect::<Vec<_>>()
                })),
                "system" | "developer" if messages.is_empty() => system.extend(content),
                "system" | "developer" => return Err("系统指令必须位于对话消息之前"),
                _ => return Err("消息 role 不受支持"),
            }
        }
    }
    if messages.is_empty() {
        return Err("至少提供一条对话消息");
    }
    if !system.is_empty() {
        target.insert("system".into(), Value::String(system.join("\n\n")));
    }
    target.insert("messages".into(), messages.into());
    Ok(Value::Object(target))
}

fn text_parts(content: &Value, role: &str) -> Result<Vec<String>, &'static str> {
    if let Some(content) = content.as_str() {
        return Ok(vec![content.to_owned()]);
    }
    content
        .as_array()
        .ok_or("消息内容必须是文本或文本块")?
        .iter()
        .map(|part| {
            if (part["type"] == "input_text"
                || (role == "assistant" && part["type"] == "output_text"))
                && part.as_object().is_some_and(|part| {
                    part.keys()
                        .all(|key| matches!(key.as_str(), "type" | "text"))
                })
            {
                return part["text"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("文本块缺少 text");
            }
            Err("消息内容含有当前协议转换暂未支持的文本块")
        })
        .collect()
}

pub(crate) struct Response {
    id: String,
    model: String,
    created_at: u64,
    output: Vec<Value>,
    sequence: u64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl Response {
    pub(crate) fn new(model: String) -> Self {
        Self {
            id: format!("resp_{}", Str::ulid()),
            model,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            output: Vec::new(),
            sequence: 0,
            input_tokens: None,
            output_tokens: None,
        }
    }

    pub(crate) fn buffered(mut self, message: &Value) -> Option<Value> {
        self.observe_usage(&message["usage"]);
        let finish = Finish::parse(message["stop_reason"].as_str()?)?;
        for content in message["content"].as_array()? {
            let part = content["text"].as_str()?;
            if content["type"] != "text" {
                return None;
            }
            self.output.push(output_item(
                &message_id(),
                finish.item_status(),
                vec![text_part(part)],
            ));
        }
        Some(self.response(Some(finish)))
    }

    fn observe_usage(&mut self, usage: &Value) {
        self.input_tokens = usage["input_tokens"].as_u64().or(self.input_tokens);
        self.output_tokens = usage["output_tokens"].as_u64().or(self.output_tokens);
    }

    fn response(&self, finish: Option<Finish>) -> Value {
        let usage = match (self.input_tokens, self.output_tokens) {
            (Some(input), Some(output)) => {
                json!({"input_tokens":input,"output_tokens":output,"total_tokens":input.saturating_add(output),"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}})
            }
            _ => Value::Null,
        };
        json!({
            "id":self.id, "object":"response", "created_at":self.created_at, "model":self.model,
            "status":finish.map_or("in_progress", Finish::item_status),
            "output":self.output, "error":null, "store":false, "usage":usage,
            "incomplete_details":finish.and_then(Finish::incomplete_reason).map(|reason| json!({"reason":reason})),
            "parallel_tool_calls":false, "tool_choice":"none", "tools":[],
            "text":{"format":{"type":"text"}},
        })
    }

    fn frame(&mut self, kind: &str, mut payload: Value) -> String {
        payload["type"] = kind.into();
        payload["sequence_number"] = self.sequence.into();
        self.sequence += 1;
        format!("event: {kind}\ndata: {payload}\n\n")
    }
}

fn message_id() -> String {
    format!("msg_{}", Str::ulid())
}

fn text_part(text: &str) -> Value {
    json!({"type":"output_text","text":text,"annotations":[],"logprobs":[]})
}

fn output_item(id: &str, status: &str, content: Vec<Value>) -> Value {
    json!({"id":id,"type":"message","role":"assistant","status":status,"content":content})
}

#[derive(Clone, Copy)]
enum Finish {
    Complete,
    Length,
    Refused,
}

impl Finish {
    fn parse(reason: &str) -> Option<Self> {
        match reason {
            "end_turn" | "stop_sequence" => Some(Self::Complete),
            "max_tokens" | "model_context_window_exceeded" => Some(Self::Length),
            "refusal" => Some(Self::Refused),
            _ => None,
        }
    }

    fn item_status(self) -> &'static str {
        if self.incomplete_reason().is_some() {
            "incomplete"
        } else {
            "completed"
        }
    }

    fn incomplete_reason(self) -> Option<&'static str> {
        match self {
            Self::Complete => None,
            Self::Length => Some("max_output_tokens"),
            Self::Refused => Some("content_filter"),
        }
    }
}

#[derive(Default)]
pub(crate) struct StreamState {
    started: bool,
    active: Option<Block>,
    finish: Option<Finish>,
}

struct Block {
    index: u64,
    id: String,
    text: String,
}

impl StreamState {
    /// 返回同一上游事件产生的 Responses 帧。最后一项给出是否收到终止事件。
    pub(crate) fn convert(
        &mut self,
        response: &mut Response,
        event: &Value,
    ) -> Option<(Vec<String>, bool)> {
        let frames = match event["type"].as_str()? {
            "ping" => vec![],
            "message_start" if !self.started => {
                self.started = true;
                response.observe_usage(&event["message"]["usage"]);
                let body = json!({"response":response.response(None)});
                vec![
                    response.frame("response.created", body.clone()),
                    response.frame("response.in_progress", body),
                ]
            }
            "content_block_start" if self.started && self.active.is_none() => {
                let part = &event["content_block"];
                if part["type"] != "text" {
                    return None;
                }
                let id = message_id();
                let text = part["text"].as_str()?.to_owned();
                let index = event["index"].as_u64()?;
                self.active = Some(Block {
                    index,
                    id: id.clone(),
                    text: text.clone(),
                });
                let output_index = response.output.len();
                vec![
                    response.frame("response.output_item.added", json!({"output_index":output_index,"item":output_item(&id, "in_progress", vec![])})),
                    response.frame("response.content_part.added", json!({"output_index":output_index,"content_index":0,"item_id":id,"part":text_part(&text)})),
                ]
            }
            "content_block_delta" if event["delta"]["type"] == "text_delta" => {
                let block = self
                    .active
                    .as_mut()
                    .filter(|block| Some(block.index) == event["index"].as_u64())?;
                let delta = event["delta"]["text"].as_str()?;
                block.text.push_str(delta);
                vec![response.frame("response.output_text.delta", json!({"output_index":response.output.len(),"content_index":0,"item_id":block.id,"delta":delta,"logprobs":[]}))]
            }
            "content_block_stop" => {
                let block = self
                    .active
                    .take()
                    .filter(|block| Some(block.index) == event["index"].as_u64())?;
                let output_index = response.output.len();
                let part = text_part(&block.text);
                let item = output_item(&block.id, "completed", vec![part.clone()]);
                response.output.push(item.clone());
                vec![
                    response.frame("response.output_text.done", json!({"output_index":output_index,"content_index":0,"item_id":block.id,"text":block.text,"logprobs":[]})),
                    response.frame("response.content_part.done", json!({"output_index":output_index,"content_index":0,"item_id":block.id,"part":part})),
                    response.frame("response.output_item.done", json!({"output_index":output_index,"item":item})),
                ]
            }
            "message_delta" if self.started && self.active.is_none() => {
                response.observe_usage(&event["usage"]);
                if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                    self.finish = Finish::parse(reason);
                }
                vec![]
            }
            "message_stop" if self.started && self.active.is_none() => {
                let finish = self.finish?;
                let kind = if finish.incomplete_reason().is_some() {
                    "response.incomplete"
                } else {
                    "response.completed"
                };
                let body = json!({"response":response.response(Some(finish))});
                return Some((vec![response.frame(kind, body)], true));
            }
            _ => return None,
        };
        Some((frames, false))
    }
}
