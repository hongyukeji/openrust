use serde_json::{Map, Value, json};

use super::responses::Frame;
use super::tools::{
    ToolHistory, allowed_fields, arguments, response_choice, tool_definition, tool_id, tool_name,
};

pub(super) fn configure(
    request: &Map<String, Value>,
    target: &mut Map<String, Value>,
) -> Result<(), &'static str> {
    let choice = request.get("tool_choice").unwrap_or(&Value::Null);
    let parallel = request.get("parallel_tool_calls").unwrap_or(&Value::Null);
    let parsed_choice = response_choice(choice, parallel)?;
    let definitions = request
        .get("tools")
        .filter(|value| !value.is_null())
        .map(|value| {
            value
                .as_array()
                .map(Vec::as_slice)
                .ok_or("tools 必须是数组")
        })
        .transpose()?
        .unwrap_or_default();
    if definitions.is_empty() {
        if matches!(parsed_choice["type"].as_str(), Some("tool" | "any")) {
            return Err("必须配置可供选择的函数工具");
        }
        return Ok(());
    }
    let definitions = definitions
        .iter()
        .map(|definition| -> Result<Value, &'static str> {
            // 共用 Responses 函数的模式与字段校验，Chat 将函数元数据包装在 function 下。
            tool_definition(definition, true)?;
            let mut function = definition.as_object().expect("已验证工具字段").clone();
            function.remove("type");
            function.retain(|_, value| !value.is_null());
            Ok(json!({"type":"function","function":function}))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if !choice.is_null() {
        let choice = if choice.is_object() {
            json!({"type":"function","function":{"name":choice["name"]}})
        } else {
            choice.clone()
        };
        target.insert("tool_choice".into(), choice);
    }
    if !parallel.is_null() {
        target.insert("parallel_tool_calls".into(), parallel.clone());
    }
    target.insert("tools".into(), definitions.into());
    Ok(())
}

#[derive(Default)]
pub(super) struct History<'a> {
    tools: ToolHistory<'a>,
    reading_results: bool,
}

impl<'a> History<'a> {
    pub(super) fn append(
        &mut self,
        item: &'a Value,
        messages: &mut Vec<Value>,
    ) -> Result<bool, &'static str> {
        match item["type"].as_str() {
            Some("function_call") => {
                if self.reading_results
                    || !allowed_fields(
                        item,
                        &["type", "id", "call_id", "name", "arguments", "status"],
                    )
                    || !(item["status"].is_null() || item["status"] == "completed")
                {
                    return Err("工具调用必须在本组工具结果之前完成");
                }
                let id = tool_id(&item["call_id"]).ok_or("缺少有效的工具调用 ID")?;
                let name = tool_name(&item["name"]).ok_or("工具名称无效")?;
                arguments(&item["arguments"]).ok_or("工具参数必须是 JSON 对象")?;
                self.tools.call(id)?;
                let call = json!({"id":id,"type":"function","function":{"name":name,"arguments":item["arguments"]}});
                if let Some(previous) = messages
                    .last_mut()
                    .filter(|message| message["role"] == "assistant")
                {
                    if previous["tool_calls"].is_null() {
                        previous["tool_calls"] = json!([]);
                    }
                    previous["tool_calls"]
                        .as_array_mut()
                        .expect("已构造工具列表")
                        .push(call);
                } else {
                    messages.push(json!({"role":"assistant","content":null,"tool_calls":[call]}));
                }
                Ok(true)
            }
            Some("function_call_output") => {
                if !allowed_fields(item, &["type", "id", "call_id", "output", "status"])
                    || !(item["status"].is_null() || item["status"] == "completed")
                {
                    return Err("工具结果字段或状态不受协议转换支持");
                }
                let id = tool_id(&item["call_id"]).ok_or("缺少有效的工具调用 ID")?;
                let output = item["output"].as_str().ok_or("工具结果必须是文本")?;
                self.tools.result(id)?;
                self.reading_results = self.tools.has_pending();
                messages.push(json!({"role":"tool","tool_call_id":id,"content":output}));
                Ok(true)
            }
            _ if self.tools.has_pending() => Err("后续消息之前必须提供全部工具结果"),
            _ => Ok(false),
        }
    }

    pub(super) fn finish(self) -> Result<(), &'static str> {
        self.tools.finish()
    }
}

pub(super) struct Call<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub arguments: &'a str,
}

pub(super) fn buffered(value: &Value) -> Option<Vec<Call<'_>>> {
    let mut seen = std::collections::HashSet::new();
    value
        .as_array()?
        .iter()
        .map(|call| {
            if call["type"] != "function" {
                return None;
            }
            let function = &call["function"];
            let id = tool_id(&call["id"])?;
            let name = tool_name(&function["name"])?;
            arguments(&function["arguments"])?;
            if !seen.insert(id) {
                return None;
            }
            Some(Call {
                id,
                name,
                arguments: function["arguments"].as_str()?,
            })
        })
        .collect()
}

struct ToolOutput {
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub(super) struct StreamTools {
    outputs: Vec<ToolOutput>,
}

impl StreamTools {
    pub(super) fn is_empty(&self) -> bool {
        self.outputs.is_empty()
    }

    pub(super) fn delta(
        &mut self,
        response: &mut super::responses::Response,
        calls: &Value,
        offset: usize,
    ) -> Option<Vec<Frame>> {
        let mut frames = Vec::new();
        let calls = calls.as_array()?;
        for call in calls {
            let index = usize::try_from(call["index"].as_u64()?).ok()?;
            let output_index = offset.checked_add(index)?;
            let function = &call["function"];
            if !call["type"].is_null() && call["type"] != "function" {
                return None;
            }
            if index == self.outputs.len() {
                let call_id = tool_id(&call["id"])?;
                let name = tool_name(&function["name"])?;
                if self.outputs.iter().any(|tool| tool.call_id == call_id) {
                    return None;
                }
                let (item_id, frame) = response.start_function(output_index, call_id, name);
                frames.push(frame);
                self.outputs.push(ToolOutput {
                    item_id,
                    call_id: call_id.into(),
                    name: name.into(),
                    arguments: String::new(),
                });
            }
            let tool = self.outputs.get_mut(index)?;
            if (!call["id"].is_null() && call["id"] != tool.call_id)
                || (!function["name"].is_null() && function["name"] != tool.name)
            {
                return None;
            }
            if !function["arguments"].is_null() {
                let delta = function["arguments"].as_str()?;
                if !delta.is_empty() {
                    tool.arguments.push_str(delta);
                    frames.push(response.function_delta(output_index, &tool.item_id, delta));
                }
            }
        }
        Some(frames)
    }

    pub(super) fn finish(
        &mut self,
        response: &mut super::responses::Response,
    ) -> Option<Vec<Frame>> {
        for tool in &self.outputs {
            serde_json::from_str::<Map<String, Value>>(&tool.arguments).ok()?;
        }
        let mut frames = Vec::new();
        for tool in std::mem::take(&mut self.outputs) {
            frames.extend(response.end_function(
                &tool.item_id,
                &tool.call_id,
                &tool.name,
                &tool.arguments,
            ));
        }
        Some(frames)
    }
}
