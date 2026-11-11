use std::time::SystemTime;

use ::http::{
    HeaderValue,
    header::{CONTENT_TYPE, LOCATION},
};

use crate::{
    contracts::{routing::UrlGenerator as _, session::Session as _},
    http::{Body, HeaderMap, Request, Response, StatusCode},
};

use super::{
    RouteParameter, SignatureMode, UrlError, UrlGenerator, UrlResult, url_generator::valid_http_url,
};

/// Creates HTTP redirects through the application's URL generator.
pub struct Redirector {
    urls: UrlGenerator,
}

/// Response settings for a redirect.
pub struct RedirectOptions {
    pub status: StatusCode,
    pub headers: HeaderMap,
}

impl Default for RedirectOptions {
    fn default() -> Self {
        Self {
            status: StatusCode::FOUND,
            headers: HeaderMap::new(),
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Redirector")]
impl Redirector {
    pub fn new(urls: UrlGenerator) -> Self {
        Self { urls }
    }

    pub fn to(&self, path: &str, options: RedirectOptions) -> UrlResult<Response> {
        redirect_response(&self.urls.to(path, &[], None)?, options)
    }

    pub fn secure(&self, path: &str, options: RedirectOptions) -> UrlResult<Response> {
        redirect_response(&self.urls.to(path, &[], Some(true))?, options)
    }

    pub fn route(
        &self,
        name: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        redirect_response(&self.urls.route(name, parameters, true)?, options)
    }

    pub fn signedRoute(
        &self,
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        let target =
            self.urls
                .signedRoute(name, parameters, expiration, SignatureMode::Absolute)?;
        redirect_response(&target, options)
    }

    pub fn temporarySignedRoute(
        &self,
        name: &str,
        expiration: SystemTime,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        self.signedRoute(name, parameters, Some(expiration), options)
    }

    pub fn refresh(&self, options: RedirectOptions) -> UrlResult<Response> {
        redirect_response(&self.urls.current()?, options)
    }

    pub fn away(&self, target: &str, options: RedirectOptions) -> UrlResult<Response> {
        valid_http_url(target).map_err(|_| UrlError::InvalidRedirect)?;
        redirect_response(target, options)
    }

    pub fn back(
        &self,
        request: &Request,
        fallback: Option<&str>,
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        let target = self.urls.forRequest(request).previous(fallback)?;
        redirect_response(&target, options)
    }

    pub fn guest(
        &self,
        request: &mut Request,
        path: &str,
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        let urls = self.urls.forRequest(request);
        let response = Self::new(urls.clone()).to(path, options)?;
        let target = if request.matched_route
            && request.method() == crate::http::Method::GET
            && !request.expectsJson()
        {
            let path = request
                .uri()
                .path_and_query()
                .map_or("/", ::http::uri::PathAndQuery::as_str);
            urls.to(path, &[], None)?
        } else {
            urls.previous(None)?
        };
        request
            .session_mut()
            .map_err(UrlError::Session)?
            .put("url.intended", target.into())
            .map_err(UrlError::Session)?;
        Ok(response)
    }

    pub fn intended(
        &self,
        request: &mut Request,
        fallback: &str,
        options: RedirectOptions,
    ) -> UrlResult<Response> {
        let urls = self.urls.forRequest(request);
        let session = request.session_mut().map_err(UrlError::Session)?;
        let target = session
            .get("url.intended")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback);
        let response = Self::new(urls).to(target, options)?;
        session.forget(&["url.intended"]);
        Ok(response)
    }
}

pub(super) fn redirect_response(target: &str, options: RedirectOptions) -> UrlResult<Response> {
    if target.is_empty()
        || target.contains('\\')
        || target.bytes().any(|byte| byte.is_ascii_control())
        || target.starts_with("//")
        || (target
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|segment| segment.contains(':'))
            && valid_http_url(target).is_err())
        || !matches!(options.status.as_u16(), 201 | 301 | 302 | 303 | 307 | 308)
    {
        return Err(UrlError::InvalidRedirect);
    }
    let header = HeaderValue::from_str(target).map_err(|_| UrlError::InvalidRedirect)?;
    let escaped = target
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&#39;");
    let body = format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="refresh" content="0;url={escaped}"><title>Redirecting</title></head><body><a href="{escaped}">Redirecting</a></body></html>"#
    );
    Ok(Response::new(options.status, Body::from(body))
        .withHeaders(options.headers)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )
        .header(LOCATION, header))
}

pub(super) struct RedirectRoute {
    target: String,
    status: StatusCode,
}

impl RedirectRoute {
    pub(super) fn new(target: &str, status: StatusCode) -> Self {
        Self {
            target: target.to_owned(),
            status,
        }
    }

    fn response(&self, request: &Request) -> UrlResult<Response> {
        let mut target = self.target.clone();
        let mut pattern = self.target.as_str();
        while let Some((_, tail)) = pattern.split_once('{') {
            let (key, rest) = tail.split_once('}').ok_or(UrlError::InvalidRedirect)?;
            let value = request.route(key).ok_or(UrlError::MissingParameter)?;
            target = target.replace(&format!("{{{key}}}"), &super::url_generator::encode(value));
            pattern = rest;
        }
        redirect_response(
            &target,
            RedirectOptions {
                status: self.status,
                ..Default::default()
            },
        )
    }
}

impl crate::contracts::routing::Handler for RedirectRoute {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> crate::http::HttpFuture<'a, Result<Response, crate::http::HttpError>> {
        Box::pin(async move { Ok(self.response(request)?) })
    }
}
