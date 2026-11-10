#![allow(non_snake_case)]

use std::error::Error;

use crate::{
    console::Output,
    http::{HttpError, Request, Response},
    log::LogResult,
};

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
    fn shouldReport(&self, exception: &Exception<'_>) -> bool;
    fn report(&self, exception: &Exception<'_>) -> LogResult<()>;
    fn render(&self, request: &Request, exception: &Exception<'_>) -> Response;
    fn renderForConsole(&self, output: &mut Output, exception: &Exception<'_>);
}
