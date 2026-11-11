use std::collections::BTreeMap;

use ::http::{HeaderValue, header::CONTENT_TYPE};
use serde::Serialize;

use super::{Body, HttpError, Response, StatusCode};

/// Pagination metadata for an API response. Page numbers start at one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PageMeta {
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
    pub total_pages: u64,
}

/// An opt-in JSON envelope for application API responses.
#[derive(Debug, Clone, Serialize)]
pub struct ApiResponse<T> {
    success: bool,
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<PageMeta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<BTreeMap<String, Vec<String>>>,
    #[serde(skip)]
    status: StatusCode,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            code: "ok".into(),
            message: "ok".into(),
            data: Some(data),
            meta: None,
            errors: None,
            status: StatusCode::OK,
        }
    }

    #[must_use]
    pub fn with_meta(mut self, meta: PageMeta) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Sets the HTTP status sent to the client.
    #[must_use]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
}

impl ApiResponse<()> {
    /// Creates a successful JSON envelope without a data field.
    pub fn empty() -> Self {
        let mut response = Self::success(());
        response.data = None;
        response
    }

    /// Creates a business failure with a client-safe message.
    pub fn failure(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            success: false,
            code: code.into(),
            message: message.into(),
            data: None,
            meta: None,
            errors: None,
            status,
        }
    }

    #[must_use]
    pub fn with_errors(mut self, errors: BTreeMap<String, Vec<String>>) -> Self {
        self.errors = Some(errors);
        self
    }
}

impl<T: Serialize> ApiResponse<T> {
    /// Encodes the envelope and lets the HTTP exception handler handle serialization errors.
    pub fn into_response(self) -> Result<Response, HttpError> {
        if self.status.is_informational()
            || matches!(
                self.status,
                StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT | StatusCode::NOT_MODIFIED
            )
        {
            return Ok(Response::empty(self.status));
        }

        let body = serde_json::to_vec(&self).map_err(|source| HttpError::Internal {
            source: source.into(),
        })?;
        let mut response = Response::new(self.status, Body::from(body));
        response
            .parts_mut()
            .headers
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(response)
    }
}
