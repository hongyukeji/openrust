use openrust_ai::CHAT_REASONING_REPLAY;
use serde_json::{Map, Value, json};

use super::{
    chat_tools::{self, History, StreamTools},
    responses::{Finish, Frame, Response, text_parts},
    tools::allowed_fields,
};

/// 将 Responses 的无状态请求发送到 Chat Completions。
pub(crate) fn from_responses(
    request: Map<String, Value>,
    signed_reasoning: bool,
) -> Result<Value, &'static str> {
    let mut target = Map::new();
    target.insert("model".into(), request["model"].clone());
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
                target.insert("max_tokens".into(), count.into());
            }
            "text" if allowed_fields(value, &["format"]) => {
                if let Some(format) = chat_format(&value["format"])? {
                    target.insert("response_format".into(), format);
                }
            }
            "include"
                if signed_reasoning
                    && value.as_array().is_some_and(|items| {
                        items
                            .iter()
                            .all(|item| item == "reasoning.encrypted_content")
                    }) => {}
            "tools" | "tool_choice" | "parallel_tool_calls" => {}
            _ if value.is_null() => {}
            _ => return Err("当前 Responses 到 Chat Completions 转换不支持该请求字段"),
        }
    }

    chat_tools::configure(&request, &mut target)?;
    let mut messages = Vec::new();
    if let Some(value) = request.get("instructions").filter(|value| !value.is_null()) {
        let instructions = value.as_str().ok_or("instructions 必须是文本")?;
        messages.push(json!({"role":"system","content":instructions}));
    }
    let input = request.get("input").ok_or("input 必须是文本或消息数组")?;
    if let Some(text) = input.as_str() {
        messages.push(json!({"role":"user","content":text}));
    } else {
        let items = input.as_array().ok_or("input 必须是文本或消息数组")?;
        let mut history = History::default();
        for item in items {
            if history.append(item, &mut messages, signed_reasoning)? {
                continue;
            }
            if !allowed_fields(item, &["type", "role", "content", "status", "id"])
                || !(item["type"].is_null() || item["type"] == "message")
                || !(item["status"].is_null() || item["status"] == "completed")
                || !(item["id"].is_null() || item["role"] == "assistant" && item["id"].is_string())
            {
                return Err("当前 Chat Completions 转换只支持已完成的文本消息");
            }
            let role = match item["role"].as_str() {
                Some("system" | "developer") => "system",
                Some("user") => "user",
                Some("assistant") => "assistant",
                _ => return Err("消息 role 不受支持"),
            };
            messages.push(json!({"role":role,"content":chat_content(&item["content"], role)?}));
        }
        history.finish()?;
    }
    if !messages
        .iter()
        .any(|message| message["role"] == "user" || message["role"] == "assistant")
    {
        return Err("至少提供一条对话消息");
    }
    target.insert("messages".into(), messages.into());
    if target.get("stream").is_some_and(|value| value == true) {
        target.insert("stream_options".into(), json!({"include_usage":true}));
    }
    Ok(target.into())
}

