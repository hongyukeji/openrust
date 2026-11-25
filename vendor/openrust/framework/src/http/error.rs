use std::{error::Error, fmt};

use crate::contracts::support::Responsable;

use super::{HeaderMap, HttpResponseException, Method, Request, Response, StatusCode};

/// An error rendered by the HTTP kernel into a response.
#[derive(Debug)]
#[non_exhaustive]
pub enum HttpError {
    InvalidRequest,
    PayloadTooLarge,
    Unauthenticated,
    InvalidSignature,
    AuthorizationDenied(String),
    #[cfg(feature = "sanctum")]
    MissingAbility(crate::sanctum::MissingAbilityException),
    #[cfg(feature = "sanctum")]
    MissingScope(crate::sanctum::MissingScopeException),
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
    Response(Box<HttpResponseException>),
    DumpAndDie(String),
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

/// Creates an HTTP exception carrying a prepared response.
pub fn abort_response(response: impl Into<Response>) -> HttpError {
    HttpResponseException::new(response.into()).into()
}

/// Resolves a request-aware response before exiting through the HTTP error path.
pub async fn abort_responsable(response: impl Responsable, request: &mut Request) -> HttpError {
    match response.to_response(request).await {
        Ok(response) => abort_response(response),
        Err(error) => error,
    }
}

impl HttpError {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::InvalidSignature | Self::AuthorizationDenied(_) => StatusCode::FORBIDDEN,
            #[cfg(feature = "sanctum")]
            Self::MissingAbility(_) | Self::MissingScope(_) => StatusCode::FORBIDDEN,
            Self::AuthorizationDeniedWithStatus { status, .. } => *status,
            Self::Validation(exception) => exception.status_code(),
            Self::OriginMismatch => StatusCode::FORBIDDEN,
            Self::TokenMismatch => StatusCode::from_u16(419).expect("valid HTTP status"),
            Self::Throttled { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed { .. } => StatusCode::METHOD_NOT_ALLOWED,
            Self::Aborted { status, .. } => *status,
            Self::Response(error) => error.status(),
            Self::DumpAndDie(_) | Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn bare_response(&self) -> Response {
        if let Self::Response(error) = self
            && let Some(response) = error.take_response()
        {
            return response;
        }
        if let Self::DumpAndDie(output) = self {
            return Response::new(self.status(), super::Body::from(output.clone())).header(
                ::http::header::CONTENT_TYPE,
                ::http::HeaderValue::from_static("text/plain; charset=utf-8"),
            );
        }
        self.with_response_headers(Response::empty(self.status()))
    }

    pub(crate) fn with_response_headers(&self, response: Response) -> Response {
        match self {
            Self::MethodNotAllowed { allowed } => response.with_allow(allowed, ", "),
            Self::Throttled { headers } => response.with_headers(headers.clone()),
            Self::Aborted { headers, .. } => response.with_headers(headers.as_ref().clone()),
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
            Self::Response(source) => Some(source.as_ref()),
            #[cfg(feature = "sanctum")]
            Self::MissingAbility(source) => Some(source),
            #[cfg(feature = "sanctum")]
            Self::MissingScope(source) => Some(source),
            _ => None,
        }
    }
}

impl From<HttpResponseException> for HttpError {
    fn from(exception: HttpResponseException) -> Self {
        Self::Response(Box::new(exception))
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

impl From<crate::support::CollectionError> for HttpError {
    fn from(error: crate::support::CollectionError) -> Self {
        match error {
            crate::support::CollectionError::DumpAndDie(output) => Self::DumpAndDie(output),
            source => Self::Internal {
                source: Box::new(source),
            },
        }
    }
}

impl From<crate::config::ConfigError> for HttpError {
    fn from(source: crate::config::ConfigError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::contracts::container::BindingResolutionError> for HttpError {
    fn from(source: crate::contracts::container::BindingResolutionError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::translation::TranslationError> for HttpError {
    fn from(source: crate::translation::TranslationError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::view::ViewError> for HttpError {
    fn from(source: crate::view::ViewError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::contracts::debug::SharedException> for HttpError {
    fn from(source: crate::contracts::debug::SharedException) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::database::DatabaseError> for HttpError {
    fn from(source: crate::database::DatabaseError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<serde_json::Error> for HttpError {
    fn from(source: serde_json::Error) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<tokio::task::JoinError> for HttpError {
    fn from(source: tokio::task::JoinError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::log::LogError> for HttpError {
    fn from(source: crate::log::LogError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::redis::RedisError> for HttpError {
    fn from(source: crate::redis::RedisError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<http::header::InvalidHeaderValue> for HttpError {
    fn from(source: http::header::InvalidHeaderValue) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<std::io::Error> for HttpError {
    fn from(source: std::io::Error) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::mail::MailError> for HttpError {
    fn from(source: crate::mail::MailError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::notifications::NotificationError> for HttpError {
    fn from(source: crate::notifications::NotificationError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::hashing::HashError> for HttpError {
    fn from(source: crate::hashing::HashError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::image::Error> for HttpError {
    fn from(source: crate::image::Error) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::filesystem::FileError> for HttpError {
    fn from(source: crate::filesystem::FileError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}

impl From<crate::foundation::vite::ViteError> for HttpError {
    fn from(source: crate::foundation::vite::ViteError) -> Self {
        Self::Internal {
            source: Box::new(source),
        }
    }
}
