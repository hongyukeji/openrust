use std::{error::Error, fmt};

use super::{HeaderMap, Method, Response, StatusCode};

/// An error rendered by the HTTP kernel into a response.
#[derive(Debug)]
#[non_exhaustive]
pub enum HttpError {
    InvalidRequest,
    PayloadTooLarge,
    Unauthenticated,
    InvalidSignature,
    AuthorizationDenied(String),
    AuthorizationDeniedWithStatus {
        status: StatusCode,
        message: String,
    },
    Validation(Box<crate::validation::ValidationException>),
    OriginMismatch,
    TokenMismatch,
    Throttled {
        headers: super::HeaderMap,
    },
    NotFound,
    MethodNotAllowed {
        allowed: Vec<Method>,
    },
    Aborted {
        status: StatusCode,
        message: String,
        headers: Box<HeaderMap>,
    },
    Internal {
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Creates an explicit HTTP exception to return from a handler or middleware.
pub fn abort(status: StatusCode, message: impl Into<String>, headers: HeaderMap) -> HttpError {
    HttpError::Aborted {
        status,
        message: message.into(),
        headers: Box::new(headers),
    }
}

impl HttpError {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::InvalidSignature | Self::AuthorizationDenied(_) => StatusCode::FORBIDDEN,
            Self::AuthorizationDeniedWithStatus { status, .. } => *status,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::OriginMismatch => StatusCode::FORBIDDEN,
            Self::TokenMismatch => StatusCode::from_u16(419).expect("valid HTTP status"),
            Self::Throttled { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed { .. } => StatusCode::METHOD_NOT_ALLOWED,
            Self::Aborted { status, .. } => *status,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn bare_response(&self) -> Response {
        self.with_response_headers(Response::empty(self.status()))
    }

    pub(crate) fn with_response_headers(&self, response: Response) -> Response {
        match self {
            Self::MethodNotAllowed { allowed } => response.with_allow(allowed, ", "),
            Self::Throttled { headers } => response.withHeaders(headers.clone()),
            Self::Aborted { headers, .. } => response.withHeaders(headers.as_ref().clone()),
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
            Self::Validation(source) => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<crate::filesystem::FilesystemError> for HttpError {
    fn from(error: crate::filesystem::FilesystemError) -> Self {
        use crate::filesystem::FilesystemError;

        match error {
            FilesystemError::InvalidPath => Self::InvalidRequest,
            FilesystemError::NotFound => Self::NotFound,
            source => Self::Internal {
                source: Box::new(source),
            },
        }
    }
}

impl From<crate::validation::ValidationError> for HttpError {
    fn from(error: crate::validation::ValidationError) -> Self {
        match error {
            crate::validation::ValidationError::Invalid(exception) => Self::Validation(exception),
            crate::validation::ValidationError::LimitsExceeded => Self::PayloadTooLarge,
            source => Self::Internal {
                source: Box::new(source),
            },
        }
    }
}
