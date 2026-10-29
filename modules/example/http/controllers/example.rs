//! 最小业务 API，直接调用第三方包 chrono 并通过框架 Json 返回。

use super::super::resources::example::ExampleInfo;
use crate::modules::example::services::ExampleMessage;
use openrust::http::RouteUrlGenerator;
use openrust::http::{Inject, Json};

pub(crate) async fn show(Inject(message): Inject<ExampleMessage>) -> Json<ExampleInfo> {
    Json(ExampleInfo {
        message: message.text().to_owned(),
        served_at: chrono::Utc::now(),
    })
}

pub(crate) async fn route_url(
    urls: RouteUrlGenerator,
) -> openrust::Result<Json<serde_json::Value>> {
    let target = urls.route("example.index", &std::collections::BTreeMap::new())?;
    Ok(Json(serde_json::json!({"url": target})))
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
