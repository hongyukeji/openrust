//! 浏览器绑定 Cookie 与一次性回调；绑定管理只供可截获回调的原生/服务客户端。
use crate::modules::example::http::requests::oidc::{
    AttachSessionInput, BindInput, BrowserBindInput, CallbackQuery, LogoutInput, UnbindInput,
};
use crate::modules::example::http::resources::credentials::CredentialsResponse;
use crate::modules::example::services::oidc::OidcAccount;
use openrust::auth::{CurrentUser, ExternalIdentityAuthorization, LoginAttempt, Session};
use openrust::http::axum::{
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Redirect, Response},
};
use openrust::http::{ClientIp, Inject, Json, Query, RequestInfo, Validated};
use openrust::{Error, Result};
fn private(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}
fn cookie(request: &RequestInfo, state: &str) -> Result<String> {
    let name = ExternalIdentityAuthorization::cookie_name_for_callback(state)?;
    let mut count = 0;
    // 解密后的 HeaderMap 可规范化同名值；拒绝重复必须核原始 wire，值经公开解密视图读取。
    for value in request.headers().get_all(header::COOKIE) {
        for part in value.to_str().map_err(|_| Error::Unauthorized)?.split(';') {
            if part
                .trim()
                .split_once('=')
                .is_some_and(|(key, _)| key == name)
            {
                count += 1;
            }
        }
    }
    if count != 1 {
        return Err(Error::Unauthorized);
    }
    request
        .cookie(&name)
        .and_then(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty() && value.len() <= 256)
                .map(str::to_owned)
        })
        .ok_or(Error::Unauthorized)
}

fn finished(state: &str, result: Result<Response>) -> Response {
    let mut response = private(match result {
        Ok(response) => response,
        Err(error) => error.into_response(),
    });
    if let Ok(value) = ExternalIdentityAuthorization::clear_cookie_header(state)
        .and_then(|value| value.parse().map_err(Error::internal))
    {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}
pub(crate) async fn authorize(
    Inject(account): Inject<OidcAccount>,
    source: ClientIp,
) -> Result<Response> {
    let authorization = account.authorize(&source.ip().to_string()).await?;
    let mut response = private(Redirect::temporary(authorization.url().as_str()).into_response());
    response.headers_mut().append(
        header::SET_COOKIE,
        authorization
            .set_cookie_header()
            .parse()
            .map_err(Error::internal)?,
    );
    Ok(response)
}
#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum LoginResponse {
    Authenticated {
        credentials: CredentialsResponse,
        external_login_handle: String,
    },
    #[cfg(feature = "auth-mfa")]
    MfaRequired {
        challenge: String,
        expires_at: String,
        external_login_handle: String,
    },
}
pub(crate) async fn callback(
    Inject(account): Inject<OidcAccount>,
    request: RequestInfo,
    source: ClientIp,
    Query(input): Query<CallbackQuery>,
    session: Session,
) -> Response {
    if input.state.is_empty() || input.state.len() > 256 {
        return finished(&input.state, Err(Error::Unauthorized));
    }
    match account.is_browser_bind(&input.state).await {
        Ok(true) => return browser_callback(&account, &request, &input, &session).await,
        Ok(false) => {}
        Err(error) => return finished(&input.state, Err(error)),
    }
    if input.code.is_none() && input.error.is_none() {
        let result = account
            .complete_logout(&input.state)
            .await
            .map(|()| Json(serde_json::json!({"status":"logged_out"})).into_response());
        return match result {
            Ok(response) => private(response),
            Err(error) => finished(&input.state, Err(error)),
        };
    }
    let result = async {
        if input.state.is_empty() || input.state.len() > 256 || input.error.is_some() {
            return Err(Error::Unauthorized);
        }
        let binding = cookie(&request, &input.state)?;
        let code = input
            .code
            .as_deref()
            .filter(|code| !code.is_empty() && code.len() <= 4096)
            .ok_or(Error::Unauthorized)?;
        let (attempt, handle) = account
            .complete(&input.state, code, &binding, &source.ip().to_string())
            .await?
            .into_parts();
        let external_login_handle = handle.expose().to_owned();
        let result = match attempt {
            LoginAttempt::Authenticated(value) => LoginResponse::Authenticated {
                external_login_handle,
                credentials: CredentialsResponse {
                    user_id: value.user_id(),
                    token_type: "Bearer",
                    access_token: value.access_token().into(),
                    refresh_token: value.refresh_token().expose().into(),
                    refresh_expires_at: value.refresh_expires_at().to_rfc3339(),
                },
            },
            #[cfg(feature = "auth-mfa")]
            LoginAttempt::MfaRequired(challenge) => LoginResponse::MfaRequired {
                external_login_handle,
                challenge: challenge.expose().into(),
                expires_at: challenge.expires_at().to_rfc3339(),
            },
        };
        Ok(Json(result).into_response())
    }
    .await;
    finished(&input.state, result)
}
pub(crate) async fn linked(
    user: CurrentUser,
    Inject(account): Inject<OidcAccount>,
) -> Result<Response> {
    let links = account.linked(user.id()).await?;
    Ok(private(Json(links.into_iter().map(|link|serde_json::json!({"provider":link.provider(),"issuer":link.issuer(),"subject":link.subject()})).collect::<Vec<_>>()).into_response()))
}
pub(crate) async fn bind(
    user: CurrentUser,
    Inject(account): Inject<OidcAccount>,
    request: RequestInfo,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<BindInput>>,
) -> Response {
    let state = input.state.clone();
    let result = async {
        if account.is_browser_bind(&state).await? {
            return Err(Error::Unauthorized);
        }
        let binding = cookie(&request, &state)?;
        account
            .bind(user.id(), input, &binding, &source.ip().to_string())
            .await?;
        Ok(Json(serde_json::json!({"status":"bound"})).into_response())
    }
    .await;
    finished(&state, result)
}
pub(crate) async fn unbind(
    user: CurrentUser,
    Inject(account): Inject<OidcAccount>,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<UnbindInput>>,
) -> Result<Response> {
    let removed = account
        .unbind(user.id(), input, &source.ip().to_string())
        .await?;
    Ok(private(
        Json(serde_json::json!({"removed":removed})).into_response(),
    ))
}

