//! 最小业务 API，直接调用第三方包 chrono 并通过框架 Json 返回。

use super::super::resources::example::ExampleInfo;
use openrust::http::Json;

pub(crate) async fn show() -> Json<ExampleInfo> {
    Json(ExampleInfo {
        message: "Hello from app/example",
        served_at: chrono::Utc::now(),
    })
}
