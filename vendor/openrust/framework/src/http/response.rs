use std::{error::Error, sync::Arc};

use axum::body::Bytes;
use http::{
    HeaderName, HeaderValue,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    cookie::Cookie,
    support::{Collection, Fluent, Stringable, facades},
};

use super::{
    Body, HeaderMap, HttpError, HttpFuture, Method, Request, ResponseParts, StatusCode,
    abortResponse,
};

#[derive(Clone)]
struct ResponseException(Arc<dyn Error + Send + Sync>);

/// 保留统一响应类型中原始正文的类别，供生命周期消费者检查。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseKind {
    Stream,
    File,
}

/// An outgoing HTTP response with owned metadata and body.
pub struct Response {
    parts: ResponseParts,
    pub(super) body: Body,
    pub(super) content: Option<Box<Bytes>>,
    pub(super) binary_file: Option<super::binary_file_response::BinaryFile>,
}

/// Converts a handler's successful return value into an HTTP response.
pub trait ResponseOutput {
    fn into_response(self) -> Result<Response, HttpError>;
}

impl<T: Into<Response>> ResponseOutput for T {
    fn into_response(self) -> Result<Response, HttpError> {
        Ok(self.into())
    }
}

fn html(body: Bytes) -> Response {
    Response::buffered(StatusCode::OK, body).header(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    )
}

impl From<&'static str> for Response {
    fn from(text: &'static str) -> Self {
        html(Bytes::from(text))
    }
}

impl From<String> for Response {
    fn from(text: String) -> Self {
        html(Bytes::from(text))
    }
}

impl From<Stringable> for Response {
    fn from(text: Stringable) -> Self {
        Self::from(text.to_string())
    }
}

fn structured_json(data: impl Serialize) -> Result<Response, HttpError> {
    Response::json_with(StatusCode::OK, || {
        let mut bytes = Vec::new();
        super::json_formatter::JsonFormatter::DEFAULT.write(&mut bytes, &data)?;
        Ok(bytes)
    })
}

impl ResponseOutput for Value {
    fn into_response(self) -> Result<Response, HttpError> {
        structured_json(self)
    }
}

impl ResponseOutput for Map<String, Value> {
    fn into_response(self) -> Result<Response, HttpError> {
        structured_json(self)
    }
}

impl<T: Serialize> ResponseOutput for Vec<T> {
    fn into_response(self) -> Result<Response, HttpError> {
        structured_json(self)
    }
}

impl<T: Serialize, const N: usize> ResponseOutput for [T; N] {
    fn into_response(self) -> Result<Response, HttpError> {
        structured_json(self.as_slice())
    }
}

impl<T: Serialize> ResponseOutput for Collection<T> {
    fn into_response(self) -> Result<Response, HttpError> {
        structured_json(self)
    }
}

impl ResponseOutput for Fluent {
    fn into_response(self) -> Result<Response, HttpError> {
        structured_json(self)
    }
}

impl<B> From<http::Response<B>> for Response
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
    fn from(response: http::Response<B>) -> Self {
        let (parts, body) = response.into_parts();
        Self::from_parts(parts, Body::new(body))
    }
}

impl From<Response> for http::Response<Body> {
    fn from(response: Response) -> Self {
        let (parts, body) = response.into_parts();
        Self::from_parts(parts, body)
    }
}

impl Response {
    /// Attaches an exception for application code that inspects this response.
    #[allow(non_snake_case, reason = "Matches Laravel's Response API")]
    pub fn withException(mut self, error: impl Error + Send + Sync + 'static) -> Self {
        self.parts
            .extensions
            .insert(ResponseException(Arc::new(error)));
        self
    }

    pub fn exception(&self) -> Option<&(dyn Error + Send + Sync + 'static)> {
        self.parts
            .extensions
            .get::<ResponseException>()
            .map(|error| error.0.as_ref())
    }

    /// Returns an error that exits the handler with this response.
    #[allow(non_snake_case, reason = "Matches Laravel's Response API")]
    pub fn throwResponse(self) -> HttpError {
        abortResponse(self)
    }

    pub fn new(status: StatusCode, body: Body) -> Self {
        let mut response = ::http::Response::new(body);
        *response.status_mut() = status;
        let (parts, body) = response.into_parts();
        Self::from_parts(parts, body)
    }

    fn buffered(status: StatusCode, content: Bytes) -> Self {
        let mut response = Self::new(status, Body::from(content.clone()));
        response.content = Some(Box::new(content));
        response
    }

    /// Replaces the body with known bytes, preserving a shared diagnostic snapshot.
    #[allow(non_snake_case, reason = "Matches Laravel's Response API")]
    pub fn setContent(&mut self, content: impl Into<Bytes>) -> &mut Self {
        let content = content.into();
        self.body = Body::from(content.clone());
        self.content = Some(Box::new(content));
        self.binary_file = None;
        self.parts.extensions.remove::<ResponseKind>();
        self.parts
            .extensions
            .remove::<Arc<crate::view::RenderedView>>();
        self
    }

    /// Returns an already-buffered body without polling a stream.
    #[allow(non_snake_case, reason = "Matches Laravel's Response API")]
    pub fn getContent(&self) -> Option<&Bytes> {
        if self.binary_file.is_some() {
            None
        } else {
            self.content.as_deref()
        }
    }

    pub fn empty(status: StatusCode) -> Self {
        Self::buffered(status, Bytes::new())
    }