const PENDING_BIND_KEY: &str = "example.oidc.pending_bind";
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PendingBind {
    state_hash: String,
    owner: i64,
    csrf: String,
}
/// 只接受经过框架 Session AuthLayer/CSRF 校验的同源 JSON 请求。
pub(crate) async fn browser_bind_authorize(
    user: CurrentUser,
    Inject(account): Inject<OidcAccount>,
    session: Session,
    headers: HeaderMap,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<BrowserBindInput>>,
) -> Result<Response> {
    account.require_browser_origin(&headers)?;
    let csrf = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 256)
        .ok_or(Error::Unauthorized)?;
    let authorization = account
        .begin_browser_bind(
            user.id(),
            &input.email,
            &input.password,
            &source.ip().to_string(),
        )
        .await?;
    let state = authorization
        .url()
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .ok_or(Error::Unauthorized)?;
    // 单个 Session 最多保留一个意图；密码与 code 不进入 Session 或意图表。
    session.put(
        PENDING_BIND_KEY,
        PendingBind {
            state_hash: crate::modules::example::services::oidc::state_hash(&state),
            owner: user.id(),
            csrf: csrf.into(),
        },
    )?;
    let mut response = private(
        Json(serde_json::json!({"authorization_url":authorization.url().as_str()})).into_response(),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        authorization
            .set_cookie_header()
            .parse()
            .map_err(Error::internal)?,
    );
    Ok(response)
}
async fn browser_callback(
    account: &OidcAccount,
    request: &RequestInfo,
    input: &CallbackQuery,
    session: &Session,
) -> Response {
    let result = async {
        let pending = session
            .get_typed::<PendingBind>(PENDING_BIND_KEY)?
            .ok_or(Error::Unauthorized)?;
        if pending.state_hash != crate::modules::example::services::oidc::state_hash(&input.state) {
            return Err(Error::Unauthorized);
        }
        let binding = cookie(request, &input.state)?;
        // 错误回调同样消耗本模块意图，但不会调用登录 complete。
        if input.error.is_some()
            || input
                .code
                .as_ref()
                .is_none_or(|code| code.is_empty() || code.len() > 4096)
        {
            account
                .transition_browser_bind(
                    pending.owner,
                    &input.state,
                    &binding,
                    "pending",
                    "consumed",
                )
                .await?;
            session.forget(&[PENDING_BIND_KEY])?;
            return Err(Error::Unauthorized);
        }
        let code = input
            .code
            .as_deref()
            .filter(|value| !value.is_empty() && value.len() <= 4096)
            .ok_or(Error::Unauthorized)?;
        account
            .transition_browser_bind(pending.owner, &input.state, &binding, "pending", "ready")
            .await?;
        confirmation_page(&input.state, code, &pending.csrf)
    }
    .await;
    match result {
        Ok(response) => response,
        Err(error) => finished(&input.state, Err(error)),
    }
}
/// 先原子占位再重新验证密码；取消、错误或并发请求都不能重新使用这个意图。
pub(crate) async fn browser_bind_confirm(
    user: CurrentUser,
    Inject(account): Inject<OidcAccount>,
    session: Session,
    headers: HeaderMap,
    request: RequestInfo,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<BindInput>>,
) -> Response {
    let state = input.state.clone();
    let result = async {
        account.require_browser_origin(&headers)?;
        let pending = session
            .get_typed::<PendingBind>(PENDING_BIND_KEY)?
            .ok_or(Error::Unauthorized)?;
        if pending.owner != user.id()
            || pending.state_hash != crate::modules::example::services::oidc::state_hash(&state)
        {
            return Err(Error::Unauthorized);
        }
        let binding = cookie(&request, &state)?;
        account
            .transition_browser_bind(user.id(), &state, &binding, "ready", "consumed")
            .await?;
        session.forget(&[PENDING_BIND_KEY])?;
        account
            .bind(user.id(), input, &binding, &source.ip().to_string())
            .await?;
        Ok(Json(serde_json::json!({"status":"bound"})).into_response())
    }
    .await;
    finished(&state, result)
}
fn escape_html(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&#39;".into(),
            _ => ch.to_string(),
        })
        .collect()
}
fn confirmation_page(state: &str, code: &str, csrf: &str) -> Result<Response> {
    let nonce = crate::modules::example::services::oidc::state_hash(&format!("{csrf}:{state}"));
    let html = format!(
        r#"<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>确认绑定外部身份</title>
<h1>确认绑定外部身份</h1><p>请重新输入当前本地账户邮箱和密码。不会切换为外部身份账户。</p>
<form id="bind"><input id="state" type="hidden" value="{}"><input id="code" type="hidden" value="{}"><input id="csrf" type="hidden" value="{}">
<label>邮箱<input id="email" type="email" autocomplete="username" required></label><label>密码<input id="password" type="password" autocomplete="current-password" required></label><button>确认绑定</button></form><p id="result" role="status"></p>
<script nonce="{nonce}">history.replaceState(null,'','/api/example/oidc/callback');document.getElementById('bind').addEventListener('submit',async function(event){{event.preventDefault();const element=id=>document.getElementById(id);const data={{state:element('state').value,code:element('code').value,email:element('email').value,password:element('password').value}};const csrf=element('csrf').value;this.querySelector('button').disabled=true;for(const id of ['code','password','csrf'])element(id).value='';try{{const response=await fetch('/api/example/oidc/bind/confirm',{{method:'POST',credentials:'same-origin',headers:{{'Content-Type':'application/json','X-Example-Request':'same-origin','X-CSRF-Token':csrf}},body:JSON.stringify(data)}});const payload=await response.json();const succeeded=response.ok&&payload?.success===true&&payload?.data?.status==='bound';element('result').textContent=succeeded?'绑定成功':'绑定失败，请重新发起授权';}}catch(_){{element('result').textContent='请求失败，请重新发起授权';}}}});</script></html>"#,
        escape_html(state),
        escape_html(code),
        escape_html(csrf)
    );
    let mut response = private(openrust::http::axum::response::Html(html).into_response());
    response.headers_mut().insert(header::CONTENT_SECURITY_POLICY,
        format!("default-src 'none'; script-src 'nonce-{nonce}'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'").parse().map_err(Error::internal)?);
    Ok(response)
}

