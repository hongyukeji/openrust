//! 最小业务 API，直接调用第三方包 chrono 并通过框架 Json 返回。

use super::super::resources::example::ExampleInfo;
use crate::app::example::services::ExampleMessage;
use openrust::http::{Inject, Json};

pub(crate) async fn show(Inject(message): Inject<ExampleMessage>) -> Json<ExampleInfo> {
    Json(ExampleInfo {
        message: message.text(),
        served_at: chrono::Utc::now(),
    })
}
