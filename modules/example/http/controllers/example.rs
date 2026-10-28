//! 最小业务 API，直接调用第三方包 chrono 并通过框架 Json 返回。

use super::super::resources::example::ExampleInfo;
use crate::modules::example::services::ExampleMessage;
use openrust::http::{Inject, Json};

pub(crate) async fn show(Inject(message): Inject<ExampleMessage>) -> Json<ExampleInfo> {
    Json(ExampleInfo {
        message: message.text().to_owned(),
        served_at: chrono::Utc::now(),
    })
}

pub(crate) async fn heartbeat_label(
    Inject(i18n): Inject<openrust::i18n::I18n>,
) -> openrust::Result<Json<serde_json::Value>> {
    let locale = openrust::i18n::Locale::new("en")?;
    let message = i18n.translate(
        &locale,
        "example::heartbeat",
        &std::collections::BTreeMap::default(),
    );
    Ok(Json(serde_json::json!({ "message": message })))
}
