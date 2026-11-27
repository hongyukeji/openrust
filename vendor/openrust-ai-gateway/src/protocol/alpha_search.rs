use std::collections::HashSet;

use eventsource_stream::Eventsource as _;
use futures_util::StreamExt as _;
use openrust::support::Uri;
use serde_json::{Map, json};

use super::{
    Call, HeaderValue, HttpFuture, HttpResult, Protocol, Request, Response, StatusCode,
    TextEndpoint, Value, bounded_json_object, client, downstream_headers, relay, rename_model,
    resources, single_header, usage_storage_failure,
};
use crate::gateway::Model;

pub(super) struct RequestOptions {
    pub(super) query: Option<String>,
    pub(super) metadata: Option<HeaderValue>,
    session_id: Option<String>,
}

impl RequestOptions {
    pub(super) fn session(&self, scope: &str) -> Option<String> {
        self.session_id
            .as_deref()
            .map(|id| crate::session_affinity::key(&["alpha/search", scope, id]))
    }
}

pub(super) fn route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::AlphaSearch, None))
}

pub(super) fn prepare(
    request: &Request,
    data: &mut Map<String, Value>,
    model: &Model,
) -> Result<RequestOptions, &'static str> {
    if !model.allows_server_tool("web_search", &Value::Null) {
        return Err("此公开模型未授权网页搜索");
    }
    if [
        "previous_response_id",
        "conversation",
        "prompt",
        "background",
        "stream",
    ]
    .into_iter()
    .any(|field| data.get(field).is_some_and(|value| !value.is_null()))
        || data.get("input").is_some_and(resources::openai_references)
    {
        return Err("独立搜索仅支持同步无状态请求");
    }
    for field in ["store", "prompt_cache_key", "prompt_cache_retention"] {
        data.remove(field);
    }
    let metadata = single_header(request, "x-codex-turn-metadata")
        .filter(|value| value.len() <= 8192)
        .and_then(|value| HeaderValue::from_str(value).ok());
    Ok(RequestOptions {
        query: request.uri().query().map(str::to_owned),
        metadata,
        session_id: data
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned),
    })
}

pub(super) fn responses_payload(mut data: Map<String, Value>) -> Map<String, Value> {
    let mut tool = json!({"type":"web_search"});
    if let Some(settings) = data.get("settings") {
        for field in ["search_context_size", "user_location", "filters"] {
            if let Some(value) = settings.get(field).filter(|value| !value.is_null()) {
                tool[field] = value.clone();
            }
        }
        match &settings["external_web_access"] {
            Value::Bool(value) => tool["external_web_access"] = json!(value),
            Value::String(value) if matches!(value.as_str(), "live" | "cached" | "indexed") => {
                tool["external_web_access"] = json!(value == "live");
            }
            _ => {}
        }
    }
    let mut input = String::from(
        "Perform the Codex web search described below. Use web search for current information. Return concise, source-backed results.",
    );
    for (field, label, limit) in [
        ("commands", "Commands", 12_000),
        ("settings", "Search settings", 4_000),
        ("input", "Recent input", 8_000),
    ] {
        if let Some(value) = data.remove(field).filter(|value| !value.is_null()) {
            let value = value.to_string();
            input.push_str("\n\n");
            input.push_str(label);
            input.push_str(":\n");
            input.push_str(&value[..value.floor_char_boundary(limit.min(value.len()))]);
        }
    }
    let mut request = Map::from_iter([
        ("model".into(), data.remove("model").unwrap_or(Value::Null)),
        (
            "input".into(),
            json!([{"role":"user","content":[{"type":"input_text","text":input}]}]),
        ),
        ("tools".into(), json!([tool])),
        ("store".into(), json!(false)),
        ("stream".into(), json!(true)),
    ]);
    if let Some(limit) = data.remove("max_output_tokens") {
        request.insert("max_output_tokens".into(), limit);
    }
    request
}

pub(super) async fn respond(upstream: client::Response, name: &str, mut call: Call) -> HttpResult {
    let status = upstream.status();
    let headers = downstream_headers(upstream.headers());
    let data = bounded_json_object(upstream, 4 * 1024 * 1024)
        .await
        .map(Value::Object);
    call.release().await;
    if let Some(data) = data.as_ref() {
        call.observe_usage(data);
    }
    let Some(mut data) = data.filter(|values| {
        values.get("output").is_some_and(Value::is_string)
            && values
                .get("results")
                .is_none_or(|value| value.is_null() || value.is_array())
            && values
                .get("encrypted_output")
                .is_none_or(|value| value.is_null() || value.is_string())
    }) else {
        call.outcome("invalid_upstream_response");
        if !call.settle().await {
            return usage_storage_failure(Protocol::OpenAi, &mut call).response;
        }
        return Protocol::OpenAi.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回完整的独立搜索结果",
        );
    };
    call.observe_standalone_search();
    if !call.complete().await {
        return usage_storage_failure(Protocol::OpenAi, &mut call).response;
    }
    rename_model(&mut data, name);
    Response::json(status, data).map(|response| response.with_headers(headers))
}

