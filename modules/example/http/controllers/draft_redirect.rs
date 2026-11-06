//! 草稿提交后返回审核页，展示当前请求的重定向和下一请求的一次性输入。

use std::collections::BTreeMap;

#[cfg(feature = "session")]
use openrust::Error;
use openrust::Result;
use openrust::http::Request;
use openrust::http::RequestInfo;
use openrust::http::axum::http::{HeaderMap, HeaderValue, StatusCode};
use openrust::http::axum::response::{IntoResponse, Response};
use openrust::routing::Redirector;
use openrust::routing::UrlGenerator;

/// 为固定草稿生成临时绝对与相对签名链接，同时展示 Redirector 包装入口。
pub(crate) async fn signed_issue(
    urls: UrlGenerator,
    info: RequestInfo,
) -> Result<openrust::http::Json<serde_json::Value>> {
    let parameters = BTreeMap::from([("id".to_owned(), "42".to_owned())]);
    let absolute = urls.temporary_signed_route("example.draft.signed", 300, &parameters, true)?;
    let relative =
        urls.temporary_signed_route("example.draft.signed.relative", 300, &parameters, false)?;
    let redirect =
        Redirector::from_info(&info).signed_route("example.draft.signed", &parameters)?;
    Ok(openrust::http::Json(
        serde_json::json!({"absolute":absolute,"relative":relative,"redirect":redirect.target_url()}),
    ))
}

/// 实际命中已通过 signed middleware；Request 再次展示公开验签入口。
pub(crate) async fn signed_show(
    info: RequestInfo,
) -> Result<openrust::http::Json<serde_json::Value>> {
    Ok(openrust::http::Json(
        serde_json::json!({"draft":"示例草稿","valid":info.has_valid_signature()?}),
    ))
}

/// 相对签名不绑定主机；仅供有权限的已知草稿路由使用。
pub(crate) async fn signed_relative_show(
    info: RequestInfo,
) -> Result<openrust::http::Json<serde_json::Value>> {
    Ok(openrust::http::Json(
        serde_json::json!({"draft":"示例草稿","valid":info.has_valid_relative_signature()?}),
    ))
}

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

/// 默认概览由模块后续登记的业务概览替换，展示同方法同路径的覆盖。
pub(crate) async fn summary_default() -> openrust::http::Json<serde_json::Value> {
    openrust::http::Json(serde_json::json!({"source":"默认草稿概览"}))
}

/// 静态概览应在 /{id} 前登记，避免被当成草稿 ID。
pub(crate) async fn summary(info: RequestInfo) -> Result<openrust::http::Json<serde_json::Value>> {
    let route = info
        .route()?
        .ok_or_else(|| openrust::Error::custom("example_draft_route_missing", "草稿路由缺失"))?;
    Ok(openrust::http::Json(
        serde_json::json!({"source":"模块草稿概览","action":route.action(),"id":route.parameter("id")}),
    ))
}

/// 固定业务动作由二元组指定，不读取请求中的任意跳转地址。
pub(crate) async fn action(info: RequestInfo) -> Result<Response> {
    Ok(Redirector::from_info(&info)
        .action(
            ("Example\\Draft\\DraftController", "show"),
            &action_parameters(),
        )?
        .into_response())
}

/// 裸 invokable 身份只用于查找已登记路由，执行的仍是显式 Rust handler。
pub(crate) async fn action_preview(info: RequestInfo) -> Result<Response> {
    Ok(Redirector::from_info(&info)
        .action(
            "\\Example\\Draft\\PreviewDraftController",
            &action_parameters(),
        )?
        .into_response())
}

fn action_parameters() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("id".into(), "42".into()),
        ("section".into(), "review".into()),
    ])
}

/// 匿名动作目标展示真实命中身份和参数，并读取一次性表单状态。
pub(crate) async fn action_show(
    info: RequestInfo,
) -> Result<openrust::http::Json<serde_json::Value>> {
    let route = info
        .route()?
        .ok_or_else(|| openrust::Error::custom("example_draft_route_missing", "草稿路由缺失"))?;
    let state = serde_json::json!({
        "draft":"示例动作草稿", "action":route.action(), "id":route.parameter("id"),
        "old_title":null, "title_error":"",
    });
    #[cfg(feature = "session")]
    let state = if info.has_session() {
        let mut state = state;
        state["old_title"] = info.old_json("title")?.unwrap_or(serde_json::Value::Null);
        state["title_error"] = serde_json::json!(
            info.session()?
                .read_errors()?
                .get_bag("default")
                .as_bag()
                .first(None, None)?
        );
        state
    } else {
        state
    };
    Ok(openrust::http::Json(state))
}

/// 提交到固定动作目标；普通跳转无需 Session，存在 Session 时延续原 flash 链。
pub(crate) async fn action_submit(request: Request) -> Result<Response> {
    let mut headers = HeaderMap::new();
    headers.insert("x-example-draft-action", HeaderValue::from_static("review"));
    let redirect = Redirector::from_request(&request).action_with(
        ("Example\\Draft\\DraftController", "show"),
        &action_parameters(),
        StatusCode::SEE_OTHER,
        headers,
    )?;
    #[cfg(feature = "session")]
    let redirect = {
        let mut redirect = redirect;
        if request.info().has_session() {
            redirect.only_input(&["title"])?;
            redirect.with_errors(RedirectErrors::Text("请复核动作草稿标题"), "default")?;
        }
        redirect
    };
    Ok(redirect.into_response())
}

const DRAFT_GUIDE: &str = "https://laravel.com/docs/11.x/redirects?source=openrust&section=draft#redirecting-to-external-domains";

/// 目标由业务常量指定，不读取请求中的任意外链地址。
pub(crate) async fn guide(info: RequestInfo) -> Result<Response> {
    let mut headers = HeaderMap::new();
    headers.insert("x-example-guide", HeaderValue::from_static("official"));
    Ok(Redirector::from_info(&info)
        .away_with(DRAFT_GUIDE, StatusCode::SEE_OTHER, headers)?
        .into_response())
}

/// 提交后刷新到同路径的 GET；刷新目标不携带本次 query。
pub(crate) async fn refresh(request: Request) -> Result<Response> {
    let redirect = Redirector::from_request(&request).refresh()?;
    #[cfg(feature = "session")]
    let redirect = {
        let mut redirect = redirect;
        if request.info().has_session() {
            redirect.with_input()?;
            redirect.with_errors(RedirectErrors::Text("请检查草稿标题"), "default")?;
        }
        redirect
    };
    Ok(redirect.into_response())
}

/// GET 展示草稿和一次性复核提示，不再次刷新，避免重定向循环。
pub(crate) async fn refreshed(
    info: RequestInfo,
) -> Result<openrust::http::Json<serde_json::Value>> {
    #[cfg(feature = "session")]
    if info.has_session() {
        let errors = info.session()?.read_errors()?;
        return Ok(openrust::http::Json(json!({
            "draft": "示例草稿",
            "old_title": info.old_json("title")?,
            "title_error": errors.get_bag("default").as_bag().first(None, None)?,
        })));
    }
    #[cfg(not(feature = "session"))]
    let _ = info;
    Ok(openrust::http::Json(serde_json::json!({
        "draft": "示例草稿",
        "old_title": null,
        "title_error": "",
    })))
}

#[cfg(feature = "session")]
use openrust::http::RedirectErrors;
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
        "old": info.old_all_json()?,
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
    if info.session()?.get_json(PREVIEW_CONFIRMED)? == Some(Value::Bool(true)) {
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
    request
        .info()
        .session()?
        .put_typed(PREVIEW_CONFIRMED, true)?;
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
