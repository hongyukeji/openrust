//! 默认 Web 请求伪造防护。
//!
//! 依次检查请求来源和请求令牌。

use crate::config::SessionConfig;
use crate::http::{CookieJar, CookieOptions, RequestInfo, request::InputTruthy};
use crate::routing::{invocation::suppress_cookies, methods::ManagedNext};
use crate::{Error, Result};
use axum::{
    body::Body,
    extract::{FromRequestParts, Request},
    response::{IntoResponse, Response},
};
use http::{Method, StatusCode};
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Web CSRF 策略；应用在 Provider boot 时一次配置，不能从客户端请求修改。
#[derive(Clone)]
pub struct PreventRequestForgery {
    except: Vec<String>,
    allow_same_site: bool,
    origin_only: bool,
    add_xsrf_cookie: bool,
}

impl Default for PreventRequestForgery {
    fn default() -> Self {
        Self::new()
    }
}

impl PreventRequestForgery {
    /// 验证来源或 Session token，成功时排入 XSRF Cookie。
    pub fn new() -> Self {
        Self {
            except: Vec::new(),
            allow_same_site: false,
            origin_only: false,
            add_xsrf_cookie: true,
        }
    }

    /// 排除应用拥有的路径或完整 URL；支持与 `Request::is` 相同的星号模式。
    #[must_use]
    pub fn except(mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.except = paths.into_iter().map(Into::into).collect();
        self
    }

    /// 允许浏览器报告的 same-site 来源；默认只信 same-origin。
    #[must_use]
    pub fn allow_same_site(mut self) -> Self {
        self.allow_same_site = true;
        self
    }

    /// 只验证浏览器来源头；非同源请求返回 403 且不发送 XSRF cookie。
    #[must_use]
    pub fn origin_only(mut self) -> Self {
        self.origin_only = true;
        self
    }

    /// 关闭给 JavaScript 读取的 XSRF cookie，仍保留服务端验证。
    #[must_use]
    pub fn without_xsrf_cookie(mut self) -> Self {
        self.add_xsrf_cookie = false;
        self
    }
}

#[derive(Clone)]
pub(crate) struct ForgeryRuntime {
    pub(crate) policy: PreventRequestForgery,
    pub(crate) cookie: Arc<crate::cookie::middleware::EncryptCookies>,
    pub(crate) xsrf: Option<SessionConfig>,
    pub(crate) repository: crate::config::repository::ConfigRepository,
    pub(crate) testing_boot: bool,
}

impl PreventRequestForgery {
    pub(crate) fn sends_xsrf_cookie(&self) -> bool {
        !self.origin_only && self.add_xsrf_cookie
    }
}
impl ForgeryRuntime {
    pub(crate) fn sends_xsrf_cookie(&self) -> bool {
        self.policy.sends_xsrf_cookie()
    }

    fn xsrf_config(&self) -> Result<SessionConfig> {
        if let Some(config) = &self.xsrf {
            Ok(config.clone())
        } else if self.repository.get("session")?.is_some() {
            Ok(self.repository.section("session")?)
        } else {
            Ok(SessionConfig::default())
        }
    }

    fn require_session(
        &self,
        request: &Request,
        cookie: &crate::http::cookie::CookieLayer,
    ) -> Result<crate::session::Session> {
        let session = request
            .extensions()
            .get::<crate::session::Session>()
            .cloned()
            .ok_or_else(missing_session)?;
        session.require_live_started()?;
        let name = session.name()?;
        let protected = if self.sends_xsrf_cookie() {
            cookie.protects_web_cookies(&name)
        } else {
            cookie.protects_cookie(&name)
        };
        if !protected {
            return Err(Error::custom(
                "web_cookie_encryption_missing",
                "Web CSRF 要求保护实际 Session Cookie",
            ));
        }
        Ok(session)
    }
}

/// 仅此 middleware 可创建，供同一请求的 Session auth 识别已完成的防护。
#[derive(Clone, Copy)]
pub(crate) struct CsrfVerified;

/// Web Session token 缺失或不匹配；由统一异常 Handler 映射到 419。
#[derive(Debug)]
pub struct CsrfTokenMismatch;
impl std::fmt::Display for CsrfTokenMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CSRF token mismatch")
    }
}
impl std::error::Error for CsrfTokenMismatch {}

/// origin-only 模式拒绝非同源请求；由统一异常 Handler 映射到 403。
#[derive(Debug)]
pub struct OriginMismatch;
impl std::fmt::Display for OriginMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Origin mismatch")
    }
}
impl std::error::Error for OriginMismatch {}

/// 默认 Web 请求伪造防护的旧入口。
#[deprecated(note = "使用 PreventRequestForgery")]
pub type VerifyCsrfToken = PreventRequestForgery;

pub(crate) async fn handle(
    runtime: ForgeryRuntime,
    mut request: Request,
    next: ManagedNext,
) -> Response {
    let invocation = next.invocation();
    let prepared: Result<_> = async {
        let (cookie, _) = crate::http::cookie::middleware::require_prepared(
            &request,
            &runtime.cookie,
            invocation,
        )?;
        let outbound = if runtime.sends_xsrf_cookie() {
            let config = runtime.xsrf_config()?;
            let session = runtime.require_session(&request, &cookie)?;
            config.validate()?;
            current_token(&session)?;
            Some((session, config))
        } else {
            None
        };
        // 普通协议拒绝只由本层的明确校验分支产生。
        if let Some(error) = verify(&runtime, &cookie, &mut request).await? {
            return Ok(Err(error));
        }
        let (_, jar) = crate::http::cookie::middleware::require_prepared(
            &request,
            &runtime.cookie,
            invocation,
        )?;
        Ok(Ok((jar, outbound)))
    }
    .await;
    let (jar, outbound) = match prepared {
        Ok(Ok(prepared)) => prepared,
        Ok(Err(protocol)) => return protocol.into_response(),
        Err(error) => {
            invocation.fail();
            return crate::http::cookie::private_error_response(error);
        }
    };
    request.extensions_mut().insert(CsrfVerified);
    let response = next.run(request).await;
    let Some((session, config)) = outbound else {
        return response;
    };
    if !invocation.seal() {
        invocation.fail();
        let _ = jar.flush_queued_cookies();
        return suppress_cookies(response);
    }
    // 业务可能轮换 token。回程始终读取入站验证过的同一 Session。
    let result = current_token(&session).and_then(|token| add_xsrf_cookie(&config, &jar, token));
    match result {
        Ok(()) => response,
        Err(error) => {
            invocation.fail();
            let _ = jar.flush_queued_cookies();
            crate::http::cookie::private_error_response(error)
        }
    }
}

