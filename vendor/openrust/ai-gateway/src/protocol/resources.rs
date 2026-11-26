use serde_json::Value;

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

fn present(value: &Value) -> bool {
    !value.is_null()
}
