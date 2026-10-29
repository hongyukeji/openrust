//! 模块业务通过公开 Cookie 接口保存有限的示例偏好。

use openrust::http::axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use openrust::http::{CookieJar, CookieOptions, CookieResponseExt, Json, Request, RequestInfo};
use serde_json::{Value, json};

const NAME: &str = "example_preference";
const PATH: &str = "/api/example";

fn options() -> CookieOptions {
    CookieOptions {
        path: Some(PATH.into()),
        ..CookieOptions::default()
    }
}

/// 仅读取示例偏好；不返回其他请求 Cookie 或原始请求头。
pub(crate) async fn show(request: RequestInfo) -> Json<Value> {
    Json(json!({
        "preference": request.cookie(NAME),
        "present": request.has_cookie(NAME),
    }))
}

/// 用公开工厂、队列及响应组合入口维护示例偏好。
pub(crate) async fn store(jar: CookieJar, request: Request) -> openrust::Result<Response> {
    let action = request.string("action")?.into_string();
    let value = request.string("value")?.into_string();
    let queued = match action.as_str() {
        "queue" => {
            let cookie = jar.factory().make(NAME, value.clone(), 15, options())?;
            jar.queue(cookie)?;
            true
        }
        "direct" => false,
        "forget" => {
            jar.expire(NAME, options())?;
            true
        }
        _ => {
            return Err(openrust::Error::custom_with_status(
                StatusCode::BAD_REQUEST,
                "example_cookie_action_invalid",
                "无效的示例 Cookie 操作",
            ));
        }
    };
    let body = Json(json!({ "action": action, "queued": queued }));
    if queued {
        Ok(body.into_response())
    } else {
        let cookie = jar.factory().make(NAME, value, 15, options())?;
        Ok(body.with_cookie(cookie)?.into_response())
    }
}
