use std::time::SystemTime;

use crate::{
    contracts::{routing::UrlGenerator as _, session::Session as _},
    http::{HeaderMap, RedirectResponse, Request, Response, StatusCode},
};

use super::{RouteParameter, SignatureMode, UrlError, UrlGenerator, UrlResult};

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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Redirector {
    pub fn new(urls: UrlGenerator) -> Self {
        Self { urls }
    }

    pub fn to(&self, path: &str, options: RedirectOptions) -> UrlResult<RedirectResponse> {
        redirect_response(&self.urls.to(path, &[], None)?, options)
    }

    pub fn secure(&self, path: &str, options: RedirectOptions) -> UrlResult<RedirectResponse> {
        redirect_response(&self.urls.to(path, &[], Some(true))?, options)
    }

    pub fn route(
        &self,
        name: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        redirect_response(&self.urls.route(name, parameters, true)?, options)
    }

    pub fn action(
        &self,
        action: &str,
        parameters: &[RouteParameter],
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        redirect_response(&self.urls.action(action, parameters, true)?, options)
    }

    pub fn signedRoute(
        &self,
        name: &str,
        parameters: &[RouteParameter],
        expiration: Option<SystemTime>,
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
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
    ) -> UrlResult<RedirectResponse> {
        self.signedRoute(name, parameters, Some(expiration), options)
    }

    pub fn refresh(&self, options: RedirectOptions) -> UrlResult<RedirectResponse> {
        redirect_response(&self.urls.current()?, options)
    }

    pub fn away(&self, target: &str, options: RedirectOptions) -> UrlResult<RedirectResponse> {
        redirect_response(target, options)
    }

    pub fn back(
        &self,
        request: &Request,
        fallback: Option<&str>,
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        let target = self.urls.forRequest(request).previous(fallback)?;
        redirect_response(&target, options)
    }

    pub fn guest(
        &self,
        request: &mut Request,
        path: &str,
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        let urls = self.urls.forRequest(request);
        let response = Self::new(urls.clone()).to(path, options)?;
        let target = if request.matched_route
            && request.method() == crate::http::Method::GET
            && !request.expectsJson()
        {
            urls.full()?
        } else {
            urls.previous(None)?
        };
        self.setIntendedUrl(request, &target)?;
        Ok(response)
    }

    pub fn intended(
        &self,
        request: &mut Request,
        fallback: &str,
        options: RedirectOptions,
    ) -> UrlResult<RedirectResponse> {
        let urls = self.urls.forRequest(request);
        let session = request.session_mut().map_err(UrlError::Session)?;
        let target = session
            .get("url.intended")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        session.forget(&["url.intended"]);
        Self::new(urls).to(target.as_deref().unwrap_or(fallback), options)
    }

    pub fn getIntendedUrl<'a>(&self, request: &'a Request) -> UrlResult<Option<&'a str>> {
        Ok(request
            .session()
            .map_err(UrlError::Session)?
            .get("url.intended")
            .and_then(serde_json::Value::as_str))
    }

    pub fn setIntendedUrl(&self, request: &mut Request, target: &str) -> UrlResult<()> {
        request
            .session_mut()
            .map_err(UrlError::Session)?
            .put("url.intended", target.into())
            .map_err(UrlError::Session)
    }
}

pub(crate) fn redirect_response(
    target: &str,
    options: RedirectOptions,
) -> UrlResult<RedirectResponse> {
    RedirectResponse::new(target, options.status, options.headers)
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

    fn response(&self, request: &Request) -> UrlResult<RedirectResponse> {
        let mut target = String::new();
        let mut pattern = self.target.as_str();
        while let Some((literal, tail)) = pattern.split_once('{') {
            let (key, rest) = tail.split_once('}').ok_or(UrlError::InvalidRedirect)?;
            let (key, optional) = key
                .strip_suffix('?')
                .map_or((key, false), |key| (key, true));
            let key = key.split_once(':').map_or(key, |(key, _)| key);
            if let Some(value) = request.route(key) {
                target.push_str(literal);
                target.push_str(&super::url_generator::encode(value));
            } else if optional {
                target.push_str(literal.strip_suffix('/').unwrap_or(literal));
            } else {
                return Err(UrlError::MissingParameter);
            }
            pattern = rest;
        }
        target.push_str(pattern);
        if target.is_empty() && self.target.starts_with('/') {
            target.push('/');
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
        Box::pin(async move { Ok(self.response(request)?.into()) })
    }
}
