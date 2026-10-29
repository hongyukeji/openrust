//! 最小业务 API，直接调用第三方包 chrono 并通过框架 Json 返回。

use super::super::resources::example::ExampleInfo;
use crate::modules::example::services::ExampleMessage;
use openrust::http::RouteUrlGenerator;
use openrust::http::axum::{
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use openrust::http::{DomainParams, Inject, Json, Path, RequestInfo};
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct ExampleDomainParams {
    account: String,
}

pub(crate) async fn show(Inject(message): Inject<ExampleMessage>) -> Json<ExampleInfo> {
    Json(ExampleInfo {
        message: message.text().to_owned(),
        served_at: chrono::Utc::now(),
    })
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(crate) async fn representation(
    request: RequestInfo,
    Inject(message): Inject<ExampleMessage>,
) -> Response {
    // 空 Accept 与缺失 Accept 均由此业务入口明确选择 HTML。
    let preferred = if request.acceptable_content_types().is_empty() {
        Some("html")
    } else {
        request.prefers(&["html", "json"])
    };
    let mut response = match preferred {
        Some("html") => Html(format!(
            "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><title>示例消息</title></head><body><p>{}</p></body></html>",
            escape_html(message.text())
        ))
        .into_response(),
        Some("json") => Json(ExampleInfo {
            message: message.text().to_owned(),
            served_at: chrono::Utc::now(),
        })
        .into_response(),
        _ => openrust::Error::custom_with_status(
            StatusCode::NOT_ACCEPTABLE,
            "not_acceptable",
            "不支持请求的表示格式",
        )
        .into_response(),
    };
    response
        .headers_mut()
        .insert(header::VARY, HeaderValue::from_static("Accept"));
    response
}

#[derive(Deserialize)]
pub(crate) struct RequestBodyInput {
    title: String,
}

pub(crate) async fn request_body(
    request: RequestInfo,
    Json(input): Json<RequestBodyInput>,
) -> openrust::Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "path": request.path(),
        "method": request.method().as_str(),
        "is_json": request.is_json()?,
        "title": input.title,
    })))
}

pub(crate) async fn route_url(
    urls: RouteUrlGenerator,
) -> openrust::Result<Json<serde_json::Value>> {
    let target = urls.route("example.index", &std::collections::BTreeMap::new())?;
    Ok(Json(serde_json::json!({"url": target})))
}

pub(crate) async fn domain_route(
    DomainParams(domain): DomainParams<ExampleDomainParams>,
    Path(id): Path<String>,
    urls: RouteUrlGenerator,
) -> openrust::Result<Json<serde_json::Value>> {
    let parameters = std::collections::BTreeMap::from([
        ("account".to_owned(), domain.account.clone()),
        ("id".to_owned(), id.clone()),
    ]);
    let url = urls.route("example.domain.orders.show", &parameters)?;
    Ok(Json(serde_json::json!({
        "account": domain.account,
        "id": id,
        "url": url,
    })))
}

pub(crate) async fn middleware() -> Json<&'static str> {
    Json("module middleware alias and group")
}

pub(crate) async fn heartbeat_label(
    locale: openrust::i18n::RequestLocale,
    Inject(i18n): Inject<openrust::i18n::I18n>,
) -> openrust::Result<Json<serde_json::Value>> {
    let message = locale.translate(
        &i18n,
        "example::heartbeat",
        &std::collections::BTreeMap::default(),
    );
    let number = i18n
        .format_number(locale.locale(), "12345.67")
        .map_err(openrust::Error::internal)?;
    Ok(Json(serde_json::json!({
        "locale": locale.as_str(),
        "message": message,
        "number": number,
    })))
}
