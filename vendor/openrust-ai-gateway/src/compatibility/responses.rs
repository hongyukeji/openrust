use std::time::{SystemTime, UNIX_EPOCH};

use openrust::support::Str;
use serde_json::{Map, Value, json};

use super::tools::{
    ToolHistory, allowed_fields, arguments, response_choice, tool_definition, tool_id, tool_name,
};

/// 将 Responses 的无状态请求转换成 Anthropic Messages。
pub(crate) fn to_messages(request: Map<String, Value>) -> Result<Value, &'static str> {
    let mut target = Map::new();
    target.insert("model".into(), request["model"].clone());
    target.insert("max_tokens".into(), json!(64_000));
    for (key, value) in &request {
        match key.as_str() {
            "model"
            | "input"
            | "instructions"
            | "store"
            | "background"
            | "tools"
            | "tool_choice"
            | "parallel_tool_calls" => {}
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
            _ => return Err("当前 Responses 到 Messages 转换不支持该请求字段"),
        }
    }

    if let Some(tools) = request.get("tools").filter(|tools| !tools.is_null()) {
        let tools = tools.as_array().ok_or("tools 必须是数组")?;
        if !tools.is_empty() {
            let definitions = tools
                .iter()
                .map(|tool| tool_definition(tool, true))
                .collect::<Result<Vec<_>, _>>()?;
            let choice = response_choice(
                request.get("tool_choice").unwrap_or(&Value::Null),
                request.get("parallel_tool_calls").unwrap_or(&Value::Null),
            )?;
            target.insert("tools".into(), definitions.into());
            target.insert("tool_choice".into(), choice);
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
        let mut tools = ToolHistory::default();
        for item in items {
            match item["type"].as_str() {
                Some("function_call") => {
                    if !allowed_fields(
                        item,
                        &["type", "id", "call_id", "name", "arguments", "status"],
                    ) || item["status"]
                        .as_str()
                        .is_some_and(|value| value != "completed")
                    {
                        return Err("工具调用状态或字段不受协议转换支持");
                    }
                    let id = tool_id(&item["call_id"]).ok_or("缺少有效的工具调用 ID")?;
                    let name = tool_name(&item["name"]).ok_or("工具名称无效")?;
                    let input = arguments(&item["arguments"]).ok_or("工具参数必须是 JSON 对象")?;
                    tools.call(id)?;
                    append_blocks(
                        &mut messages,
                        "assistant",
                        vec![json!({"type":"tool_use","id":id,"name":name,"input":input})],
                    );
                    continue;
                }
                Some("function_call_output") => {
                    if !allowed_fields(item, &["type", "id", "call_id", "output", "status"]) {
                        return Err("工具结果字段不受协议转换支持");
                    }
                    let id = tool_id(&item["call_id"]).ok_or("缺少有效的工具调用 ID")?;
                    let output = item["output"].as_str().ok_or("工具结果必须是文本")?;
                    tools.result(id)?;
                    append_blocks(
                        &mut messages,
                        "user",
                        vec![json!({"type":"tool_result","tool_use_id":id,"content":output})],
                    );
                    continue;
                }
                None if item["type"].is_null() => {}
                Some("message") => {}
                _ => return Err("input 只支持文本消息和客户端函数工具"),
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
                "user" | "assistant" => append_blocks(
                    &mut messages,
                    role,
                    content
                        .into_iter()
                        .map(|text| json!({"type":"text","text":text}))
                        .collect(),
                ),
                "system" | "developer" if messages.is_empty() => system.extend(content),
                "system" | "developer" => return Err("系统指令必须位于对话消息之前"),
                _ => return Err("消息 role 不受支持"),
            }
        }
        tools.finish()?;
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

fn append_blocks(messages: &mut Vec<Value>, role: &str, blocks: Vec<Value>) {
    if let Some(content) = messages
        .last_mut()
        .filter(|message| message["role"] == role)
        .and_then(|message| message["content"].as_array_mut())
    {
        content.extend(blocks);
    } else {
        messages.push(json!({"role":role,"content":blocks}));
    }
}

pub(super) fn text_parts(content: &Value, role: &str) -> Result<Vec<String>, &'static str> {
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
    tool_fields: Option<Map<String, Value>>,
    sequence: u64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_creation_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
}

impl Response {
    pub(crate) fn new(model: String, tool_fields: Option<Map<String, Value>>) -> Self {
        Self {
            id: format!("resp_{}", Str::ulid()),
            model,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            output: Vec::new(),
            tool_fields,
            sequence: 0,
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        }
    }

    pub(crate) fn buffered(mut self, message: &Value) -> Option<Value> {
        self.observe_usage(&message["usage"]);
        let reason = message["stop_reason"].as_str()?;
        let finish = Finish::parse(reason)?;
        let mut calls = std::collections::HashSet::new();
        for content in message["content"].as_array()? {
            match content["type"].as_str()? {
                "text" => self.output.push(output_item(
                    &message_id(),
                    finish.item_status(),
                    vec![text_part(content["text"].as_str()?)],
                )),
                "tool_use" if reason == "tool_use" && self.tool_fields.is_some() => {
                    let id = tool_id(&content["id"])?;
                    let name = tool_name(&content["name"])?;
                    if !content["input"].is_object() || !calls.insert(id) {
                        return None;
                    }
                    self.output.push(json!({
                        "id":format!("fc_{}", Str::ulid()),"type":"function_call","status":"completed",
                        "call_id":id,"name":name,"arguments":content["input"].to_string(),
                    }));
                }
                _ => return None,
            }
        }
        if reason == "tool_use" && calls.is_empty() {
            return None;
        }
        if self.output.is_empty() && matches!(finish, Finish::Complete) {
            return None;
        }
        Some(self.response(Some(finish)))
    }

    fn observe_usage(&mut self, usage: &Value) {
        self.input_tokens = usage["input_tokens"].as_u64().or(self.input_tokens);
        self.output_tokens = usage["output_tokens"].as_u64().or(self.output_tokens);
        self.cache_read_tokens = usage["cache_read_input_tokens"]
            .as_u64()
            .or(self.cache_read_tokens);
        self.cache_creation_tokens = usage["cache_creation_input_tokens"]
            .as_u64()
            .or(self.cache_creation_tokens);
    }

    pub(super) fn response(&self, finish: Option<Finish>) -> Value {
        let usage = match (self.input_tokens, self.output_tokens) {
            (Some(input), Some(output)) => {
                let cached = self.cache_read_tokens.unwrap_or_default();
                let input = input
                    .saturating_add(cached)
                    .saturating_add(self.cache_creation_tokens.unwrap_or_default());
                let mut usage = json!({
                    "input_tokens":input,"output_tokens":output,"total_tokens":input.saturating_add(output),
                    "input_tokens_details":{"cached_tokens":cached},"output_tokens_details":{"reasoning_tokens":self.reasoning_tokens.unwrap_or_default()},
                });
                if let Some(created) = self.cache_creation_tokens {
                    usage["cache_creation_input_tokens"] = created.into();
                }
                usage
            }
            _ => Value::Null,
        };
        let mut result = json!({
            "id":self.id, "object":"response", "created_at":self.created_at, "model":self.model,
            "status":finish.map_or("in_progress", Finish::item_status),
            "output":self.output, "error":null, "store":false, "usage":usage,
            "incomplete_details":finish.and_then(Finish::incomplete_reason).map(|reason| json!({"reason":reason})),
            "parallel_tool_calls":false, "tool_choice":"none", "tools":[],
            "text":{"format":{"type":"text"}},
        });
        if let Some(fields) = &self.tool_fields {
            result
                .as_object_mut()
                .expect("Responses 响应为 JSON 对象")
                .extend(fields.clone());
        }
        result
    }

    pub(super) fn observe_chat_usage(&mut self, usage: &Value) {
        if let Some(input) = usage["prompt_tokens"].as_u64() {
            let cached = usage["prompt_tokens_details"]["cached_tokens"]
                .as_u64()
                .or_else(|| usage["prompt_cache_hit_tokens"].as_u64())
                .filter(|cached| *cached <= input);
            self.input_tokens = Some(input - cached.unwrap_or_default());
            self.cache_read_tokens = cached;
        }
        self.output_tokens = usage["completion_tokens"].as_u64().or(self.output_tokens);
        self.reasoning_tokens = usage["completion_tokens_details"]["reasoning_tokens"]
            .as_u64()
            .or(self.reasoning_tokens);
    }

    pub(super) fn push_text(&mut self, text: &str, finish: Finish) {
        self.output.push(output_item(
            &message_id(),
            finish.item_status(),
            vec![text_part(text)],
        ));
    }

    pub(super) fn start(&mut self) -> [Frame; 2] {
        let body = json!({"response":self.response(None)});
        [
            self.frame("response.created", body.clone()),
            self.frame("response.in_progress", body),
        ]
    }

    pub(super) fn start_text(&mut self, text: String) -> (TextOutput, [Frame; 2]) {
        let id = message_id();
        let output_index = self.output.len();
        let frames = [
            self.frame("response.output_item.added", json!({"output_index":output_index,"item":output_item(&id, "in_progress", vec![])})),
            self.frame("response.content_part.added", json!({"output_index":output_index,"content_index":0,"item_id":id,"part":text_part(&text)})),
        ];
        (TextOutput { id, text }, frames)
    }

    pub(super) fn text_delta(&mut self, id: &str, delta: &str) -> Frame {
        self.frame("response.output_text.delta", json!({"output_index":self.output.len(),"content_index":0,"item_id":id,"delta":delta,"logprobs":[]}))
    }

    fn text_done(&mut self, id: &str, text: &str, status: &str) -> (Value, Vec<Frame>) {
        let output_index = self.output.len();
        let part = text_part(text);
        let item = output_item(id, status, vec![part.clone()]);
        let frames = vec![
            self.frame("response.output_text.done", json!({"output_index":output_index,"content_index":0,"item_id":id,"text":text,"logprobs":[]})),
            self.frame("response.content_part.done", json!({"output_index":output_index,"content_index":0,"item_id":id,"part":part})),
        ];
        (item, frames)
    }

    fn push_item(&mut self, item: Value) -> Frame {
        let output_index = self.output.len();
        self.output.push(item.clone());
        self.frame(
            "response.output_item.done",
            json!({"output_index":output_index,"item":item}),
        )
    }

    pub(super) fn end_text(&mut self, text: TextOutput, finish: Finish) -> Vec<Frame> {
        let (item, mut frames) = self.text_done(&text.id, &text.text, finish.item_status());
        frames.push(self.push_item(item));
        frames
    }

    pub(super) fn push_function(&mut self, call_id: &str, name: &str, arguments: &str) {
        let item_id = format!("fc_{}", Str::ulid());
        self.output.push(function_item(
            &item_id,
            call_id,
            name,
            arguments,
            "completed",
        ));
    }

    pub(super) fn start_function(
        &mut self,
        output_index: usize,
        call_id: &str,
        name: &str,
    ) -> (String, Frame) {
        let item_id = format!("fc_{}", Str::ulid());
        let item = function_item(&item_id, call_id, name, "", "in_progress");
        let frame = self.frame(
            "response.output_item.added",
            json!({"output_index":output_index,"item":item}),
        );
        (item_id, frame)
    }

    pub(super) fn function_delta(
        &mut self,
        output_index: usize,
        item_id: &str,
        delta: &str,
    ) -> Frame {
        self.frame(
            "response.function_call_arguments.delta",
            json!({"output_index":output_index,"item_id":item_id,"delta":delta}),
        )
    }

    pub(super) fn end_function(
        &mut self,
        item_id: &str,
        call_id: &str,
        name: &str,
        arguments: &str,
    ) -> [Frame; 2] {
        let done = self.frame(
            "response.function_call_arguments.done",
            json!({"output_index":self.output.len(),"item_id":item_id,"name":name,"arguments":arguments}),
        );
        let item = function_item(item_id, call_id, name, arguments, "completed");
        [done, self.push_item(item)]
    }

    pub(super) fn terminal(&mut self, finish: Finish) -> Frame {
        let kind = if finish.incomplete_reason().is_some() {
            "response.incomplete"
        } else {
            "response.completed"
        };
        let body = json!({"response":self.response(Some(finish))});
        self.frame(kind, body)
    }

    fn frame(&mut self, kind: &'static str, mut payload: Value) -> Frame {
        payload["type"] = kind.into();
        payload["sequence_number"] = self.sequence.into();
        self.sequence += 1;
        Frame { kind, payload }
    }
}

pub(super) struct Frame {
    pub(super) kind: &'static str,
    pub(super) payload: Value,
}

impl Frame {
    pub(super) fn wire(self) -> String {
        format!("event: {}\ndata: {}\n\n", self.kind, self.payload)
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
pub(super) enum Finish {
    Complete,
    ToolUse,
    Length,
    Refused,
}

impl Finish {
    fn parse(reason: &str) -> Option<Self> {
        match reason {
            "end_turn" | "stop_sequence" => Some(Self::Complete),
            "tool_use" => Some(Self::ToolUse),
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
            Self::Complete | Self::ToolUse => None,
            Self::Length => Some("max_output_tokens"),
            Self::Refused => Some("content_filter"),
        }
    }
}

pub(super) struct TextOutput {
    pub(super) id: String,
    pub(super) text: String,
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
    content: BlockContent,
}

enum BlockContent {
    Text(String),
    Tool {
        call_id: String,
        name: String,
        initial: Value,
        arguments: String,
    },
}

impl StreamState {
    /// 返回同一上游事件产生的 Responses 帧。最后一项给出是否收到终止事件。
    pub(super) fn convert(
        &mut self,
        response: &mut Response,
        event: &Value,
    ) -> Option<(Vec<Frame>, bool)> {
        let frames = match event["type"].as_str()? {
            "ping" => vec![],
            "message_start" if !self.started => {
                self.started = true;
                response.observe_usage(&event["message"]["usage"]);
                response.start().into()
            }
            "content_block_start" if self.started && self.active.is_none() => {
                let part = &event["content_block"];
                let index = event["index"].as_u64()?;
                let output_index = response.output.len();
                let (id, content, frames) = match part["type"].as_str()? {
                    "text" => {
                        let text = part["text"].as_str()?.to_owned();
                        let (TextOutput { id, text }, frames) = response.start_text(text);
                        (id, BlockContent::Text(text), frames.into())
                    }
                    "tool_use" if response.tool_fields.is_some() => {
                        let call_id = tool_id(&part["id"])?.to_owned();
                        if !part["input"].is_object()
                            || response
                                .output
                                .iter()
                                .any(|item| item["call_id"] == call_id)
                        {
                            return None;
                        }
                        let name = tool_name(&part["name"])?.to_owned();
                        let (id, frame) = response.start_function(output_index, &call_id, &name);
                        let frames = vec![frame];
                        (
                            id,
                            BlockContent::Tool {
                                call_id,
                                name,
                                initial: part["input"].clone(),
                                arguments: String::new(),
                            },
                            frames,
                        )
                    }
                    _ => return None,
                };
                self.active = Some(Block { index, id, content });
                frames
            }
            "content_block_delta" => {
                let block = self
                    .active
                    .as_mut()
                    .filter(|block| Some(block.index) == event["index"].as_u64())?;
                let output_index = response.output.len();
                match (&mut block.content, event["delta"]["type"].as_str()?) {
                    (BlockContent::Text(text), "text_delta") => {
                        let delta = event["delta"]["text"].as_str()?;
                        text.push_str(delta);
                        vec![response.text_delta(&block.id, delta)]
                    }
                    (
                        BlockContent::Tool {
                            arguments, initial, ..
                        },
                        "input_json_delta",
                    ) if initial.as_object()?.is_empty() => {
                        let delta = event["delta"]["partial_json"].as_str()?;
                        arguments.push_str(delta);
                        vec![response.function_delta(output_index, &block.id, delta)]
                    }
                    _ => return None,
                }
            }
            "content_block_stop" => {
                let block = self
                    .active
                    .take()
                    .filter(|block| Some(block.index) == event["index"].as_u64())?;
                let output_index = response.output.len();
                let (item, mut frames) = match block.content {
                    BlockContent::Text(text) => response.text_done(&block.id, &text, "completed"),
                    BlockContent::Tool {
                        call_id,
                        name,
                        initial,
                        mut arguments,
                    } => {
                        let mut frames = Vec::new();
                        if arguments.is_empty() {
                            arguments = initial.to_string();
                            frames.push(response.frame("response.function_call_arguments.delta", json!({"output_index":output_index,"item_id":block.id,"delta":arguments})));
                        } else {
                            serde_json::from_str::<Map<String, Value>>(&arguments).ok()?;
                        }
                        frames.push(response.frame("response.function_call_arguments.done", json!({"output_index":output_index,"item_id":block.id,"name":name,"arguments":arguments})));
                        (
                            function_item(&block.id, &call_id, &name, &arguments, "completed"),
                            frames,
                        )
                    }
                };
                frames.push(response.push_item(item));
                frames
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
                let has_call = response
                    .output
                    .iter()
                    .any(|item| item["type"] == "function_call");
                if (response.output.is_empty() && matches!(finish, Finish::Complete))
                    || (matches!(finish, Finish::ToolUse) != has_call
                        && finish.incomplete_reason().is_none())
                {
                    return None;
                }
                return Some((vec![response.terminal(finish)], true));
            }
            _ => return None,
        };
        Some((frames, false))
    }
}

fn function_item(id: &str, call_id: &str, name: &str, arguments: &str, status: &str) -> Value {
    json!({"id":id,"type":"function_call","status":status,"call_id":call_id,"name":name,"arguments":arguments})
}
