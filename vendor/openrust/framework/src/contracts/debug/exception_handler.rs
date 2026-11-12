#![allow(non_snake_case)]

use std::{
    error::Error,
    fmt,
    sync::{Arc, Weak},
};

use crate::{
    console::Output,
    http::{HttpError, HttpFuture, Request, Response},
    log::LogResult,
};

/// An error with explicit, shared instance identity for duplicate reporting.
#[derive(Clone, Debug)]
pub struct SharedException(Arc<dyn Error + Send + Sync>);

impl SharedException {
    pub fn new(error: impl Error + Send + Sync + 'static) -> Self {
        Self(Arc::new(error))
    }

    pub fn from_arc(error: Arc<dyn Error + Send + Sync>) -> Self {
        Self(error)
    }

    pub(crate) fn identity(&self) -> Weak<dyn Error + Send + Sync> {
        Arc::downgrade(&self.0)
    }
}

impl fmt::Display for SharedException {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl Error for SharedException {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Borrows the original error while preserving HTTP-specific metadata.
pub enum Exception<'a> {
    Http(&'a HttpError),
    Error(&'a (dyn Error + Send + Sync + 'static)),
}

impl Exception<'_> {
    pub(crate) fn error(&self) -> &(dyn Error + 'static) {
        match self {
            Self::Http(error) => *error,
            Self::Error(error) => *error,
        }
    }

    /// Finds the first matching error in the exception and its source chain.
    pub fn downcast_ref<E: Error + 'static>(&self) -> Option<&E> {
        let mut current = Some(self.error());
        while let Some(error) = current {
            if let Some(matched) = error.downcast_ref::<E>() {
                return Some(matched);
            }
            current = error.source();
        }
        None
    }
}

/// Reports exceptions and renders them at the appropriate application boundary.
pub trait ExceptionHandler: Send + Sync {
    /// Determines whether a queued attempt should fail instead of being retried.
    fn shouldStopRetries(&self, _exception: &Exception<'_>) -> bool {
        false
    }

    /// Checks report eligibility, consuming an attempt for a rate-limited report.
    fn shouldReport<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, bool>;
    fn report<'a>(&'a self, exception: &'a Exception<'_>) -> HttpFuture<'a, LogResult<()>>;
    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response;
    fn renderForConsole(&self, output: &mut Output, exception: &Exception<'_>);
}
