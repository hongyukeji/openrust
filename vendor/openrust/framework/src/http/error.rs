use std::{error::Error, fmt};

use ::http::{HeaderValue, header::CONTENT_TYPE};

use crate::foundation::configuration::Exceptions;

use super::{Body, Method, Request, Response, StatusCode};

/// An error rendered by the HTTP kernel into a response.
#[derive(Debug)]
#[non_exhaustive]
pub enum HttpError {
    InvalidRequest,
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
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::MethodNotAllowed { .. } => StatusCode::METHOD_NOT_ALLOWED,
            Self::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn default_response(
        &self,
        request: &Request,
        exceptions: Option<&Exceptions>,
    ) -> Response {
        let status = self.status();
        let mut response = if let Some(exceptions) = exceptions {
            let message = status.canonical_reason().expect("known HTTP error status");
            let (content_type, body) = if exceptions.renders_json(request, self) {
                (
                    "application/json",
                    serde_json::json!({"message": message}).to_string(),
                )
            } else {
                let title = format!("{} {message}", status.as_u16());
                let body = format!(
                    "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title></head><body><h1>{title}</h1></body></html>"
                );
                ("text/html; charset=utf-8", body)
            };
            let mut response = Response::new(status, Body::from(body));
            response
                .parts_mut()
                .headers
                .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
            response
        } else {
            Response::empty(status)
        };
        if let Self::MethodNotAllowed { allowed } = self {
            response = response.with_allow(allowed, ", ");
        }
        response
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
