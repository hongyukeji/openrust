use serde_json::{Map, Value, json};

use super::{
    responses::{Finish, Response, text_parts},
    tools::allowed_fields,
};

/// 将 Responses 的无状态文本请求发送到 Chat Completions。
pub(crate) fn from_responses(request: Map<String, Value>) -> Result<Value, &'static str> {
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
                let format = &value["format"];
                if !allowed_fields(format, &["type"])
                    || !matches!(format["type"].as_str(), Some("text" | "json_object"))
                {
                    return Err("当前 Chat Completions 转换只支持文本或 JSON 对象输出");
                }
                if format["type"] == "json_object" {
                    target.insert("response_format".into(), format.clone());
                }
            }
            "tools" if value.as_array().is_some_and(Vec::is_empty) => {}
            "tool_choice" if value == "none" || value == "auto" => {}
            "parallel_tool_calls" if value.is_boolean() => {}
            _ if value.is_null() => {}
            _ => return Err("当前 Responses 到 Chat Completions 转换不支持该请求字段"),
        }
    }

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
        for item in items {
            if !allowed_fields(item, &["type", "role", "content", "status"])
                || !(item["type"].is_null() || item["type"] == "message")
                || !(item["status"].is_null() || item["status"] == "completed")
            {
                return Err("当前 Chat Completions 转换只支持已完成的文本消息");
            }
            let role = match item["role"].as_str() {
                Some("system" | "developer") => "system",
                Some("user") => "user",
                Some("assistant") => "assistant",
                _ => return Err("消息 role 不受支持"),
            };
            let text = text_parts(&item["content"], role)?.concat();
            messages.push(json!({"role":role,"content":text}));
        }
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

fn finish(reason: &Value) -> Option<Finish> {
    match reason.as_str()? {
        "stop" => Some(Finish::Complete),
        "length" => Some(Finish::Length),
        "content_filter" => Some(Finish::Refused),
        _ => None,
    }
}

pub(crate) fn response(
    model: String,
    response_fields: Option<Map<String, Value>>,
    data: &Value,
) -> Option<Value> {
    let [choice] = data["choices"].as_array()?.as_slice() else {
        return None;
    };
    let mut response = Response::new(model, response_fields);
    response.observe_chat_usage(&data["usage"]);
    let message = &choice["message"];
    if !message.is_object()
        || !message["tool_calls"].is_null()
        || !message["function_call"].is_null()
    {
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
    Some(response.response(Some(finish)))
}

pub(crate) struct StreamState {
    response: Response,
    started: bool,
    text: Option<super::responses::TextOutput>,
    finish: Option<Finish>,
    refused: bool,
}

impl StreamState {
    pub(crate) fn new(model: String, response_fields: Option<Map<String, Value>>) -> Self {
        Self {
            response: Response::new(model, response_fields),
            started: false,
            text: None,
            finish: None,
            refused: false,
        }
    }

    pub(crate) fn convert(&mut self, data: &Value) -> Option<(Vec<String>, bool)> {
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
        if !delta.is_object() || !delta["tool_calls"].is_null() || !delta["function_call"].is_null()
        {
            return None;
        }
        if delta["refusal"]
            .as_str()
            .is_some_and(|text| !text.is_empty())
        {
            self.refused = true;
        }
        if let Some(content) = delta["content"].as_str().filter(|text| !text.is_empty()) {
            let text = self.text.get_or_insert_with(|| {
                let (output, added) = self.response.start_text(String::new());
                frames.extend(added);
                output
            });
            text.text.push_str(content);
            frames.push(self.response.text_delta(&text.id, content));
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

    pub(crate) fn finish(&mut self) -> Option<(Vec<String>, bool)> {
        let finish = self.finish.take()?;
        if self.text.is_none() && matches!(finish, Finish::Complete) {
            return None;
        }
        let mut frames = Vec::new();
        if let Some(text) = self.text.take() {
            frames.extend(self.response.end_text(text, finish));
        }
        frames.push(self.response.terminal(finish));
        Some((frames, true))
    }
}
