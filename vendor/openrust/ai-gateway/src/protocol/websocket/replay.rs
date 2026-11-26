use serde_json::{Map, Value, json};

const MAX_BYTES: usize = 512 * 1024;
const MAX_ITEMS: usize = 256;

pub(super) struct Checkpoint {
    lane: Option<String>,
    id: String,
    model: String,
    history: Vec<Value>,
}

impl Checkpoint {
    pub(super) fn from_complete(
        response: &Value,
        lane: Option<String>,
        model: String,
        mut input: Vec<Value>,
    ) -> Option<Self> {
        let id = response["id"].as_str().filter(|id| valid_id(id))?;
        let output = response["output"].as_array()?;
        if output.is_empty() || input.len() + output.len() > MAX_ITEMS {
            return None;
        }
        input.extend(output.iter().map(replay_item).collect::<Option<Vec<_>>>()?);
        bounded(&input).then(|| Self {
            lane,
            id: id.into(),
            model,
            history: input,
        })
    }

    pub(super) fn resume(
        &self,
        previous: &str,
        lane: Option<&str>,
        body: &mut Map<String, Value>,
    ) -> Result<(), &'static str> {
        if previous != self.id || self.lane.as_deref() != lane {
            return Err("previous_response_not_found");
        }
        match body.get("model") {
            None | Some(Value::Null) => {
                body.insert("model".into(), self.model.clone().into());
            }
            Some(Value::String(model)) if model == &self.model => {}
            _ => return Err("previous_response_not_found"),
        }
        let next = input_items(body.get("input")).ok_or("unsupported_continuation_input")?;
        if self.history.len() + next.len() > MAX_ITEMS {
            return Err("continuation_too_large");
        }
        let mut full = self.history.clone();
        full.extend(next);
        if !bounded(&full) {
            return Err("continuation_too_large");
        }
        body.insert("input".into(), Value::Array(full));
        Ok(())
    }
}

pub(super) fn replay_input(input: Option<&Value>) -> Option<Vec<Value>> {
    let input = input_items(input)?;
    bounded(&input).then_some(input)
}

fn input_items(input: Option<&Value>) -> Option<Vec<Value>> {
    match input {
        None | Some(Value::Null) => Some(Vec::new()),
        Some(Value::String(text)) => Some(vec![json!({"role":"user","content":text})]),
        Some(Value::Array(items)) if items.len() <= MAX_ITEMS => {
            items.iter().map(replay_item).collect()
        }
        _ => None,
    }
}

fn replay_item(item: &Value) -> Option<Value> {
    match item.get("type").unwrap_or(&Value::Null) {
        Value::Null => replay_message(item),
        Value::String(kind) => match kind.as_str() {
            "message" => replay_message(item),
            "function_call" => {
                let call_id = item["call_id"].as_str().filter(|id| valid_id(id))?;
                let name = item["name"].as_str().filter(|name| valid_id(name))?;
                if item.get("namespace").is_some_and(|value| !value.is_null())
                    || item["status"]
                        .as_str()
                        .is_some_and(|status| status != "completed")
                {
                    return None;
                }
                let arguments = item["arguments"].as_str()?;
                serde_json::from_str::<Map<String, Value>>(arguments).ok()?;
                Some(json!({
                    "type":"function_call", "call_id":call_id, "name":name,
                    "arguments":arguments, "status":"completed"
                }))
            }
            "function_call_output" => {
                let call_id = item["call_id"].as_str().filter(|id| valid_id(id))?;
                let output = item["output"].as_str()?;
                Some(json!({"type":"function_call_output","call_id":call_id,"output":output}))
            }
            _ => None,
        },
        _ => None,
    }
}

fn replay_message(item: &Value) -> Option<Value> {
    let role = item["role"].as_str()?;
    if !matches!(role, "user" | "assistant" | "system" | "developer") {
        return None;
    }
    let content = match &item["content"] {
        Value::String(text) => Value::String(text.clone()),
        Value::Array(parts) if role == "assistant" => {
            let mut text = String::new();
            for part in parts {
                if !matches!(part["type"].as_str(), Some("output_text" | "input_text")) {
                    return None;
                }
                text.push_str(part["text"].as_str()?);
            }
            Value::String(text)
        }
        Value::Array(parts) => {
            let content = parts
                .iter()
                .map(|part| {
                    let kind = part["type"].as_str()?;
                    if kind != "input_text" {
                        return None;
                    }
                    Some(json!({"type":kind,"text":part["text"].as_str()?}))
                })
                .collect::<Option<Vec<_>>>()?;
            Value::Array(content)
        }
        _ => return None,
    };
    Some(json!({"type":"message","role":role,"content":content}))
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn bounded(input: &[Value]) -> bool {
    serde_json::to_vec(input).is_ok_and(|bytes| bytes.len() <= MAX_BYTES)
}
