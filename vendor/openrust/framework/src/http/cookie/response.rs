//! 响应保留 Cookie 值对象，避免先 URL 编码再加密改变原值。
use super::{Cookie, CookieFactory, CookieOptions};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};

#[derive(Clone, Default)]
pub(crate) struct ResponseCookies(pub(crate) Vec<(HeaderValue, Cookie)>);

/// 将 Cookie 属性组合到任意既有响应，不改变响应正文与封装职责。
pub trait CookieResponseExt: IntoResponse + Sized {
    /// 附加一个 Cookie，同 domain/path/name 覆盖而保留原位置。
    fn with_cookie(self, cookie: Cookie) -> crate::Result<CookieResponse<Self>> {
        self.with_cookies([cookie])
    }
    /// `with_cookie` 的同义入口。
    fn cookie(self, cookie: Cookie) -> crate::Result<CookieResponse<Self>> {
        self.with_cookie(cookie)
    }
    /// 附加一组 Cookie；资源或属性错误不产生部分响应。
    fn with_cookies<I: IntoIterator<Item = Cookie>>(
        self,
        cookies: I,
    ) -> crate::Result<CookieResponse<Self>> {
        let mut values = Vec::new();
        let mut bytes = 0usize;
        for cookie in cookies {
            if values.len() >= 128 {
                return Err(super::cookie_limits_error());
            }
            bytes = bytes
                .checked_add(cookie.to_header(false)?.as_bytes().len())
                .ok_or_else(super::cookie_limits_error)?;
            if bytes > 64 * 1024 {
                return Err(super::cookie_limits_error());
            }
            values.push(cookie);
        }
        Ok(CookieResponse {
            inner: self,
            cookies: values,
        })
    }
    /// 附加调用方工厂创建的删除 Cookie；对应上游接受 Cookie 对象的分支。
    fn without_cookie(self, expired: Cookie) -> crate::Result<CookieResponse<Self>> {
        self.with_cookie(expired)
    }
    /// 显式使用应用工厂默认值创建删除 Cookie，不隐式读取全局容器。
    fn without_cookie_named(
        self,
        factory: &CookieFactory,
        name: &str,
        options: CookieOptions,
    ) -> crate::Result<CookieResponse<Self>> {
        self.with_cookie(factory.forget(name, options)?)
    }
}
impl<T: IntoResponse> CookieResponseExt for T {}

/// 保留原始 Cookie 对象的响应组合器。
pub struct CookieResponse<T> {
    inner: T,
    cookies: Vec<Cookie>,
}
impl<T: IntoResponse> IntoResponse for CookieResponse<T> {
    fn into_response(self) -> Response {
        let mut response = self.inner.into_response();
        match attach(&mut response, self.cookies, false) {
            Ok(()) => response,
            Err(error) => error.into_response(),
        }
    }
}

pub(crate) fn models(response: &Response) -> crate::Result<Vec<Cookie>> {
    models_with_context(response, &crate::support::CalendarContext::default())
}

pub(crate) fn models_with_context(
    response: &Response,
    context: &crate::support::CalendarContext,
) -> crate::Result<Vec<Cookie>> {
    let known = response.extensions().get::<ResponseCookies>();
    let mut cookies = Vec::new();
    let mut bytes = 0usize;
    for (index, wire) in response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .enumerate()
    {
        if cookies.len() >= 128 {
            return Err(super::cookie_limits_error());
        }
        bytes = bytes
            .checked_add(wire.as_bytes().len())
            .ok_or_else(super::cookie_limits_error)?;
        if bytes > 64 * 1024 {
            return Err(super::cookie_limits_error());
        }
        let cookie = if let Some((_, cookie)) =
            known.and_then(|entries| entries.0.get(index).filter(|(header, _)| header == wire))
        {
            cookie.clone()
        } else {
            Cookie::from_header_with_context(
                std::str::from_utf8(wire.as_bytes()).map_err(|_| invalid_header())?,
                context,
            )?
        };
        insert_model(&mut cookies, cookie);
    }
    Ok(cookies)
}

pub(crate) fn attach(
    response: &mut Response,
    extra: impl IntoIterator<Item = Cookie>,
    secure: bool,
) -> crate::Result<()> {
    let mut cookies = models(response)?;
    for cookie in extra {
        insert_model(&mut cookies, cookie);
    }
    replace(response, cookies, secure)
}

pub(crate) fn replace(
    response: &mut Response,
    cookies: Vec<Cookie>,
    secure: bool,
) -> crate::Result<()> {
    let mut encoded = Vec::new();
    let mut bytes = 0usize;
    for cookie in cookies {
        if encoded.len() >= 128 {
            return Err(super::cookie_limits_error());
        }
        let header = cookie.to_response_header(secure)?;
        bytes = bytes
            .checked_add(header.as_bytes().len())
            .ok_or_else(super::cookie_limits_error)?;
        if bytes > 64 * 1024 {
            return Err(super::cookie_limits_error());
        }
        encoded.push((header, cookie));
    }
    response.headers_mut().remove(header::SET_COOKIE);
    for (header, _) in &encoded {
        response
            .headers_mut()
            .append(header::SET_COOKIE, header.clone());
    }
    response.extensions_mut().insert(ResponseCookies(encoded));
    Ok(())
}

pub(crate) fn insert_model(cookies: &mut Vec<Cookie>, value: Cookie) {
    let same_group = |existing: &Cookie| {
        existing.domain().unwrap_or_default() == value.domain().unwrap_or_default()
            && existing.path() == value.path()
    };
    if let Some(index) = cookies
        .iter()
        .position(|existing| same_group(existing) && existing.name() == value.name())
    {
        cookies[index] = value;
    } else if let Some(index) = cookies.iter().rposition(same_group) {
        cookies.insert(index + 1, value);
    } else if let Some(index) = cookies.iter().rposition(|existing| {
        existing.domain().unwrap_or_default() == value.domain().unwrap_or_default()
    }) {
        cookies.insert(index + 1, value);
    } else {
        cookies.push(value);
    }
}
fn invalid_header() -> crate::Error {
    crate::Error::custom("cookie_parameters_invalid", "响应 Cookie 无效")
}
