use std::collections::{HashMap, HashSet};

use aws_sdk_bedrockruntime::types::{Tool as AwsTool, ToolConfiguration};
use serde_json::{Value, json};

use crate::{FinishReason, ToolCall, ToolChoice, ToolDefinition, ToolResult};

use super::{BedrockMessage, chat::text_parts, tools};

pub(super) struct ChatTools {
    pub(super) config: Option<ToolConfiguration>,
    names: HashSet<String>,
    choice: ToolChoice,
    parallel: bool,
    history: HashSet<String>,
}

impl ChatTools {
    pub(super) fn parse(body: &Value) -> Result<Self, &'static str> {
        let mut definitions = Vec::new();
        let mut options = Vec::new();
        let mut names = HashSet::new();
        if !body["tools"].is_null() {
            for tool in body["tools"].as_array().ok_or("tools 须为数组")? {
                if tool["type"] != "function" || !has_only(tool, &["type", "function"]) {
                    return Err("Bedrock Chat 只支持客户端 function 工具");
                }
                let function = &tool["function"];
                let name = name(&function["name"])?;
                if !has_only(function, &["name", "description", "parameters", "strict"])
                    || !names.insert(name.to_owned())
                {
                    return Err("Bedrock Chat 工具定义或名称重复");
                }
                let description = match &function["description"] {
                    Value::Null => String::new(),
                    value => value.as_str().ok_or("工具描述须为文本")?.to_owned(),
                };
                let schema = match &function["parameters"] {
                    Value::Null => json!({"type":"object","properties":{}}),
                    value if value.is_object() && value["type"] == "object" => value.clone(),
                    _ => return Err("工具参数须为对象 Schema"),
                };
                let strict = flag(&function["strict"])?;
                definitions.push(ToolDefinition {
                    name: name.to_owned(),
                    description,
                    schema,
                });
                options.push(strict);
            }
        }
        let choice = match &body["tool_choice"] {
            Value::Null => ToolChoice::Auto,
            Value::String(choice) if choice == "auto" => ToolChoice::Auto,
            Value::String(choice) if choice == "none" => ToolChoice::None,
            Value::String(choice) if choice == "required" && !names.is_empty() => {
                ToolChoice::Required
            }
            value
                if value["type"] == "function"
                    && has_only(value, &["type", "function"])
                    && has_only(&value["function"], &["name"]) =>
            {
                let selected = name(&value["function"]["name"])?;
                if !names.contains(selected) {
                    return Err("tool_choice 须引用已声明的工具");
                }
                ToolChoice::Tool(selected.to_owned())
            }
            _ => return Err("Bedrock Chat tool_choice 无效"),
        };
        let parallel = flag(&body["parallel_tool_calls"])?.unwrap_or(true);
        let mut config = tools::configuration(&definitions, Some(&choice))
            .map_err(|_| "无法构造 Bedrock 工具配置")?;
        if let Some(config) = config.as_mut() {
            for (tool, strict) in config.tools.iter_mut().zip(options) {
                if let AwsTool::ToolSpec(spec) = tool {
                    spec.strict = strict;
                    if spec.description.as_deref() == Some("") {
                        spec.description = None;
                    }
                }
            }
        }
        Ok(Self {
            config,
            names,
            choice,
            parallel,
            history: HashSet::new(),
        })
    }

    pub(super) fn history_calls(&mut self, value: &Value) -> Result<Vec<ToolCall>, &'static str> {
        if self.config.is_none() {
            return Err("Bedrock Chat 回放工具历史需要启用 tools");
        }
        let calls = value
            .as_array()
            .filter(|calls| !calls.is_empty())
            .ok_or("tool_calls 须为非空数组")?;
        calls
            .iter()
            .map(|call| {
                let id = id(&call["id"])?;
                let function = &call["function"];
                let name = name(&function["name"])?;
                if call["type"] != "function"
                    || !has_only(call, &["type", "id", "function"])
                    || !has_only(function, &["name", "arguments"])
                    || !self.history.insert(id.to_owned())
                {
                    return Err("Bedrock Chat 工具历史格式无效或 ID 重复");
                }
                let arguments = function["arguments"]
                    .as_str()
                    .and_then(|value| serde_json::from_str(value).ok())
                    .ok_or("工具 arguments 须为 JSON 对象字符串")?;
                Ok(ToolCall {
                    id: id.into(),
                    name: name.into(),
                    arguments,
                    result_id: None,
                })
            })
            .collect()
    }

    pub(super) fn accepts(&self, reason: FinishReason, calls: &[ToolCall]) -> bool {
        if super::validate_tool_stop(reason, calls).is_err() {
            return false;
        }
        if calls.is_empty() {
            return super::chat::chat_reason(reason)
                && (reason == FinishReason::ContentFilter
                    || self.choice.validate_calls(calls).is_ok());
        }
        if self.choice.validate_calls(calls).is_err() || (!self.parallel && calls.len() > 1) {
            return false;
        }
        let mut seen = HashSet::new();
        calls.iter().all(|call| {
            valid_id(&call.id)
                && !self.history.contains(&call.id)
                && seen.insert(call.id.as_str())
                && self.names.contains(&call.name)
        })
    }
}

#[derive(Default)]
pub(super) struct ToolHistory {
    pending: HashMap<String, String>,
    results: Vec<ToolResult>,
}

impl ToolHistory {
    pub(super) fn begin(&mut self, calls: &[ToolCall]) {
        self.pending.extend(
            calls
                .iter()
                .map(|call| (call.id.clone(), call.name.clone())),
        );
    }

    pub(super) fn accept(
        &mut self,
        message: &Value,
    ) -> Result<Option<BedrockMessage>, &'static str> {
        if !has_only(message, &["role", "tool_call_id", "content"]) {
            return Err("Bedrock Chat 工具结果只支持文本");
        }
        let id = id(&message["tool_call_id"])?;
        let name = self
            .pending
            .remove(id)
            .ok_or("工具结果 ID 与上轮调用不匹配")?;
        let content = match &message["content"] {
            Value::String(text) => text.clone(),
            value => text_parts(value)?.concat(),
        };
        self.results.push(ToolResult::new(id, name, content));
        if !self.pending.is_empty() {
            return Ok(None);
        }
        let message = tools::results(&self.results).map_err(|_| "无法构造 Bedrock 工具结果")?;
        self.results.clear();
        Ok(Some(message))
    }

    pub(super) fn require_finished(&self) -> Result<(), &'static str> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err("提交下一轮前须提供所有工具调用的结果")
        }
    }
}

pub(super) fn wire_call(call: &ToolCall) -> Value {
    json!({"id":call.id,"type":"function","function":{
        "name":call.name,"arguments":Value::Object(call.arguments.clone()).to_string(),
    }})
}

pub(super) fn has_only(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|value| value.keys().all(|key| keys.contains(&key.as_str())))
}

fn flag(value: &Value) -> Result<Option<bool>, &'static str> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(value) => Ok(Some(*value)),
        _ => Err("Bedrock Chat 工具选项须为布尔值"),
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || b"_.:-".contains(&value))
}

fn id(value: &Value) -> Result<&str, &'static str> {
    value
        .as_str()
        .filter(|id| valid_id(id))
        .ok_or("Bedrock 工具调用 ID 无效")
}

fn name(value: &Value) -> Result<&str, &'static str> {
    value
        .as_str()
        .filter(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || b"_-".contains(&value))
        })
        .ok_or("Bedrock 工具名称无效")
}
