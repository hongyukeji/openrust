use std::collections::BTreeMap;

use openrust::support::Uri;
use serde::Deserialize;
use serde_json::Map;

use super::*;
use crate::gateway::Model;

pub(super) fn web(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::XaiWebSearch, None))
}

pub(super) fn x(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(relay(request, TextEndpoint::XaiXSearch, None))
}

#[derive(Deserialize)]
struct SearchInput {
    query: Option<String>,
    input: Option<String>,
    max_results: Option<i64>,
    allowed_x_handles: Option<Vec<String>>,
    excluded_x_handles: Option<Vec<String>>,
    from_date: Option<String>,
    to_date: Option<String>,
    enable_image_understanding: Option<bool>,
    enable_video_understanding: Option<bool>,
}

pub(super) struct Search {
    query: String,
    max_results: usize,
}

pub(super) fn prepare(
    endpoint: TextEndpoint,
    data: &mut Map<String, Value>,
    model: &Model,
) -> Result<Search, &'static str> {
    let raw = serde_json::from_value::<SearchInput>(Value::Object(std::mem::take(data)))
        .map_err(|_| "搜索参数类型无效")?;
    let query = [raw.query, raw.input]
        .into_iter()
        .flatten()
        .find_map(|query| (!query.trim().is_empty()).then(|| query.trim().to_owned()))
        .ok_or("必须提供搜索内容")?;
    let max_results = raw
        .max_results
        .filter(|limit| *limit > 0)
        .unwrap_or(5)
        .min(20) as usize;
    let mut tool = json!({"type": endpoint.path()});
    let source = if endpoint == TextEndpoint::XaiXSearch {
        for (field, value) in [
            (
                "allowed_x_handles",
                json!(raw.allowed_x_handles.filter(|handles| !handles.is_empty())),
            ),
            (
                "excluded_x_handles",
                json!(raw.excluded_x_handles.filter(|handles| !handles.is_empty())),
            ),
            (
                "from_date",
                json!(
                    raw.from_date
                        .as_deref()
                        .map(str::trim)
                        .filter(|date| !date.is_empty())
                ),
            ),
            (
                "to_date",
                json!(
                    raw.to_date
                        .as_deref()
                        .map(str::trim)
                        .filter(|date| !date.is_empty())
                ),
            ),
            (
                "enable_image_understanding",
                json!(raw.enable_image_understanding),
            ),
            (
                "enable_video_understanding",
                json!(raw.enable_video_understanding),
            ),
        ] {
            if !value.is_null() {
                tool[field] = value;
            }
        }
        "X"
    } else {
        "the web"
    };
    if !model.allows_server_tool(endpoint.path(), &tool) {
        return Err("此公开模型未授权该搜索工具");
    }
    let prompt = format!(
        "Search {source} for the query below. Return up to {max_results} unique cited results as JSON in the form {{\"results\":[{{\"url\":\"https://example.com\",\"title\":\"title\",\"snippet\":\"summary\"}}]}}.\n\n{query}"
    );
    *data = Map::from_iter([
        ("input".into(), json!(prompt)),
        ("tools".into(), json!([tool])),
        ("tool_choice".into(), json!("required")),
        (
            "include".into(),
            json!([format!("{}_call.action.sources", endpoint.path())]),
        ),
        ("stream".into(), json!(false)),
        ("store".into(), json!(false)),
    ]);
    Ok(Search { query, max_results })
}

impl Search {
    pub(super) async fn respond(self, upstream: client::Response, mut call: Call) -> HttpResult {
        let status = upstream.status();
        let headers = downstream_headers(upstream.headers());
        let payload = upstream.json::<Value>().ok();
        call.release().await;
        if let Some(payload) = &payload {
            call.observe_usage(payload);
            call.observe_final_cost(payload);
        }
        let Some(output) = payload.as_ref().and_then(|body| {
            (body["status"].is_null() || body["status"] == "completed")
                .then(|| body["output"].as_array())
                .flatten()
        }) else {
            call.outcome("invalid_upstream_response");
            if !call.settle().await {
                return usage_storage_failure(Protocol::OpenAi, &mut call).response;
            }
            return Protocol::OpenAi.error(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "上游未返回完整的搜索响应",
            );
        };
        call.observe_standalone_search();
        if !call.complete().await {
            return usage_storage_failure(Protocol::OpenAi, &mut call).response;
        }
        Response::json(
            status,
            json!({
                "query": self.query,
                "provider": "grok-native",
                "max_results": self.max_results,
                "results": results(output, self.max_results),
            }),
        )
        .map(|response| response.with_headers(headers))
    }
}

fn source_key(url: &str) -> Option<String> {
    let uri = Uri::of(url.trim()).ok()?;
    if !matches!(uri.scheme(), Some("http" | "https"))
        || uri.host().is_none_or(str::is_empty)
        || uri.user_info().is_some()
    {
        return None;
    }
    Some(uri.without_fragment().ok()?.value().to_owned())
}

fn results(output: &[Value], limit: usize) -> Vec<Value> {
    let mut sources = BTreeMap::new();
    let mut order = Vec::new();
    let mut add = |source: &Value| {
        let url = source
            .as_str()
            .or_else(|| source["url"].as_str())
            .unwrap_or_default()
            .trim();
        if let Some(key) = source_key(url) {
            let entry = sources.entry(key.clone()).or_insert_with(|| {
                order.push(key);
                (url.to_owned(), String::new(), String::new())
            });
            for (field, destination) in [("title", &mut entry.1), ("snippet", &mut entry.2)] {
                if destination.is_empty() {
                    source[field]
                        .as_str()
                        .unwrap_or_default()
                        .trim()
                        .clone_into(destination);
                }
            }
        }
    };
    let mut text = Vec::new();
    for item in output {
        if matches!(
            item["type"].as_str(),
            Some("web_search_call" | "x_search_call")
        ) && let Some(found) = item["action"]["sources"].as_array()
        {
            found.iter().for_each(&mut add);
        }
        if item["type"] == "message"
            && let Some(parts) = item["content"].as_array()
        {
            for part in parts.iter().filter(|part| part["type"] == "output_text") {
                if let Some(annotations) = part["annotations"].as_array() {
                    annotations
                        .iter()
                        .filter(|annotation| {
                            matches!(annotation["type"].as_str(), Some("url_citation" | "web"))
                        })
                        .for_each(&mut add);
                }
                if let Some(value) = part["text"].as_str() {
                    text.push(value);
                }
            }
        }
    }
    let mut results = Vec::new();
    let mut push = |key: &str, candidate: &Value| {
        if results.len() < limit
            && let Some((url, title, snippet)) = sources.remove(key)
        {
            let title = description(&candidate["title"])
                .unwrap_or_else(|| if title.is_empty() { &url } else { &title });
            let snippet = description(&candidate["snippet"]).unwrap_or(&snippet);
            results.push(json!({"url": url, "title": title, "snippet": snippet}));
        }
    };
    for value in text {
        if let Some(start) = value.find('{')
            && let Some(end) = value.rfind('}')
            && start < end
            && let Ok(payload) = serde_json::from_str::<Value>(&value[start..=end])
            && let Some(candidates) = payload["results"].as_array()
        {
            for candidate in candidates {
                if let Some(key) = candidate["url"].as_str().and_then(source_key) {
                    push(&key, candidate);
                }
            }
        }
    }
    for key in order {
        push(&key, &Value::Null);
    }
    results
}

fn description(value: &Value) -> Option<&str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
