use serde_json::Map;

use super::{
    Call, HeaderValue, HttpFuture, HttpResult, Protocol, Request, Response, StatusCode,
    TextEndpoint, Value, bounded_json_object, client, downstream_headers, relay, rename_model,
    resources, single_header, usage_storage_failure,
};
use crate::gateway::Model;

pub(super) struct RequestOptions {
    pub(super) query: Option<String>,
    pub(super) metadata: Option<HeaderValue>,
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
    })
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
