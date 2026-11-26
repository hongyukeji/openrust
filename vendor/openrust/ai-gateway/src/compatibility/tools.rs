use std::collections::HashSet;

use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct ToolHistory<'a> {
    seen: HashSet<&'a str>,
    pending: HashSet<&'a str>,
}

impl<'a> ToolHistory<'a> {
    pub(super) fn call(&mut self, id: &'a str) -> Result<(), &'static str> {
        if !self.seen.insert(id) {
            return Err("工具调用 ID 重复");
        }
        self.pending.insert(id);
        Ok(())
    }

    pub(super) fn result(&mut self, id: &str) -> Result<(), &'static str> {
        if !self.pending.remove(id) {
            return Err("工具结果缺少对应调用或已经提交");
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<(), &'static str> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err("工具调用历史缺少对应结果")
        }
    }
}

pub(super) fn allowed_fields(value: &Value, fields: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.keys().all(|key| fields.contains(&key.as_str())))
}

pub(super) fn tool_id(value: &Value) -> Option<&str> {
    value.as_str().filter(|id| {
        !id.is_empty()
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    })
}

pub(super) fn tool_name(value: &Value) -> Option<&str> {
    tool_id(value).filter(|name| name.len() <= 64)
}

pub(super) fn arguments(value: &Value) -> Option<Value> {
    serde_json::from_str::<Value>(value.as_str()?)
        .ok()
        .filter(Value::is_object)
}

pub(super) fn tool_definition(tool: &Value, to_messages: bool) -> Result<Value, &'static str> {
    let schema_field = if to_messages {
        "parameters"
    } else {
        "input_schema"
    };
    if !allowed_fields(
        tool,
        &["type", "name", "description", schema_field, "strict"],
    ) || if to_messages {
        tool["type"] != "function"
    } else {
        !(tool["type"].is_null() || tool["type"] == "custom")
    } {
        return Err("协议转换只支持客户端函数工具");
    }
    let _name = tool_name(&tool["name"]).ok_or("工具名称无效")?;
    let schema = &tool[schema_field];
    if !schema.is_object() || !matches!(schema["type"].as_str(), Some("object")) {
        return Err("工具参数必须定义为 JSON 对象");
    }
    let mut target = if to_messages {
        json!({"name":tool["name"], "input_schema":schema})
    } else {
        json!({"type":"function", "name":tool["name"], "parameters":schema, "strict":false})
    };
    if !tool["description"].is_null() {
        if !tool["description"].is_string() {
            return Err("工具描述必须是文本");
        }
        target["description"] = tool["description"].clone();
    }
    if !tool["strict"].is_null() {
        if !tool["strict"].is_boolean() {
            return Err("工具 strict 必须是布尔值");
        }
        target["strict"] = tool["strict"].clone();
    }
    Ok(target)
}

pub(super) fn response_choice(choice: &Value, parallel: &Value) -> Result<Value, &'static str> {
    if !parallel.is_null() && !parallel.is_boolean() {
        return Err("parallel_tool_calls 必须是布尔值");
    }
    let kind = match choice.as_str() {
        None if choice.is_null() => "auto",
        Some("auto") => "auto",
        Some("none") => "none",
        Some("required") => "any",
        None if allowed_fields(choice, &["type", "name"]) && choice["type"] == "function" => "tool",
        _ => return Err("不支持该 Responses 工具选择模式"),
    };
    let mut result = json!({"type":kind});
    if kind == "tool" {
        result["name"] = tool_name(&choice["name"]).ok_or("缺少指定工具名称")?.into();
    }
    if kind != "none" && parallel == false {
        result["disable_parallel_tool_use"] = true.into();
    }
    Ok(result)
}

pub(super) fn message_choice(choice: &Value) -> Result<(Value, Option<bool>), &'static str> {
    if !allowed_fields(choice, &["type", "name", "disable_parallel_tool_use"]) {
        return Err("不支持该 Anthropic 工具选择字段");
    }
    let output = match choice["type"].as_str() {
        Some("auto") => json!("auto"),
        Some("any") => json!("required"),
        Some("none") => json!("none"),
        Some("tool") => {
            json!({"type":"function","name":tool_name(&choice["name"]).ok_or("缺少指定工具名称")?})
        }
        _ => return Err("不支持该 Anthropic 工具选择模式"),
    };
    let parallel = if choice["disable_parallel_tool_use"].is_null() {
        None
    } else {
        Some(
            !choice["disable_parallel_tool_use"]
                .as_bool()
                .ok_or("disable_parallel_tool_use 必须是布尔值")?,
        )
    };
    Ok((output, parallel))
}
