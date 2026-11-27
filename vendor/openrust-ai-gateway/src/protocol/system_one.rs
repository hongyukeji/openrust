use std::{collections::BTreeMap, fmt};

use futures_util::StreamExt as _;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use serde_json::Map;

use super::{
    Call, HttpFuture, HttpResult, Protocol, Request, Response, StatusCode, TextEndpoint, Value,
    client, downstream_headers, json, relay, usage_storage_failure,
};

pub(super) fn route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::SystemOne, None))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    model: String,
    state: Description,
    #[serde(deserialize_with = "unique_questions")]
    questions: BTreeMap<String, Question>,
    stream: Option<bool>,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Description {
    Text(String),
    Object(Map<String, Value>),
    List(Vec<Value>),
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Question {
    Noul {
        #[serde(skip_serializing_if = "Option::is_none")]
        instructions: Option<Description>,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        #[serde(skip_serializing_if = "Option::is_none")]
        instructions: Option<Description>,
        criteria: BTreeMap<String, Option<Description>>,
    },
    Score {
        #[serde(skip_serializing_if = "Option::is_none")]
        instructions: Option<Description>,
        criteria: Vec<Description>,
    },
}

#[derive(Deserialize, Serialize)]
struct NoulCriteria {
    #[serde(rename = "true", skip_serializing_if = "Option::is_none")]
    yes: Option<Description>,
    #[serde(rename = "false", skip_serializing_if = "Option::is_none")]
    no: Option<Description>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn unique_questions<'de, D>(deserializer: D) -> Result<BTreeMap<String, Question>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Questions;
    impl<'de> Visitor<'de> for Questions {
        type Value = BTreeMap<String, Question>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("问题名称唯一的 JSON 对象")
        }

        fn visit_map<M: MapAccess<'de>>(self, mut entries: M) -> Result<Self::Value, M::Error> {
            let mut questions = BTreeMap::new();
            while let Some((name, question)) = entries.next_entry()? {
                if questions.insert(name, question).is_some() {
                    return Err(de::Error::custom("问题名称重复"));
                }
            }
            Ok(questions)
        }
    }
    deserializer.deserialize_map(Questions)
}

pub(super) fn decode(bytes: &[u8]) -> Result<Map<String, Value>, &'static str> {
    const INVALID: &str = "System One 正文须包含有效的模型、状态和命名问题";
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) != Some(&b'{') {
        return Err(INVALID);
    }
    let input: Input = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    if input.stream == Some(true) {
        return Err("System One 仅支持同步响应");
    }
    if input.model.trim().is_empty()
        || input.questions.is_empty()
        || input.questions.values().any(
            |question| matches!(question, Question::Score { criteria, .. } if criteria.is_empty()),
        )
    {
        return Err(INVALID);
    }
    Ok(Map::from_iter([
        ("model".into(), input.model.into()),
        ("state".into(), json!(input.state)),
        ("questions".into(), json!(input.questions)),
    ]))
}

pub(super) async fn respond(upstream: client::Response, name: &str, mut call: Call) -> HttpResult {
    let status = upstream.status();
    let headers = downstream_headers(upstream.headers());
    let data = response_object(upstream).await;
    call.release().await;
    let Some(mut data) = data else {
        call.outcome("invalid_upstream_response");
        if !call.settle().await {
            return usage_storage_failure(Protocol::OpenAi, &mut call).response;
        }
        return Protocol::OpenAi.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回有效的 System One 响应对象",
        );
    };
    let counts = data.get("usage");
    let input = counts.and_then(|usage| reported_tokens(&usage["input_tokens"]));
    let output = counts.and_then(|usage| reported_tokens(&usage["output_tokens"]));
    call.observe_usage(&json!({
        "model": data.get("model"),
        "usage": {"input_tokens":input, "output_tokens":output}
    }));
    data.insert("model".into(), name.into());
    if let Some(routing) = data.get_mut("routing").and_then(Value::as_object_mut)
        && routing.contains_key("model")
    {
        routing.insert("model".into(), name.into());
    }
    if !call.complete().await {
        return usage_storage_failure(Protocol::OpenAi, &mut call).response;
    }
    Response::json(status, data).map(|response| response.with_headers(headers))
}

async fn response_object(upstream: client::Response) -> Option<Map<String, Value>> {
    const LIMIT: usize = 4 * 1024 * 1024;
    let mut stream = upstream.into_stream().ok()?;
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if chunk.len() > LIMIT - body.len() {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).ok()
}

fn reported_tokens(value: &Value) -> Option<u64> {
    const MAX: u64 = 1 << 40;
    let count = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())?;
    (count.is_finite() && count >= 0.0).then(|| count.min(MAX as f64).round() as u64)
}
