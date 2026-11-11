use std::{error::Error, fmt};

use super::{Method, Response, StatusCode};

/// An error rendered by the HTTP kernel into a response.
#[derive(Debug)]
#[non_exhaustive]
pub enum HttpError {
    InvalidRequest,
    PayloadTooLarge,
    NotFound,
    MethodNotAllowed {
        allowed: Vec<Method>,
    },
    Internal {
        source: Box<dyn Error + Send + Sync>,
    },
}

impl HttpError {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed { .. } => StatusCode::METHOD_NOT_ALLOWED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn bare_response(&self) -> Response {
        self.with_response_headers(Response::empty(self.status()))
    }

    pub(crate) fn with_response_headers(&self, response: Response) -> Response {
        match self {
            Self::MethodNotAllowed { allowed } => response.with_allow(allowed, ", "),
            _ => response,
        }
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.status().fmt(formatter)
    }
}

impl Error for HttpError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Internal { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}
