//! 草稿提交后返回审核页，展示当前请求的重定向和下一请求的一次性输入。

use std::collections::BTreeMap;

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

/// 按公开路由名返回示例首页，无需 Session。
pub(crate) async fn named(info: RequestInfo) -> Result<Response> {
    Ok(Redirector::from_info(&info)
        .route("example.index", &BTreeMap::new())?
        .into_response())
}

/// 仅将相对目标转换为 HTTPS；不表示当前监听器已经启用 TLS。
pub(crate) async fn secure(info: RequestInfo) -> Result<Response> {
    Ok(Redirector::from_info(&info)
        .secure("/api/example")?
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
    let redirector = Redirector::from_request(&request);
    let mut redirect = if matches!(mode, "errors" | "custom") {
        redirector.route("example.draft.review", &BTreeMap::new())?
    } else {
        redirector.to("/api/example/draft/review")?
    };
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
const PREVIEW_CONFIRMED: &str = "example.draft.preview_confirmed";

/// 草稿预览先提示确认；该普通业务状态不是认证、授权或敏感操作的权限门。
#[cfg(feature = "session")]
pub(crate) async fn preview(info: RequestInfo) -> Result<Response> {
    if info.session()?.get(PREVIEW_CONFIRMED)? == Some(Value::Bool(true)) {
        return Ok(openrust::http::Json(json!({
            "draft": "示例草稿",
            "preview_confirmed": true,
        }))
        .into_response());
    }
    Ok(Redirector::from_info(&info)
        .guest("/api/example/draft/confirm")?
        .into_response())
}

/// 仅读取导航意图；刷新此确认页不会消费普通 Session 属性。
#[cfg(feature = "session")]
pub(crate) async fn confirmation(info: RequestInfo) -> Result<openrust::http::Json<Value>> {
    Ok(openrust::http::Json(json!({
        "notice": "确认后查看示例草稿；此步骤不登录账户或授予权限",
        "intended": Redirector::from_info(&info).get_intended_url()?,
    })))
}

/// 确认非敏感预览后消费一次意图；请求正文不能指定任意重定向目标。
#[cfg(feature = "session")]
pub(crate) async fn confirm(request: Request) -> Result<Response> {
    request.info().session()?.put(PREVIEW_CONFIRMED, true)?;
    Ok(Redirector::from_request(&request)
        .intended_or("/api/example/draft/review")?
        .into_response())
}

#[cfg(feature = "session")]
fn invalid_mode() -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::BAD_REQUEST,
        "example_draft_mode_invalid",
        "草稿提交模式无效",
    )
}