fn missing_session() -> Error {
    Error::custom("csrf_session_missing", "Web CSRF 要求已启动 Session")
}

fn current_token(session: &crate::session::Session) -> Result<String> {
    session.require_live_started()?;
    session
        .token()?
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| {
            Error::custom(
                "csrf_session_token_missing",
                "Web CSRF Session token 不可用",
            )
        })
}

async fn verify(
    runtime: &ForgeryRuntime,
    cookie: &crate::http::cookie::CookieLayer,
    request: &mut Request,
) -> Result<Option<Error>> {
    let (mut parts, body) = std::mem::replace(request, Request::new(Body::empty())).into_parts();
    let info = RequestInfo::from_request_parts(&mut parts, &()).await?;
    *request = Request::from_parts(parts, body);
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let test = runtime.testing_boot;
    let mut excluded = false;
    for path in &runtime.policy.except {
        let pattern = if path == "/" {
            "/"
        } else {
            path.trim_matches('/')
        };
        if info.full_url_is(&[pattern])? || info.is(&[pattern])? {
            excluded = true;
            break;
        }
    }
    if safe || test || excluded {
        return Ok(None);
    }
    let site = request
        .headers()
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok());
    if site == Some("same-origin") || (site == Some("same-site") && runtime.policy.allow_same_site)
    {
        return Ok(None);
    }
    if runtime.policy.origin_only {
        return Ok(Some(Error::http_with_source(
            StatusCode::FORBIDDEN,
            "origin_mismatch",
            "Origin Mismatch",
            OriginMismatch,
        )));
    }
    let session = runtime.require_session(request, cookie)?;
    let expected = session
        .token()?
        .and_then(|token| token.as_str().map(str::to_owned))
        // 应用写入的空值不能充当请求伪造凭证。
        .filter(|token| !cookie.uses_name_bound_protection() || !token.is_empty());
    let (managed, replay) = crate::http::request::prepare_csrf_request(std::mem::replace(
        request,
        Request::new(Body::empty()),
    ))
    .await?;
    *request = replay;
    let token = csrf_token(request.headers(), managed.input("_token")?, cookie)?;
    let valid = expected
        .as_deref()
        .zip(token.as_deref())
        .is_some_and(|(expected, supplied)| expected.as_bytes().ct_eq(supplied.as_bytes()).into());
    if valid {
        Ok(None)
    } else {
        Ok(Some(Error::http_with_source(
            StatusCode::from_u16(419).map_err(Error::internal)?,
            "page_expired",
            "Page Expired",
            CsrfTokenMismatch,
        )))
    }
}

fn csrf_token(
    headers: &http::HeaderMap,
    input: Option<serde_json::Value>,
    cookie: &crate::http::cookie::CookieLayer,
) -> Result<Option<String>> {
    let name_bound = cookie.uses_name_bound_protection();
    // PHP 的 ?: 先判断输入真假。真值的非字符串会在令牌比对时被拒绝。
    if name_bound && input.is_truthy() {
        return Ok(input.and_then(|value| value.as_str().map(str::to_owned)));
    }
    let input = input
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.is_empty());
    let header = headers.get("x-csrf-token");
    if name_bound
        && let Some(value) =
            header.filter(|value| !value.as_bytes().is_empty() && value.as_bytes() != b"0")
    {
        return Ok(value.to_str().ok().map(str::to_owned));
    }
    let header = header.and_then(|value| value.to_str().ok());
    if !name_bound
        && let Some(token) =
            input.or_else(|| header.filter(|value| !value.is_empty()).map(str::to_owned))
    {
        return Ok(Some(token));
    }
    let xsrf = headers
        .get("x-xsrf-token")
        .filter(|value| !name_bound || (!value.as_bytes().is_empty() && value.as_bytes() != b"0"));
    let Some(xsrf) = xsrf else {
        return Ok(header.filter(|_| name_bound).map(str::to_owned));
    };
    if !cookie.protects_cookie("XSRF-TOKEN") {
        return Err(Error::custom(
            "web_cookie_encryption_missing",
            "Web CSRF 要求保护 XSRF Cookie",
        ));
    }
    Ok(xsrf
        .to_str()
        .ok()
        .and_then(|value| cookie.decrypt_xsrf_header(value)))
}

fn add_xsrf_cookie(config: &SessionConfig, jar: &CookieJar, token: String) -> Result<()> {
    let mut options = CookieOptions {
        http_only: false,
        ..CookieOptions::default()
    };
    options.path = Some(config.path.clone());
    options.domain.clone_from(&config.domain);
    options.secure = config.secure;
    options.same_site = config.validate()?;
    let cookie = jar
        .factory()
        .make(
            "XSRF-TOKEN",
            token,
            i64::try_from(config.lifetime).map_err(Error::internal)?,
            options,
        )?
        .with_partitioned(config.partitioned);
    jar.queue(cookie)
}