pub(super) async fn respond_responses(upstream: client::Response, mut call: Call) -> HttpResult {
    let headers = downstream_headers(upstream.headers());
    let output = match read_responses(upstream, &mut call).await {
        Ok(output) => output,
        Err(outcome) => {
            call.outcome(outcome);
            call.release().await;
            if !call.settle().await || outcome == "usage_storage_error" {
                return usage_storage_failure(Protocol::OpenAi, &mut call).response;
            }
            return Protocol::OpenAi.error(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "搜索上游未完成 Responses 请求",
            );
        }
    };
    call.release().await;
    call.observe_standalone_search();
    if !call.complete().await {
        return usage_storage_failure(Protocol::OpenAi, &mut call).response;
    }
    Response::json(StatusCode::OK, output).map(|response| response.with_headers(headers))
}

async fn read_responses(
    upstream: client::Response,
    call: &mut Call,
) -> Result<Value, &'static str> {
    const LIMIT: usize = 4 * 1024 * 1024;
    let mut size = 0;
    let stream = upstream.into_stream().map_err(|_| "stream_error")?;
    let mut events = stream
        .map(|chunk| {
            let chunk = chunk?;
            if chunk.len() > LIMIT - size {
                return Err(client::Error::BodyTooLarge);
            }
            size += chunk.len();
            Ok(chunk)
        })
        .eventsource();
    let mut output = String::new();
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    while let Some(frame) = events.next().await {
        let frame = frame.map_err(|_| "stream_error")?;
        if frame.data == "[DONE]" {
            break;
        }
        let event: Value = serde_json::from_str(&frame.data).map_err(|_| "stream_error")?;
        let kind = event["type"].as_str().unwrap_or(frame.event.as_str());
        let response = &event["response"];
        let observed = call.observe_usage(response) | call.observe_usage(&event);
        if observed && !call.persist_observation().await {
            return Err("usage_storage_error");
        }
        match kind {
            "response.output_text.delta" => {
                if let Some(delta) = event["delta"].as_str() {
                    output.push_str(delta);
                }
            }
            "response.output_text.annotation.added" => {
                collect_citations(&event["annotation"], &mut results, &mut seen);
            }
            "response.output_item.done" => {
                collect_citations(&event["item"], &mut results, &mut seen);
            }
            "error" | "response.failed" | "response.incomplete" => return Err("incomplete_stream"),
            "response.completed" => {
                if !response.is_object()
                    || response
                        .get("status")
                        .is_some_and(|status| status != "completed")
                {
                    return Err("incomplete_stream");
                }
                call.observe_final_cost(response);
                collect_citations(&response["output"], &mut results, &mut seen);
                if output.trim().is_empty() {
                    output = completed_text(response);
                }
                return Ok(if results.is_empty() {
                    json!({"output": output})
                } else {
                    json!({"output": output,"results":results})
                });
            }
            _ => {}
        }
    }
    Err("incomplete_stream")
}

fn completed_text(response: &Value) -> String {
    response["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == "message")
        .flat_map(|item| item["content"].as_array().into_iter().flatten())
        .filter(|content| content["type"] == "output_text")
        .filter_map(|content| content["text"].as_str())
        .collect()
}

fn collect_citations(value: &Value, results: &mut Vec<Value>, seen: &mut HashSet<String>) {
    match value {
        Value::Object(fields) if value["type"] == "url_citation" => {
            let Some(url) = value["url"].as_str().map(str::trim) else {
                return;
            };
            let Ok(uri) = Uri::of(url) else {
                return;
            };
            if !matches!(uri.scheme(), Some("https" | "http"))
                || uri.host().is_none_or(str::is_empty)
                || uri.user_info().is_some()
                || !seen.insert(uri.value().to_owned())
            {
                return;
            }
            let mut result = json!({"type":"text_result","ref_id":format!("turn0search{}",results.len()),"url":url});
            if let Some(title) = fields
                .get("title")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
            {
                result["title"] = title.into();
            }
            results.push(result);
        }
        Value::Object(fields) => fields
            .values()
            .for_each(|child| collect_citations(child, results, seen)),
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_citations(item, results, seen)),
        _ => {}
    }
}