pub(crate) async fn logout(
    user: CurrentUser,
    Inject(account): Inject<OidcAccount>,
    Inject(sessions): Inject<openrust::auth::SessionStoreHandle>,
    Validated(Json(input)): Validated<Json<LogoutInput>>,
) -> Result<Response> {
    let redirect = account.logout(user.id(), input.handle, &sessions).await?;
    Ok(private(Json(serde_json::json!({"status":"locally_logged_out","end_session_url":redirect.as_ref().map(|redirect|redirect.url().as_str())})).into_response()))
}
/// 只关联现有已认证 Session；不是 OIDC 直接创建 Session，也不绕过现有 MFA Session 限制。
pub(crate) async fn attach_session(
    user: CurrentUser,
    Inject(account): Inject<OidcAccount>,
    session: Session,
    headers: HeaderMap,
    source: ClientIp,
    Validated(Json(input)): Validated<Json<AttachSessionInput>>,
) -> Result<Response> {
    account.require_browser_origin(&headers)?;
    account
        .attach_session(
            user.id(),
            input.handle,
            &input.email,
            &input.password,
            &source.ip().to_string(),
            &session,
        )
        .await?;
    Ok(private(
        Json(serde_json::json!({"status":"session_attached"})).into_response(),
    ))
}
/// `IdP` 签名验证是认证边界；浏览器 `Origin`/`JWT` 不能替代 backchannel token 校验。
pub(crate) async fn backchannel(
    Inject(account): Inject<OidcAccount>,
    Inject(sessions): Inject<openrust::auth::SessionStoreHandle>,
    request: openrust::http::axum::extract::Request,
) -> Response {
    let result = async {
        if request
            .headers()
            .get_all(header::CONTENT_TYPE)
            .iter()
            .count()
            != 1
            || request
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .map(str::trim)
                != Some("application/x-www-form-urlencoded")
        {
            return Err(Error::BadRequest("需要 logout_token 表单".into()));
        }
        let bytes = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            openrust::http::axum::body::to_bytes(request.into_body(), 16 * 1024),
        )
        .await
        .map_err(|_| Error::BadRequest("退出通知请求超过期限".into()))?
        .map_err(|_| Error::BadRequest("退出通知请求超过大小限制".into()))?;
        let fields = url::form_urlencoded::parse(&bytes).collect::<Vec<_>>();
        if fields.len() != 1 || fields[0].0 != "logout_token" || fields[0].1.is_empty() {
            return Err(Error::BadRequest("需要唯一的 logout_token".into()));
        }
        let outcome = account.backchannel(&fields[0].1, &sessions).await?;
        let (status, revoked) = match outcome {
            openrust::auth::BackchannelLogoutOutcome::Processed(count) => ("processed", count),
            openrust::auth::BackchannelLogoutOutcome::AlreadyProcessed => ("already_processed", 0),
        };
        Ok(Json(serde_json::json!({"status":status,"revoked":revoked})).into_response())
    }
    .await;
    private(match result {
        Ok(response) => response,
        Err(error) => error.into_response(),
    })
}

