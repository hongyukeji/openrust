//! 最小业务 API，直接调用第三方包 chrono 并通过框架 Json 返回。

use super::super::resources::example::ExampleInfo;
use crate::modules::example::services::ExampleMessage;
use openrust::http::RouteUrlGenerator;
use openrust::http::axum::{
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use openrust::http::{DomainParams, Inject, Json, Path, Request, RequestInfo};
use serde::{Deserialize, Serialize};

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

#[derive(Deserialize)]
struct InputQuery {
    page: Option<u32>,
    label: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum InputPublication {
    Draft,
    Published,
}

impl openrust::support::BackedEnum for InputPublication {
    type Backing = String;

    fn try_from_backing(value: Self::Backing) -> Option<Self> {
        match value.as_str() {
            "draft" => Some(Self::Draft),
            "published" => Some(Self::Published),
            _ => None,
        }
    }
}

fn typed_input(request: &Request) -> openrust::Result<serde_json::Value> {
    let profile = request.fluent("typed.profile")?;
    let items = request.collect("typed.items")?;
    let note = request.when_filled("typed.note", Ok)?;
    let published_at = request
        .date("typed.published_at")?
        .map(|date| date.to_iso8601_string())
        .transpose()?;
    let calendar_date = request
        .date_with(
            "typed.calendar_date",
            openrust::support::DateOptions {
                format: Some("!Y-m-d"),
                timezone: Some(openrust::support::TimezoneInput::Name("UTC")),
            },
        )?
        .map(|date| date.to_iso8601_string())
        .transpose()?;
    let duration = request.interval("typed.duration")?.map(|interval| {
        serde_json::json!({
            "years": interval.years,
            "months": interval.months,
            "days": interval.days,
            "hours": interval.hours,
            "minutes": interval.minutes,
            "seconds": interval.seconds,
            "microseconds": interval.microseconds,
            "invert": interval.invert,
            "total_days": interval.total_days(),
        })
    });
    let delay_minutes = request
        .interval_with_unit("typed.delay", openrust::support::IntervalUnit::Minute)?
        .map(|interval| interval.minutes);
    Ok(serde_json::json!({
        "title": request.string("typed.title")?.trim(),
        "quantity": request.integer("typed.quantity")?,
        "amount": request.float("typed.amount")?,
        "subscribed": request.boolean("typed.subscribed")?,
        "bounded_quantity": request.clamp("typed.quantity", 1_i64.into(), 10_i64.into())?,
        "publication": request.enum_value::<InputPublication>("typed.publication")?,
        "publications": request.enums::<InputPublication>("typed.publications")?,
        "items": items.to_value()?,
        "selected": request.array_only(&["typed.title", "typed.quantity"])?,
        "profile_display_name": profile.string("display_name")?,
        "note_produced_value": note.into_value().is_some(),
        "published_at": published_at,
        "calendar_date": calendar_date,
        "duration": duration,
        "delay_minutes": delay_minutes,
    }))
}

/// 由唯一正文提取器读取动态输入；响应只展示业务白名单与凭据处理布尔值。
pub(crate) async fn input(mut request: Request) -> openrust::Result<Json<serde_json::Value>> {
    let original = request.content().clone();
    let query: InputQuery = request.deserialize_query()?;
    let title = request.input("title")?;
    let profile_name = request.input("profile.name")?;
    let selected = request.only(&["title", "profile.name", "items", "php_keys"])?;
    let typed = typed_input(&request)?;
    let checks = serde_json::json!({
        "has_nullable": request.has(&["nullable"]),
        "nullable_filled": request.filled(&["nullable"]),
        "has_any": request.has_any(&["missing", "title"]),
        "missing": request.missing(&["missing"]),
        "is_not_filled": request.is_not_filled(&["nullable"]),
        "any_filled": request.any_filled(&["nullable", "title"]),
        "password_has_outer_spaces": outer_spaces(request.input("password")?.as_ref()),
        "nested_password_has_outer_spaces": outer_spaces(request.input("user.password")?.as_ref()),
        "empty_password_is_null": request.input("password_confirmation")? == Some(serde_json::Value::Null),
    });
    let query_title = request.query("title");
    let form_title = request.post("title");
    let json_title = request.json("title")?;
    let excluded_title = request.except(&["title"])?.get("title").is_none();
    let source_has_title = request.all().get("title").is_some();
    request.merge(serde_json::json!({"title": "  revised  ", "profile.role": "reader"}))?;
    request.merge_if_missing(serde_json::json!({"title": "ignored", "generated": "fallback"}))?;
    let merged = request.only(&["title", "profile.role", "generated"])?;
    request.replace(serde_json::json!({"title": "replacement"}))?;
    Ok(Json(serde_json::json!({
        "method": request.info().method().as_str(),
        "selected": selected,
        "typed": typed,
        "title": title,
        "profile_name": profile_name,
        "checks": checks,
        "query_title": query_title,
        "query_page": query.page,
        "query_label": query.label,
        "form_title": form_title,
        "json_title": json_title,
        "excluded_title": excluded_title,
        "source_has_title": source_has_title,
        "merged": merged,
        "replaced_title": request.input("title")?,
        "query_fallback": request.input("fallback")?,
        "content_bytes": original.len(),
        "content_preserved": request.content() == &original,
    })))
}

fn outer_spaces(value: Option<&serde_json::Value>) -> bool {
    value
        .and_then(serde_json::Value::as_str)
        .is_some_and(|text| text.starts_with(' ') && text.ends_with(' '))
}

pub(crate) async fn input_raw(request: Request) -> openrust::Result<Json<serde_json::Value>> {
    Ok(Json(serde_json::json!({
        "title": request.input("title")?,
        "nullable": request.input("nullable")?,
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