    /// Creates an empty 204 response. Use `empty` to select another status.
    #[allow(non_snake_case, reason = "Matches Laravel's response factory API")]
    pub fn noContent() -> Self {
        Self::empty(StatusCode::NO_CONTENT)
    }

    /// Returns Markdown source for the client to render.
    pub fn markdown(status: StatusCode, content: impl Into<String>) -> Self {
        if !Self::allows_body(status) {
            return Self::empty(status);
        }
        Self::buffered(status, Bytes::from(content.into())).header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/markdown; charset=utf-8"),
        )
    }

    /// Renders a view in the current request with the supplied status and headers.
    pub fn view(
        status: StatusCode,
        view: crate::view::View,
        headers: HeaderMap,
        request: &mut Request,
    ) -> HttpFuture<'_, Result<Self, HttpError>> {
        Box::pin(async move {
            let mut response = view.render(request).await?.withHeaders(headers);
            response.parts.status = status;
            if !Self::allows_body(status) {
                response.body = Body::empty();
                response.content = Some(Box::new(Bytes::new()));
                response = response.withoutHeaders([CONTENT_TYPE, CONTENT_LENGTH]);
            }
            Ok(response)
        })
    }

    pub fn json(status: StatusCode, data: impl Serialize) -> Result<Self, HttpError> {
        Self::json_with(status, || {
            let mut bytes = Vec::new();
            super::json_formatter::JsonFormatter::DEFAULT.write(&mut bytes, &data)?;
            Ok(bytes)
        })
    }

    pub(crate) fn json_with(
        status: StatusCode,
        encode: impl FnOnce() -> Result<Vec<u8>, HttpError>,
    ) -> Result<Self, HttpError> {
        if !Self::allows_body(status) {
            return Ok(Self::empty(status));
        }
        let bytes = encode()?;
        Ok(Self::buffered(status, Bytes::from(bytes))
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json")))
    }

    pub(crate) fn allows_body(status: StatusCode) -> bool {
        !status.is_informational()
            && !matches!(status, StatusCode::NO_CONTENT | StatusCode::NOT_MODIFIED)
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
            .with_kind(ResponseKind::Stream)
            .header(
                HeaderName::from_static("x-accel-buffering"),
                HeaderValue::from_static("no"),
            )
    }

    pub(super) fn with_kind(mut self, kind: ResponseKind) -> Self {
        self.parts.extensions.insert(kind);
        self
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

    #[allow(non_snake_case, reason = "Matches Laravel's Response API")]
    pub fn withoutHeader(mut self, name: HeaderName) -> Self {
        if name == ::http::header::SET_COOKIE {
            self.parts
                .extensions
                .remove::<crate::cookie::ResponseCookies>();
        }
        self.parts.headers.remove(name);
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Response API")]
    pub fn withoutHeaders(mut self, names: impl IntoIterator<Item = HeaderName>) -> Self {
        for name in names {
            self = self.withoutHeader(name);
        }
        self
    }

    /// Attaches a cookie using the given request's HTTPS setting.
    pub fn cookie(self, cookie: &Cookie, request: &Request) -> Result<Self, HttpError> {
        self.withCookie(cookie, request)
    }

    /// Attaches one cookie to the response.
    #[allow(non_snake_case, reason = "Matches Laravel's response cookie API")]
    pub fn withCookie(mut self, cookie: &Cookie, request: &Request) -> Result<Self, HttpError> {
        self.setCookie(cookie, request.is_secure())?;
        Ok(self)
    }

    /// Attaches cookies in order, replacing cookies with the same name, path and domain.
    #[allow(non_snake_case, reason = "Matches Laravel's response cookie API")]
    pub fn withCookies<'a>(
        mut self,
        cookies: impl IntoIterator<Item = &'a Cookie>,
        request: &Request,
    ) -> Result<Self, HttpError> {
        for cookie in cookies {
            self.setCookie(cookie, request.is_secure())?;
        }
        Ok(self)
    }

    /// Expires one cookie using the application's Cookie factory.
    #[allow(non_snake_case, reason = "Matches Laravel's response cookie API")]
    pub fn withoutCookie(
        self,
        name: &str,
        path: Option<&str>,
        domain: Option<&str>,
        request: &Request,
    ) -> Result<Self, HttpError> {
        self.withoutCookies([name], path, domain, request)
    }

    /// Expires cookies using the given path and domain or the application defaults.
    #[allow(non_snake_case, reason = "Matches Laravel's response cookie API")]
    pub fn withoutCookies<'a>(
        mut self,
        names: impl IntoIterator<Item = &'a str>,
        path: Option<&str>,
        domain: Option<&str>,
        request: &Request,
    ) -> Result<Self, HttpError> {
        for name in names {
            self.setCookie(
                &facades::Cookie::forget(name, path, domain)?,
                request.is_secure(),
            )?;
        }
        Ok(self)
    }

    /// Sets a cookie, replacing an existing cookie with the same name, path and domain.
    #[allow(non_snake_case, reason = "Matches Laravel's response cookie API")]
    pub fn setCookie(&mut self, cookie: &Cookie, secure_default: bool) -> Result<(), HttpError> {
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
        Self {
            parts,
            body,
            content: None,
            binary_file: None,
        }
    }
    pub fn parts(&self) -> &ResponseParts {
        &self.parts
    }
    pub fn parts_mut(&mut self) -> &mut ResponseParts {
        &mut self.parts
    }
    pub fn take_body(&mut self) -> Body {
        self.content = Some(Box::new(Bytes::new()));
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
