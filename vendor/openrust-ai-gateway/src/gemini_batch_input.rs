use serde_json::{Map, Value, json};

fn input_key(batch: &Value) -> Option<&'static str> {
    match (batch.get("inputConfig"), batch.get("input_config")) {
        (Some(_), None) => Some("inputConfig"),
        (None, Some(_)) => Some("input_config"),
        _ => None,
    }
}

pub(crate) fn requests(batch: &Value) -> Option<&[Value]> {
    let input = batch.get(input_key(batch)?)?.as_object()?;
    let requests = input.get("requests")?.as_object()?;
    if input.len() != 1 || requests.len() != 1 {
        return None;
    }
    requests.get("requests")?.as_array().map(Vec::as_slice)
}

pub(crate) fn requests_mut(batch: &mut Value) -> Option<&mut [Value]> {
    let key = input_key(batch)?;
    batch[key]["requests"]["requests"]
        .as_array_mut()
        .map(Vec::as_mut_slice)
}

pub(crate) fn text_output(batch: &Value) -> bool {
    requests(batch).is_some_and(|items| {
        !items.is_empty()
            && items.iter().all(|item| {
                let Some(request) = item["request"].as_object() else {
                    return false;
                };
                match (
                    request.get("generationConfig"),
                    request.get("generation_config"),
                ) {
                    (None, None) => true,
                    (Some(config), None) | (None, Some(config)) => {
                        config.as_object().is_some_and(text_config)
                    }
                    _ => false,
                }
            })
    })
}

fn text_config(config: &Map<String, Value>) -> bool {
    let modalities = single(config, "responseModalities", "response_modalities");
    let mime = single(config, "responseMimeType", "response_mime_type");
    let format = single(config, "responseFormat", "response_format");
    (matches!(modalities, Ok(None))
        || matches!(modalities, Ok(Some(modalities)) if modalities.as_array().is_some_and(|items| items.iter().all(|item| item == "TEXT"))))
        && (matches!(mime, Ok(None))
            || matches!(mime, Ok(Some(Value::String(mime))) if matches!(mime.as_str(), "text/plain" | "application/json" | "text/x.enum")))
        && (matches!(format, Ok(None))
            || matches!(format, Ok(Some(format)) if format.as_object().is_some_and(|fields| fields.keys().all(|key| key == "text"))))
}

fn single<'a>(
    object: &'a Map<String, Value>,
    camel: &str,
    snake: &str,
) -> Result<Option<&'a Value>, &'static str> {
    match (object.get(camel), object.get(snake)) {
        (Some(_), Some(_)) => Err("批任务请只设置一种 Gemini 字段拼写"),
        (Some(value), None) | (None, Some(value)) if !value.is_null() => Ok(Some(value)),
        _ => Ok(None),
    }
}

pub(crate) fn limit_output(batch: &mut Value, max: i64) -> Result<i64, &'static str> {
    if !text_output(batch) {
        return Err("Gemini 生成批次的上限计算只支持文本输出");
    }
    requests_mut(batch)
        .ok_or("Gemini 生成批次需要内联请求")?
        .iter_mut()
        .try_fold(0_i64, |total, item| {
            let request = item["request"]
                .as_object_mut()
                .ok_or("Gemini 批任务项无效")?;
            let key = if request.contains_key("generation_config") {
                "generation_config"
            } else {
                "generationConfig"
            };
            let config = request
                .entry(key)
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .ok_or("Gemini 生成配置无效")?;
            let candidates =
                positive(single(config, "candidateCount", "candidate_count")?)?.unwrap_or(1);
            let requested = positive(single(config, "maxOutputTokens", "max_output_tokens")?)?;
            let max_each = (max / candidates).min(i64::from(i32::MAX));
            if max_each < 1 {
                return Err("Gemini 候选数量超出批任务单项输出上限");
            }
            let limit = requested.unwrap_or(max_each).min(max_each);
            let key = if config.contains_key("max_output_tokens") {
                "max_output_tokens"
            } else {
                "maxOutputTokens"
            };
            config.insert(key.into(), json!(limit));
            total
                .checked_add(limit * candidates)
                .ok_or("Gemini 批任务输出上限超出可用范围")
        })
}

fn positive(value: Option<&Value>) -> Result<Option<i64>, &'static str> {
    value
        .map(|value| {
            value
                .as_i64()
                .filter(|value| (1..=i64::from(i32::MAX)).contains(value))
                .ok_or("Gemini 候选数量和输出上限须为正整数")
        })
        .transpose()
}
