//! 草稿提交后返回审核页，展示当前请求的重定向和下一请求的一次性输入。

#[cfg(feature = "session")]
use openrust::Error;
use openrust::Result;
use openrust::http::axum::response::{IntoResponse, Response};
use openrust::http::{Redirector, RequestInfo};

/// 无 Session 时仍可用的普通目标重定向。
pub(crate) async fn plain(info: RequestInfo) -> Result<Response> {
    Ok(Redirector::from_info(&info)
        .to("/api/example")?
        .into_response())
}

/// Referer、前次 URL 与回退地址由框架按当前请求决定。
pub(crate) async fn back(info: RequestInfo) -> Result<Response> {
    Ok(Redirector::from_info(&info)
        .back(Some("/api/example"))?
        .into_response())
}

#[cfg(feature = "session")]
use openrust::http::{RedirectErrors, Request};
#[cfg(feature = "session")]
use openrust::support::{ArrayKey, MessageInput};
#[cfg(feature = "session")]
use serde_json::{Value, json};

/// 草稿表单选择闪存全部、白名单或排除字段；审核错误通过公开错误包扩展保存。
#[cfg(feature = "session")]
pub(crate) async fn submit(request: Request) -> Result<Response> {
    let mode = request.input("mode")?;
    let mode = mode
        .as_ref()
        .and_then(Value::as_str)
        .ok_or_else(invalid_mode)?;
    let mut redirect = Redirector::from_request(&request).to("/api/example/draft/review")?;
    match mode {
        "all" => {
            redirect.with_input()?;
        }
        "only" => {
            redirect.only_input(&["title", "summary"])?;
        }
        "except" => {
            redirect.except_input(&["token", "mode"])?;
        }
        "errors" => {
            redirect.only_input(&["title"])?;
            let entries = [(
                ArrayKey::from("title"),
                MessageInput::One("标题不能为空".into()),
            )];
            redirect.with_errors(RedirectErrors::Entries(&entries), "default")?;
            redirect.with_errors(RedirectErrors::Text("摘要需要补充"), "other")?;
        }
        "custom" => {
            let errors =
                crate::modules::example::services::draft_errors::DraftErrors::new("请联系编辑")?;
            redirect.with_errors(RedirectErrors::Text("摘要需要补充"), "other")?;
            redirect.with_errors(RedirectErrors::Provider(&errors), "default")?;
        }
        _ => return Err(invalid_mode()),
    }
    Ok(redirect.into_response())
}

/// 显式读取当前 Session 的旧输入及命名错误包；读取本身不延长 flash 寿命。
#[cfg(feature = "session")]
pub(crate) async fn review(info: RequestInfo) -> Result<openrust::http::Json<Value>> {
    let errors = info.session()?.read_errors()?;
    Ok(openrust::http::Json(json!({
        "old": info.old_all()?,
        "default": {
            "present": errors.has_bag("default"),
            "title": errors.get_bag("default").as_bag().first(Some("title"), None)?,
        },
        "other": {
            "present": errors.has_bag("other"),
            "message": errors.get_bag("other").as_bag().first(None, None)?,
        }
    })))
}

#[cfg(feature = "session")]
fn invalid_mode() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::BAD_REQUEST,
        "example_draft_mode_invalid",
        "草稿提交模式无效",
    )
}
