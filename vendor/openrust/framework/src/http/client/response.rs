use http::{HeaderMap, StatusCode, header};
use serde::de::DeserializeOwned;

use super::{Error, Result, transport_error};

/// A response whose body is available for repeated inspection.
#[derive(Clone)]
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: axum::body::Bytes,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Response")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's HTTP response API")]
impl Response {
    pub fn new(status: StatusCode, headers: HeaderMap, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers,
            body: axum::body::Bytes::from(body.into()),
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn header(&self, name: &str) -> Option<&http::HeaderValue> {
        self.headers.get(name)
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(self.body()).map_err(Error::InvalidJson)
    }

    pub fn successful(&self) -> bool {
        self.status.is_success()
    }

    pub fn redirect(&self) -> bool {
        self.status.is_redirection()
    }

    pub fn clientError(&self) -> bool {
        self.status.is_client_error()
    }

    pub fn serverError(&self) -> bool {
        self.status.is_server_error()
    }

    pub fn failed(&self) -> bool {
        self.clientError() || self.serverError()
    }

    pub fn ok(&self) -> bool {
        self.status == StatusCode::OK
    }

    pub fn created(&self) -> bool {
        self.status == StatusCode::CREATED
    }

    pub fn accepted(&self) -> bool {
        self.status == StatusCode::ACCEPTED
    }

    pub fn noContent(&self) -> bool {
        self.status == StatusCode::NO_CONTENT
    }

    pub fn movedPermanently(&self) -> bool {
        self.status == StatusCode::MOVED_PERMANENTLY
    }

    pub fn found(&self) -> bool {
        self.status == StatusCode::FOUND
    }

    pub fn badRequest(&self) -> bool {
        self.status == StatusCode::BAD_REQUEST
    }

    pub fn unauthorized(&self) -> bool {
        self.status == StatusCode::UNAUTHORIZED
    }

    pub fn paymentRequired(&self) -> bool {
        self.status == StatusCode::PAYMENT_REQUIRED
    }

    pub fn forbidden(&self) -> bool {
        self.status == StatusCode::FORBIDDEN
    }

    pub fn notFound(&self) -> bool {
        self.status == StatusCode::NOT_FOUND
    }

    pub fn requestTimeout(&self) -> bool {
        self.status == StatusCode::REQUEST_TIMEOUT
    }

    pub fn conflict(&self) -> bool {
        self.status == StatusCode::CONFLICT
    }

    pub fn unprocessableEntity(&self) -> bool {
        self.status == StatusCode::UNPROCESSABLE_ENTITY
    }

    pub fn tooManyRequests(&self) -> bool {
        self.status == StatusCode::TOO_MANY_REQUESTS
    }

    pub fn throw(self) -> Result<Self> {
        if self.failed() {
            return Err(Error::RequestFailed(Box::new(self)));
        }
        Ok(self)
    }

    pub(super) async fn from_transport(
        mut response: reqwest::Response,
        limit: usize,
    ) -> Result<Self> {
        let mut body = Vec::new();
        // HEAD and 304 may advertise the size of a representation they do not send.
        if response
            .content_length()
            .is_some_and(|size| size > limit as u64)
        {
            return Err(Error::BodyTooLarge);
        }
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if chunk.len() > limit.saturating_sub(body.len()) {
                return Err(Error::BodyTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let headers = std::mem::take(response.headers_mut());
        Ok(Self::new(response.status(), headers, body))
    }

    pub fn from_json(status: StatusCode, value: &impl serde::Serialize) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        Ok(Self::new(
            status,
            headers,
            serde_json::to_vec(value).map_err(Error::InvalidJson)?,
        ))
    }
}
