//! 最小业务 API，直接调用第三方包 chrono 并通过框架 Json 返回。

use super::super::resources::example::ExampleInfo;
use crate::modules::example::services::{
    EXAMPLE_PRESENTER_ALIAS, ExampleMessage, GreetingPresenter,
};
use openrust::container::ServiceKey;
use openrust::foundation::ApplicationState;
use openrust::http::axum::{
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use openrust::http::{DomainParams, Inject, Json, Path, Request, RequestInfo};
use openrust::routing::UrlGenerator;
use openrust::support::{MessageBag, MessageJsonOptions, MessageKeys, ViewErrorBag};
use serde::{Deserialize, Serialize};

/// 只由综合示例的公开 Handler 扩展处理的业务异常。
#[derive(Debug, thiserror::Error)]
#[error("example domain conflict")]
pub(crate) struct ExampleDomainConflict;

pub(crate) async fn exception_example() -> openrust::Result<Json<serde_json::Value>> {
    Err(openrust::Error::internal(ExampleDomainConflict))
}

#[derive(Deserialize)]
pub(crate) struct ExampleDomainParams {
    account: String,
}

pub(crate) async fn show(
    State(app): State<ApplicationState>,
) -> openrust::Result<Json<ExampleInfo>> {
    let presenter = app
        .resolve_key::<dyn GreetingPresenter>(ServiceKey::named(EXAMPLE_PRESENTER_ALIAS))
        .await?;
    Ok(Json(ExampleInfo {
        message: presenter.text().to_owned(),
        served_at: presenter.served_at(),
    }))
}

#[derive(Deserialize)]
pub(crate) struct MessageBagInput {
    title: Option<String>,
    email: Option<String>,
    record_id: Option<i64>,
}

/// 业务自行收集表单和更新错误；错误包不执行校验或重定向。
pub(crate) async fn message_bag(
    Json(input): Json<MessageBagInput>,
) -> openrust::Result<Json<serde_json::Value>> {
    let mut create = MessageBag::new();
    if input
        .title
        .as_deref()
        .is_none_or(|title| title.trim().is_empty())
    {
        create.add("title", "标题不能为空")?;
        create.add("title", "标题不能为空")?;
        let mut imported = MessageBag::new();
        imported.add("title", "标题不能为空")?;
        create.merge(imported.messages())?;
    }
    if input
        .email
        .as_deref()
        .is_none_or(|email| !email.contains('@'))
    {
        create.add("email", "邮箱格式无效")?;
    }

    let mut update = MessageBag::new();
    if input.record_id.is_none_or(|id| id <= 0) {
        update.add("record_id", "记录编号必须为正数")?;
    }
    let mut errors = ViewErrorBag::new();
    errors.put("default", Box::new(create))?;
    errors.put("update", Box::new(update))?;

    let default = errors.get_bag("default");
    let default = default.as_bag();
    let update = errors.get_bag("update");
    let update = update.as_bag();
    let missing = errors.get_bag("missing");
    Ok(Json(serde_json::json!({
        "default": {
            "keys": default.keys().iter().map(ToString::to_string).collect::<Vec<_>>(),
            "messages": default.messages(),
            "first_title": default.first(Some("title"), None)?,
            "first_missing": default.first(Some("missing"), None)?,
            "all": default.all(None)?,
            "unique": default.unique(None)?,
            "formatted": default.all(Some(":key: :message"))?,
            "format_zero": default.all(Some("0"))?,
            "has_title": default.has(MessageKeys::Keys(vec!["title".into()]))?,
            "has_empty_selection": default.has(MessageKeys::Keys(Vec::new()))?,
            "json": default.to_json(MessageJsonOptions::UNESCAPED_UNICODE)?,
        },
        "update": {
            "keys": update.keys().iter().map(ToString::to_string).collect::<Vec<_>>(),
            "first_record_id": update.first(Some("record_id"), None)?,
            "all": update.all(None)?,
        },
        "missing_bag": {
            "stored": errors.has_bag("missing"),
            "first_title": missing.as_bag().first(Some("title"), None)?,
        },
    })))
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
    let mut output = serde_json::json!({
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
    });
    if let Some(value) = request.input("typed.numeric_timestamp")? {
        let instant = openrust::support::calendar::parse_php_numeric_timestamp(&value)?;
        output["numeric_timestamp"] = serde_json::json!({
            "epoch_seconds": instant.epoch_seconds(),
            "microseconds": instant.microseconds(),
        });
    }
    if let Some(value) = request.input("typed.numeric_format")? {
        let number = value.as_f64().ok_or_else(|| {
            openrust::Error::custom_with_status(
                StatusCode::BAD_REQUEST,
                "example_numeric_format_type",
                "数值格式化示例只接受JSON number",
            )
        })?;
        output["numeric_format"] =
            openrust::support::calendar::format_php_numeric_timestamp(number)?.into();
    }
    Ok(output)
}

/// 由唯一正文提取器读取动态输入；响应只展示业务白名单与凭据处理布尔值。
pub(crate) async fn input(mut request: Request) -> openrust::Result<Json<serde_json::Value>> {
    let original = request.content_bytes(request.content_len()).await?;
    let query: InputQuery = request.deserialize_query()?;
    let title = request.input("title")?;
    let profile_name = request.input("profile.name")?;
    let selected = request
        .only(&["title", "profile.name", "items", "php_keys"])?
        .to_json()?;
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
    let excluded_title = request.except(&["title"])?.get("title")?.is_none();
    let source_has_title = request.all().get("title")?.is_some();
    request.merge(serde_json::json!({"title": "  revised  ", "profile.role": "reader"}))?;
    request.merge_if_missing(serde_json::json!({"title": "ignored", "generated": "fallback"}))?;
    let merged = request
        .only(&["title", "profile.role", "generated"])?
        .to_json()?;
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
        "content_preserved": request.content_bytes(original.len()).await? == original,
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

pub(crate) async fn route_url(urls: UrlGenerator) -> openrust::Result<Json<serde_json::Value>> {
    let target = urls.route("example.index", &std::collections::BTreeMap::new())?;
    Ok(Json(serde_json::json!({"url": target})))
}

pub(crate) async fn domain_route(
    DomainParams(domain): DomainParams<ExampleDomainParams>,
    Path(id): Path<String>,
    urls: UrlGenerator,
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
    locale: openrust::translation::RequestLocale,
    Inject(i18n): Inject<openrust::translation::Translator>,
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

/// 展示入站请求地址；只接受固定公开查询，避免回显调用方的敏感查询。
pub(crate) async fn request_url(mut request: Request) -> openrust::Result<Json<serde_json::Value>> {
    let full_url = request.full_url()?;
    if full_url
        .split_once('?')
        .is_some_and(|(_, query)| query != "label=public&page=1")
    {
        return Err(openrust::Error::custom_with_status(
            StatusCode::BAD_REQUEST,
            "example_url_query",
            "此演示仅接受固定公开查询 label=public&page=1",
        ));
    }
    let original_with_query = request.full_url_with_query(&serde_json::json!({"page": 2}))?;
    let original_without_query = request.full_url_without_query(&["page"])?;
    let address = serde_json::json!({
        "root": request.root()?,
        "url": request.url()?,
        "full_url": full_url,
        "scheme": request.scheme()?,
        "http_host": request.http_host()?,
        "port": request.port()?,
        "base_url": request.base_url()?,
        "client_ip": request.client_ip()?,
        "client_ips": request.client_ips()?,
        "path": request.path(),
        "decoded_path": request.decoded_path()?,
        "segments": request.segments()?,
        "first_segment": request.segment(1)?,
        "zero_segment": request.segment(0)?,
        "missing_segment": request.segment(99)?,
        "is_example": request.is(&["api/example/*"])?,
        "full_url_is_example": request.full_url_is(&["*/api/example/request-url*"])?,
        "prefetch": request.prefetch()?,
        "with_query": original_with_query,
        "without_query": original_without_query,
    });
    request.merge(serde_json::json!({"page": 3}))?;
    let merged = serde_json::json!({
        "full_url": request.full_url()?,
        "with_query": request.full_url_with_query(&serde_json::json!({"view": "compact"}))?,
        "without_query": request.full_url_without_query(&["label"])?,
    });
    request.replace(serde_json::json!({"page": 4}))?;
    Ok(Json(serde_json::json!({
        "address": address,
        "merged": merged,
        "replaced_with_query": request.full_url_with_query(&serde_json::json!({"view": "compact"}))?,
        "replaced_full_url": request.full_url()?,
    })))
}

/// 读取真正匹配的路由；查询与正文不能覆盖路径参数。
pub(crate) async fn route_context(
    request: RequestInfo,
) -> openrust::Result<Json<serde_json::Value>> {
    let route = request.route()?.ok_or_else(|| {
        openrust::Error::custom("example_route_missing", "业务处理器缺少路由上下文")
    })?;
    if request.route_parameter("id")? == Some(serde_json::json!("missing")) {
        return Err(openrust::Error::NotFound("示例记录不存在".into()));
    }
    Ok(Json(serde_json::json!({
        "name": request.route_name()?,
        "is_example": request.route_is(&["unrelated.*", "example.route-context.*"])?,
        "uri": route.uri(),
        "methods": route.methods(),
        "id": request.route_parameter("id")?,
        "absent": request.route_parameter("absent")?,
        "default": request.route_parameter_or("absent", serde_json::json!("public-default"))?,
        "parameters": route.parameters(),
    })))
}

/// 在业务 middleware 前后读取同一个请求的匹配快照，短路也保留命中。
pub(crate) async fn route_context_middleware(
    info: RequestInfo,
    request: openrust::http::axum::extract::Request,
    next: openrust::http::axum::middleware::Next,
) -> openrust::Result<Response> {
    let before = info.route_name()?;
    let mut response = if info.route_parameter("id")? == Some(serde_json::json!("blocked")) {
        openrust::Error::custom_with_status(
            StatusCode::FORBIDDEN,
            "example_route_blocked",
            "示例业务 middleware 拒绝请求",
        )
        .into_response()
    } else {
        next.run(request).await
    };
    for (key, value) in [
        ("x-example-route-before", before),
        ("x-example-route-after", info.route_name()?),
    ] {
        if let Some(value) = value.and_then(|value| HeaderValue::from_str(&value).ok()) {
            response.headers_mut().insert(key, value);
        }
    }
    Ok(response)
}