#[cfg(test)]
mod browser_page_tests {
    use openrust::http::axum::response::IntoResponse;
    use std::process::Command;

    #[tokio::test]
    async fn binding_success_json_matches_confirmation_page_envelope() -> openrust::Result<()> {
        let response = super::Json(serde_json::json!({ "status": "bound" })).into_response();
        let body = openrust::http::axum::body::to_bytes(response.into_body(), 16 * 1024)
            .await
            .map_err(openrust::Error::internal)?;
        let payload: serde_json::Value =
            serde_json::from_slice(&body).map_err(openrust::Error::internal)?;
        assert_eq!(payload["success"], true);
        assert_eq!(payload["data"]["status"], "bound");
        Ok(())
    }

    #[tokio::test]
    async fn binding_confirmation_requires_http_and_business_success() -> openrust::Result<()> {
        let page = super::confirmation_page("state", "code", "csrf")?;
        let body = openrust::http::axum::body::to_bytes(page.into_body(), 16 * 1024)
            .await
            .map_err(openrust::Error::internal)?;
        let html = std::str::from_utf8(&body).map_err(openrust::Error::internal)?;
        assert!(
            html.contains("response.ok&&payload?.success===true&&payload?.data?.status==='bound'")
        );
        assert!(html.contains("const payload=await response.json()"));
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需安装 Python Playwright 与 Chromium；显式运行以执行真实页面 JavaScript"]
    async fn browser_confirmation_reports_success_and_failures_from_actual_page()
    -> openrust::Result<()> {
        let page = super::confirmation_page("state", "code", "csrf")?;
        let body = openrust::http::axum::body::to_bytes(page.into_body(), 16 * 1024)
            .await
            .map_err(openrust::Error::internal)?;
        let html = std::str::from_utf8(&body).map_err(openrust::Error::internal)?;
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("modules/example/tests/oidc_confirmation_browser.py");
        let output = Command::new("python3")
            .arg(script)
            .env("OPENRUST_OIDC_CONFIRMATION_HTML", html)
            .output()
            .map_err(openrust::Error::internal)?;
        if !output.status.success() {
            return Err(openrust::Error::custom(
                "example_oidc_browser_test_failed",
                "OIDC 确认页真实浏览器回归失败；子进程输出已隐藏",
            ));
        }
        Ok(())
    }

    #[test]
    fn confirmation_attributes_escape_quotes_and_markup() {
        assert_eq!(
            super::escape_html("\"><script>&'"),
            "&quot;&gt;&lt;script&gt;&amp;&#39;"
        );
    }
}
