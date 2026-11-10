use super::{Body, HeaderMap, Method, ResponseParts, StatusCode};

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
    pub fn into_parts(self) -> (ResponseParts, Body) {
        (self.parts, self.body)
    }
    pub fn status(&self) -> StatusCode {
        self.parts.status
    }
    pub fn headers(&self) -> &HeaderMap {
        &self.parts.headers
    }
}