fn chat_content(content: &Value, role: &str) -> Result<Value, &'static str> {
    if role == "assistant"
        && let Some(parts) = content.as_array()
    {
        let mut text = String::new();
        for part in parts {
            let plain = part["type"] == "input_text" && allowed_fields(part, &["type", "text"]);
            let output = part["type"] == "output_text"
                && allowed_fields(part, &["type", "text", "annotations", "logprobs"])
                && ["annotations", "logprobs"].iter().all(|field| {
                    part[*field].is_null() || part[*field].as_array().is_some_and(Vec::is_empty)
                });
            if !plain && !output {
                return Err("助手消息含有当前 Chat 转换不支持的文本块");
            }
            text.push_str(part["text"].as_str().ok_or("文本块缺少 text")?);
        }
        return Ok(text.into());
    }
    let Some(parts) = content.as_array().filter(|parts| {
        role == "user"
            && parts
                .iter()
                .any(|part| matches!(part["type"].as_str(), Some("input_image" | "input_file")))
    }) else {
        return Ok(text_parts(content, role)?.concat().into());
    };
    let parts = parts.iter().map(|part| {
        match part["type"].as_str() {
            Some("input_text") if allowed_fields(part, &["type", "text"]) && part["text"].is_string() => {
                Ok(json!({"type":"text","text":part["text"]}))
            }
            Some("input_image") if allowed_fields(part, &["type", "image_url", "detail", "file_id"])
                && part["file_id"].is_null()
                && part["image_url"].as_str().is_some_and(|url| url.starts_with("data:"))
                && (part["detail"].is_null() || part["detail"].is_string()) =>
            {
                let mut image = json!({"url":part["image_url"]});
                if !part["detail"].is_null() {
                    image["detail"] = part["detail"].clone();
                }
                Ok(json!({"type":"image_url","image_url":image}))
            }
            Some("input_file") if allowed_fields(part, &["type", "filename", "file_data", "file_id", "file_url"])
                && part["file_url"].is_null() && part["file_id"].is_null()
                && part["filename"].is_string() && part["file_data"].is_string() =>
            {
                Ok(json!({"type":"file","file":{"filename":part["filename"],"file_data":part["file_data"]}}))
            }
            _ => Err("Responses 到 Chat Completions 仅转换文本和内联图片或文档"),
        }
    }).collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(parts))
}

fn chat_format(format: &Value) -> Result<Option<Value>, &'static str> {
    match format["type"].as_str() {
        Some("text") if allowed_fields(format, &["type"]) => Ok(None),
        Some("json_object") if allowed_fields(format, &["type"]) => Ok(Some(format.clone())),
        Some("json_schema")
            if allowed_fields(format, &["type", "name", "description", "schema", "strict"])
                && format["name"].as_str().is_some_and(|name| !name.is_empty())
                && format["schema"].is_object()
                && (format["description"].is_null() || format["description"].is_string())
                && (format["strict"].is_null() || format["strict"].is_boolean()) =>
        {
            let mut schema = format.as_object().expect("format 通过对象检查").clone();
            schema.remove("type");
            Ok(Some(json!({"type":"json_schema","json_schema":schema})))
        }
        _ => Err("Responses 的 text.format 无法转换为 Chat Completions 输出格式"),
    }
}

fn finish(reason: &Value) -> Option<Finish> {
    match reason.as_str()? {
        "stop" => Some(Finish::Complete),
        "tool_calls" => Some(Finish::ToolUse),
        "length" => Some(Finish::Length),
        "content_filter" => Some(Finish::Refused),
        _ => None,
    }
}

pub(crate) fn response(
    model: String,
    response_fields: Option<Map<String, Value>>,
    allows_tools: bool,
    signed_reasoning: bool,
    data: &Value,
) -> Option<Value> {
    let [choice] = data["choices"].as_array()?.as_slice() else {
        return None;
    };
    let mut response = Response::new(model, response_fields);
    response.observe_chat_usage(&data["usage"]);
    let message = &choice["message"];
    if !message.is_object() || !message["function_call"].is_null() {
        return None;
    }
    let finish = if message["refusal"]
        .as_str()
        .is_some_and(|text| !text.is_empty())
    {
        Finish::Refused
    } else {
        finish(&choice["finish_reason"])?
    };
    if let Some(text) = message["content"].as_str() {
        response.push_text(text, finish);
    } else if matches!(finish, Finish::Complete) {
        return None;
    }
    if signed_reasoning && let Some(token) = message[CHAT_REASONING_REPLAY].as_str() {
        response.push_reasoning(token);
    }
    let calls = if message["tool_calls"].is_null() {
        Vec::new()
    } else {
        chat_tools::buffered(&message["tool_calls"], signed_reasoning)?
    };
    if (!calls.is_empty() && !allows_tools)
        || (matches!(finish, Finish::ToolUse) == calls.is_empty())
    {
        return None;
    }
    for call in calls {
        response.push_function(call.id, call.name, call.arguments);
    }
    Some(response.response(Some(finish)))
}

