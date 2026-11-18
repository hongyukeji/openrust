use std::error::Error;

use ::http::{
    HeaderValue,
    header::{CONTENT_TYPE, LOCATION},
};
use serde_json::{Map, Value};

use crate::{
    contracts::session::Session as _,
    cookie::Cookie,
    routing::{UrlError, UrlResult, valid_http_url},
    support::{Arr, MessageBag, Uri, data_forget},
};

use super::{Body, HeaderMap, HttpError, Request, Response, StatusCode};

/// An HTTP redirect with fluent session flashing.
pub struct RedirectResponse(Response);

#[allow(non_snake_case, reason = "Matches Laravel's RedirectResponse")]
impl RedirectResponse {
    pub(crate) fn new(target: &str, status: StatusCode, headers: HeaderMap) -> UrlResult<Self> {
        if !matches!(status.as_u16(), 201 | 301 | 302 | 303 | 307 | 308) {
            return Err(UrlError::InvalidRedirect);
        }
        let location = redirect_location(target)?;
        Ok(Self(
            Response::new(status, redirect_body(target))
                .withHeaders(headers)
                .header(
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                )
                .header(LOCATION, location),
        ))
    }

    /// Attaches an exception to the response.
    pub fn withException(self, error: impl Error + Send + Sync + 'static) -> Self {
        Self(self.0.withException(error))
    }

    pub fn exception(&self) -> Option<&(dyn Error + Send + Sync + 'static)> {
        self.0.exception()
    }

    pub fn throwResponse(self) -> HttpError {
        self.0.throwResponse()
    }

    /// Attaches a cookie while keeping the redirect available for further changes.
    pub fn cookie(self, cookie: &Cookie, request: &Request) -> Result<Self, HttpError> {
        self.withCookie(cookie, request)
    }

    pub fn withCookie(self, cookie: &Cookie, request: &Request) -> Result<Self, HttpError> {
        self.0.withCookie(cookie, request).map(Self)
    }

