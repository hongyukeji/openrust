use openrust_ai::TextEndpoint;
use serde_json::{Map, Value};

pub(super) fn openai_references(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(openai_references),
        Value::Object(item) => {
            let resource = match value["type"].as_str() {
                Some("item_reference") => true,
                Some("input_file" | "input_image") => present(&value["file_id"]),
                Some("file") => present(&value["file"]["file_id"]),
                _ => false,
            };
            resource
                || (item.contains_key("role") && present(&value["audio"]["id"]))
                || ["content", "output"]
                    .iter()
                    .any(|field| item.get(*field).is_some_and(openai_references))
        }
        _ => false,
    }
}

pub(super) fn anthropic_references(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(anthropic_references),
        Value::Object(item) => {
            value["type"] == "container_upload"
                || value["source"]["type"] == "file"
                || item.get("content").is_some_and(anthropic_references)
        }
        _ => false,
    }
}

pub(super) fn image_file_references(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(image_file_references),
        Value::Object(item) => {
            present(&value["file_id"]) || item.values().any(image_file_references)
        }
        _ => false,
    }
}

pub(super) fn present_field(data: &Map<String, Value>, field: &str) -> bool {
    data.get(field).is_some_and(present)
}

pub(super) fn gemini_file_references(data: &Map<String, Value>) -> bool {
    fn contains_reference(value: &Value) -> bool {
        match value {
            Value::Array(items) => items.iter().any(contains_reference),
            Value::Object(fields) => gemini_file_references(fields),
            _ => false,
        }
    }

    ["fileData", "file_data", "cachedContent", "cached_content"]
        .iter()
        .any(|field| present_field(data, field))
        || [
            "content",
            "contents",
            "parts",
            "requests",
            "systemInstruction",
            "system_instruction",
            "functionResponse",
            "function_response",
        ]
        .iter()
        .any(|field| data.get(*field).is_some_and(contains_reference))
}

pub(super) fn gemini_embedding_model_matches(
    data: &Map<String, Value>,
    alias: &str,
    endpoint: TextEndpoint,
) -> bool {
    let permitted = |data: &Map<String, Value>| {
        data.get("model").is_none_or(|value| {
            value
                .as_str()
                .is_some_and(|model| model.strip_prefix("models/").unwrap_or(model) == alias)
        })
    };
    permitted(data)
        && (endpoint != TextEndpoint::GeminiBatchEmbedContents
            || data
                .get("requests")
                .and_then(Value::as_array)
                .is_some_and(|requests| {
                    !requests.is_empty()
                        && requests
                            .iter()
                            .all(|request| request.as_object().is_some_and(permitted))
                }))
}

fn present(value: &Value) -> bool {
    !value.is_null()
}

pub(super) fn gemini_function_tools(tools: Option<&Value>) -> bool {
    tools.filter(|tools| !tools.is_null()).is_none_or(|tools| {
        tools.as_array().is_some_and(|tools| {
            tools.iter().all(|tool| {
                tool.as_object().is_some_and(|tool| {
                    tool.len() == 1
                        && tool.keys().all(|name| {
                            matches!(
                                name.as_str(),
                                "functionDeclarations" | "function_declarations"
                            )
                        })
                })
            })
        })
    })
}
