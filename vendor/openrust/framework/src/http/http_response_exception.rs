use std::{
    error::Error,
    fmt,
    sync::{Mutex, PoisonError},
};

use super::{Response, StatusCode};

/// Carries a ready-to-send response through the HTTP error path.
pub struct HttpResponseException {
    status: StatusCode,
    response: Mutex<Option<Response>>,
}

impl HttpResponseException {
    pub fn new(response: Response) -> Self {
        Self {
            status: response.status(),
            response: Mutex::new(Some(response)),
        }
    }

    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// Takes the response once, including its owned body, for custom rendering.
    pub fn take_response(&self) -> Option<Response> {
        self.response
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

impl fmt::Debug for HttpResponseException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpResponseException")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for HttpResponseException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.status.fmt(formatter)
    }
}

impl Error for HttpResponseException {}
