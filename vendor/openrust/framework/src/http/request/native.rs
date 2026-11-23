use axum::body::Bytes;
use http::{Extensions, Request as NativeRequest, Uri, Version};

use crate::http::{Body, HttpError, Request};

impl From<NativeRequest<Body>> for Request {
    fn from(request: NativeRequest<Body>) -> Self {
        let (parts, body) = request.into_parts();
        Self::from_parts(parts, body)
    }
}

impl From<Request> for NativeRequest<Body> {
    fn from(request: Request) -> Self {
        let (parts, body) = request.into_parts();
        Self::from_parts(parts, body)
    }
}

impl Request {
    /// Returns the transport protocol version.
    pub fn version(&self) -> Version {
        self.parts.version
    }

    /// Returns the native request extensions.
    pub fn extensions(&self) -> &Extensions {
        &self.parts.extensions
    }

    /// Returns the transport URI before trusted-proxy rewriting.
    #[allow(
        non_snake_case,
        reason = "Pairs with the original request metadata API"
    )]
    pub fn original_uri(&self) -> &Uri {
        &self.parts.uri
    }

    /// Builds an independent native request with the transport URI and effective method.
    /// The bounded body cache remains available to the framework request.
    #[allow(
        non_snake_case,
        reason = "Matches application-facing request method names"
    )]
    pub async fn to_http_request(&mut self) -> Result<NativeRequest<Bytes>, HttpError> {
        let body = self.body_bytes().await?;
        Ok(NativeRequest::from_parts(self.parts.clone(), body))
    }
}