    pub fn withCookies<'a>(
        self,
        cookies: impl IntoIterator<Item = &'a Cookie>,
        request: &Request,
    ) -> Result<Self, HttpError> {
        self.0.withCookies(cookies, request).map(Self)
    }

    pub fn withoutCookie(
        self,
        name: &str,
        path: Option<&str>,
        domain: Option<&str>,
        request: &Request,
    ) -> Result<Self, HttpError> {
        self.0.withoutCookie(name, path, domain, request).map(Self)
    }

    pub fn withoutCookies<'a>(
        self,
        names: impl IntoIterator<Item = &'a str>,
        path: Option<&str>,
        domain: Option<&str>,
        request: &Request,
    ) -> Result<Self, HttpError> {
        self.0
            .withoutCookies(names, path, domain, request)
            .map(Self)
    }

    /// Replaces the target's fragment, keeping its path and query.
    pub fn withFragment(self, fragment: &str) -> UrlResult<Self> {
        let current = self.target()?;
        let base = current.split_once('#').map_or(current, |(base, _)| base);
        if base.is_empty() {
            return Err(UrlError::InvalidRedirect);
        }
        let fragment = fragment
            .split_once('#')
            .map_or(fragment, |(_, fragment)| fragment);
        let target = format!("{base}#{fragment}");
        self.with_target(&target)
    }

    /// Removes the target's fragment, including the separator.
    pub fn withoutFragment(self) -> UrlResult<Self> {
        let target = self.target()?;
        let Some((base, _)) = target.split_once('#') else {
            return Ok(self);
        };
        let base = base.to_owned();
        self.with_target(&base)
    }

    /// Keeps the target when its origin matches the request, or uses the supplied fallback.
    pub fn enforceSameOrigin(
        self,
        request: &Request,
        fallback: &str,
        validate_scheme: bool,
        validate_port: bool,
    ) -> Result<Self, HttpError> {
        let target = Uri::of(self.target()?).map_err(|_| UrlError::InvalidRedirect)?;
        let current =
            Uri::of(&request.schemeAndHttpHost()?).map_err(|_| HttpError::InvalidRequest)?;
        if target.host() == current.host()
            && (!validate_scheme || target.scheme() == current.scheme())
            && (!validate_port || target.port() == current.port())
        {
            return Ok(self);
        }
        Ok(self.with_target(fallback)?)
    }

    fn target(&self) -> UrlResult<&str> {
        self.0
            .headers()
            .get(LOCATION)
            .and_then(|location| str::from_utf8(location.as_bytes()).ok())
            .ok_or(UrlError::InvalidRedirect)
    }

    fn with_target(mut self, target: &str) -> UrlResult<Self> {
        let location = redirect_location(target)?;
        self.0.parts_mut().headers.insert(LOCATION, location);
        self.0.body = redirect_body(target);
        Ok(self)
    }

    /// Flashes a value to the next request's session.
    pub fn with(self, request: &mut Request, key: &str, value: Value) -> UrlResult<Self> {
        request
            .session_mut()
            .and_then(|session| session.flash(key, value))
            .map_err(UrlError::Session)?;
        Ok(self)
    }

    /// Flashes supplied input, or reads the current request's input when omitted.
    pub async fn withInput(
        self,
        request: &mut Request,
        input: Option<Value>,
    ) -> Result<Self, HttpError> {
        let input = match input {
            Some(input) => input,
            None => request_input(request).await?,
        };
        Ok(self.flash_input(request, input)?)
    }

    /// Flashes only the selected input paths.
    pub async fn onlyInput(self, request: &mut Request, keys: &[&str]) -> Result<Self, HttpError> {
        let input = request_input(request).await?;
        let mut selected = Value::Object(Map::new());
        for key in keys {
            if let Some(value) = Arr::get(&input, key) {
                Arr::set(&mut selected, key, value.clone())
                    .map_err(|_| HttpError::InvalidRequest)?;
            }
        }
        Ok(self.flash_input(request, selected)?)
    }

    /// Flashes input after removing the specified paths.
    pub async fn exceptInput(
        self,
        request: &mut Request,
        keys: &[&str],
    ) -> Result<Self, HttpError> {
        let mut input = request_input(request).await?;
        for key in keys {
            if let Some(fields) = input.as_object_mut() {
                fields.remove(*key);
            }
            data_forget(&mut input, key).map_err(|_| HttpError::InvalidRequest)?;
        }
        Ok(self.flash_input(request, input)?)
    }

    /// Flashes an error bag without replacing other named bags.
    pub fn withErrors(
        self,
        request: &mut Request,
        errors: &MessageBag,
        bag: &str,
    ) -> UrlResult<Self> {
        request
            .session_mut()
            .and_then(|session| session.flashErrors(bag, errors))
            .map_err(UrlError::Session)?;
        Ok(self)
    }

    fn flash_input(self, request: &mut Request, input: Value) -> UrlResult<Self> {
        request
            .session_mut()
            .and_then(|session| session.flashInput(input))
            .map_err(UrlError::Session)?;
        Ok(self)
    }
}

impl From<RedirectResponse> for Response {
    fn from(redirect: RedirectResponse) -> Self {
        redirect.0
    }
}

fn redirect_location(target: &str) -> UrlResult<HeaderValue> {
    if target.is_empty()
        || target.contains('\\')
        || target.bytes().any(|byte| byte.is_ascii_control())
        || target.starts_with("//")
        || (target
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|segment| segment.contains(':'))
            && valid_http_url(target).is_err())
    {
        return Err(UrlError::InvalidRedirect);
    }
    HeaderValue::from_str(target).map_err(|_| UrlError::InvalidRedirect)
}

fn redirect_body(target: &str) -> Body {
    let escaped = target
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&#39;");
    Body::from(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="refresh" content="0;url={escaped}"><title>Redirecting</title></head><body><a href="{escaped}">Redirecting</a></body></html>"#
    ))
}

async fn request_input(request: &mut Request) -> Result<Value, HttpError> {
    Ok(request.input(None).await?.unwrap_or_default())
}