pub(crate) struct StreamState {
    response: Response,
    started: bool,
    text: Option<super::responses::TextOutput>,
    finish: Option<Finish>,
    refused: bool,
    allows_tools: bool,
    signed_reasoning: bool,
    reasoning_sent: bool,
    text_completed: bool,
    tools: StreamTools,
}

impl StreamState {
    pub(crate) fn new(
        model: String,
        response_fields: Option<Map<String, Value>>,
        allows_tools: bool,
        signed_reasoning: bool,
    ) -> Self {
        Self {
            response: Response::new(model, response_fields),
            started: false,
            text: None,
            finish: None,
            refused: false,
            allows_tools,
            signed_reasoning,
            reasoning_sent: false,
            text_completed: false,
            tools: StreamTools::default(),
        }
    }

    pub(super) fn convert(&mut self, data: &Value) -> Option<(Vec<Frame>, bool)> {
        let mut frames = Vec::new();
        if !data["error"].is_null() {
            return None;
        }
        let choices = data["choices"].as_array()?;
        self.response.observe_chat_usage(&data["usage"]);
        if !self.started {
            self.started = true;
            frames.extend(self.response.start());
        }
        let choice = match choices.as_slice() {
            [] => return Some((frames, false)),
            [choice] if self.finish.is_none() => choice,
            _ => return None,
        };
        let delta = &choice["delta"];
        if !delta.is_object() || !delta["function_call"].is_null() {
            return None;
        }
        if delta["refusal"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
        {
            self.refused = true;
        }
        if let Some(content) = delta["content"].as_str().filter(|text| !text.is_empty()) {
            if self.reasoning_sent || self.text.is_none() && !self.tools.is_empty() {
                return None;
            }
            let text = self.text.get_or_insert_with(|| {
                let (output, added) = self.response.start_text(String::new());
                frames.extend(added);
                output
            });
            text.text.push_str(content);
            frames.push(self.response.text_delta(&text.id, content));
        }
        if self.signed_reasoning
            && let Some(token) = delta[CHAT_REASONING_REPLAY].as_str()
        {
            if self.reasoning_sent || !self.tools.is_empty() {
                return None;
            }
            if let Some(text) = self.text.take() {
                frames.extend(self.response.end_text(text, Finish::Complete));
                self.text_completed = true;
            }
            frames.extend(self.response.emit_reasoning(token));
            self.reasoning_sent = true;
        }
        if !delta["tool_calls"].is_null() {
            if !self.allows_tools && !delta["tool_calls"].as_array().is_some_and(Vec::is_empty) {
                return None;
            }
            let offset = self.response.output_count() + usize::from(self.text.is_some());
            frames.extend(self.tools.delta(
                &mut self.response,
                &delta["tool_calls"],
                offset,
                self.signed_reasoning,
            )?);
        }
        if !choice["finish_reason"].is_null() {
            self.finish = Some(if self.refused {
                Finish::Refused
            } else {
                finish(&choice["finish_reason"])?
            });
        }
        Some((frames, false))
    }

    pub(super) fn finish(&mut self) -> Option<(Vec<Frame>, bool)> {
        let finish = self.finish.take()?;
        if (matches!(finish, Finish::ToolUse) == self.tools.is_empty())
            || (self.text.is_none() && !self.text_completed && matches!(finish, Finish::Complete))
        {
            return None;
        }
        let mut frames = Vec::new();
        if let Some(text) = self.text.take() {
            frames.extend(self.response.end_text(text, finish));
        }
        frames.extend(self.tools.finish(&mut self.response)?);
        frames.push(self.response.terminal(finish));
        Some((frames, true))
    }
}
