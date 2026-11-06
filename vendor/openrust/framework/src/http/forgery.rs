//! 默认 Web 请求伪造防护，按固定 Laravel `PreventRequestForgery` 顺序执行。

use crate::config::SessionConfig;
use crate::http::{CookieJar, CookieOptions, RequestInfo};
use crate::{Error, Result};
use axum::{
    body::Body,
    extract::{FromRequestParts, Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http::{Method, StatusCode};
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
    /// Laravel 默认策略：验证来源或 Session token，成功时排入 XSRF cookie。
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
    pub(crate) cookie: crate::http::cookie::CookieLayer,
    pub(crate) session: SessionConfig,
    pub(crate) session_cookie_name: String,
    pub(crate) testing_boot: bool,
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

/// Laravel 旧 middleware 名称；行为由同一默认 Web 防护实现。
#[deprecated(note = "使用 PreventRequestForgery")]
pub type VerifyCsrfToken = PreventRequestForgery;

pub(crate) async fn handle(
    State(runtime): State<ForgeryRuntime>,
    mut request: Request,
    next: Next,
) -> Response {
    match verify(&runtime, &mut request).await {
        Ok(()) => {
            request.extensions_mut().insert(CsrfVerified);
            let jar = request.extensions().get::<CookieJar>().cloned();
            let session = request
                .extensions()
                .get::<crate::session::Session>()
                .cloned();
            let mut response = next.run(request).await;
            if !runtime.policy.origin_only && runtime.policy.add_xsrf_cookie {
                let result = (|| {
                    let session = session.as_ref().ok_or_else(|| {
                        Error::custom("csrf_session_missing", "Web CSRF 要求已启动 Session")
                    })?;
                    let token = session
                        .token()?
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .ok_or_else(|| {
                            Error::custom(
                                "csrf_session_token_missing",
                                "Web CSRF Session token 不可用",
                            )
                        })?;
                    let jar = jar.as_ref().ok_or_else(|| {
                        Error::custom("csrf_cookie_jar_missing", "Web CSRF 要求 CookieJar")
                    })?;
                    add_xsrf_cookie(&runtime.session, jar, token)
                })();
                if let Err(error) = result {
                    if let Some(jar) = &jar {
                        let _ = jar.flush_queued_cookies();
                    }
                    response = error.into_response();
                }
            }
            response
        }
        Err(error) => error.into_response(),
    }
}

async fn verify(runtime: &ForgeryRuntime, request: &mut Request) -> Result<()> {
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
        return Ok(());
    }
    let site = request
        .headers()
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok());
    if site == Some("same-origin") || (site == Some("same-site") && runtime.policy.allow_same_site)
    {
        return Ok(());
    }
    if runtime.policy.origin_only {
        return Err(Error::http_with_source(
            StatusCode::FORBIDDEN,
            "origin_mismatch",
            "Origin Mismatch",
            OriginMismatch,
        ));
    }
    let session = request
        .extensions()
        .get::<crate::session::Session>()
        .ok_or_else(|| Error::custom("csrf_session_missing", "Web CSRF 要求已启动 Session"))?;
    let expected = session
        .token()?
        .and_then(|token| token.as_str().map(str::to_owned));
    let (managed, replay) = crate::http::request::prepare_csrf_request(std::mem::replace(
        request,
        Request::new(Body::empty()),
    ))
    .await?;
    *request = replay;
    let input = managed
        .input("_token")?
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| !value.is_empty());
    let header = request
        .headers()
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let xsrf = request
        .headers()
        .get("x-xsrf-token")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| runtime.cookie.decrypt_xsrf_header(value));
    let token = input.or(header).or(xsrf);
    let valid = expected
        .as_deref()
        .zip(token.as_deref())
        .is_some_and(|(expected, supplied)| expected.as_bytes().ct_eq(supplied.as_bytes()).into());
    if valid {
        Ok(())
    } else {
        Err(Error::http_with_source(
            StatusCode::from_u16(419).map_err(Error::internal)?,
            "page_expired",
            "Page Expired",
            CsrfTokenMismatch,
        ))
    }
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
