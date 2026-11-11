use axum::body::Bytes;
use http::{HeaderName, HeaderValue, header::CONTENT_TYPE};
use serde::Serialize;

use super::{Body, HeaderMap, HttpError, Method, ResponseParts, StatusCode};

/// An outgoing HTTP response with owned metadata and body.
pub struct Response {
    parts: ResponseParts,
    body: Body,
}

impl Response {
    pub fn new(status: StatusCode, body: Body) -> Self {
        let mut response = ::http::Response::new(body);
        *response.status_mut() = status;
        let (parts, body) = response.into_parts();
        Self::from_parts(parts, body)
    }

    pub fn empty(status: StatusCode) -> Self {
        Self::new(status, Body::empty())
    }

    pub fn json(status: StatusCode, data: impl Serialize) -> Result<Self, HttpError> {
        if status.is_informational()
            || matches!(status, StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED)
        {
            return Ok(Self::empty(status));
        }
        let bytes = serde_json::to_vec(&data).map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?;
        Ok(Self::new(status, Body::from(bytes))
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json")))
    }

    pub fn stream<T, E>(
        status: StatusCode,
        stream: impl futures_util::Stream<Item = Result<T, E>> + Send + 'static,
    ) -> Self
    where
        T: Into<Bytes> + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
    {
        Self::new(status, Body::from_stream(stream))
    }

    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.parts.headers.insert(name, value);
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Response API")]
    pub fn withHeaders(mut self, headers: HeaderMap) -> Self {
        self.parts.headers.extend(headers);
        self
    }

    /// Attaches a cookie using the given request's HTTPS setting.
    pub fn cookie(
        mut self,
        cookie: &crate::cookie::Cookie,
        request: &super::Request,
    ) -> Result<Self, HttpError> {
        self.setCookie(cookie, request.is_secure())?;
        Ok(self)
    }

    /// Sets a cookie, replacing an existing cookie with the same name, path and domain.
    #[allow(non_snake_case, reason = "Matches Laravel's response cookie API")]
    pub fn setCookie(
        &mut self,
        cookie: &crate::cookie::Cookie,
        secure_default: bool,
    ) -> Result<(), HttpError> {
        use ::http::header::SET_COOKIE;

        let header = cookie.to_header(secure_default)?;
        let identity = cookie_identity(&header);
        let headers = &mut self.parts.headers;
        let existing = headers
            .get_all(SET_COOKIE)
            .iter()
            .filter(|current| cookie_identity(current) != identity)
            .cloned()
            .collect::<Vec<_>>();
        headers.remove(SET_COOKIE);
        for current in existing {
            headers.append(SET_COOKIE, current);
        }
        headers.append(SET_COOKIE, header.clone());
        let values = self
            .parts
            .extensions
            .get_or_insert_default::<crate::cookie::ResponseCookies>();
        values
            .0
            .retain(|(current, _)| cookie_identity(current) != identity);
        values.0.push((header, cookie.clone()));
        Ok(())
    }

    pub(crate) fn with_allow(mut self, methods: &[Method], separator: &str) -> Self {
        let allow = methods
            .iter()
            .map(Method::as_str)
            .collect::<Vec<_>>()
            .join(separator);
        self.parts.headers.insert(
            ::http::header::ALLOW,
            ::http::HeaderValue::from_str(&allow).expect("valid HTTP methods"),
        );
        self
    }

    pub fn from_parts(parts: ResponseParts, body: Body) -> Self {
        Self { parts, body }
    }
    pub fn parts(&self) -> &ResponseParts {
        &self.parts
    }
    pub fn parts_mut(&mut self) -> &mut ResponseParts {
        &mut self.parts
    }
    pub fn take_body(&mut self) -> Body {
        std::mem::replace(&mut self.body, Body::empty())
    }
    pub fn into_parts(mut self) -> (ResponseParts, Body) {
        self.parts
            .extensions
            .remove::<crate::cookie::ResponseCookies>();
        (self.parts, self.body)
    }
    pub fn status(&self) -> StatusCode {
        self.parts.status
    }
    pub fn headers(&self) -> &HeaderMap {
        &self.parts.headers
    }
}

fn cookie_identity(header: &HeaderValue) -> Option<(&str, &str, &str)> {
    let mut parts = header.to_str().ok()?.split(';');
    let (name, _) = parts.next()?.split_once('=')?;
    let mut domain = "";
    let mut path = "/";
    for attribute in parts {
        if let Some((key, value)) = attribute.trim().split_once('=') {
            if key.eq_ignore_ascii_case("domain") {
                domain = value.trim_start_matches('.');
            } else if key.eq_ignore_ascii_case("path") {
                path = value;
            }
        }
    }
    Some((name, domain, path))
}
