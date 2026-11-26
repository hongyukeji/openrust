//! Middleware around an individual HTTP transport attempt.
use super::{Request, Response, Result};
use crate::http::HttpFuture;
use std::sync::Arc;

/// An application-local asynchronous HTTP handler wrapper.
pub type Middleware =
    dyn for<'a> Fn(Request, Next<'a>) -> HttpFuture<'a, Result<Response>> + Send + Sync;

/// A continuation consumed when client middleware sends the request downstream.
pub struct Next<'a> {
    handler: Box<dyn FnOnce(Request) -> HttpFuture<'a, Result<Response>> + Send + 'a>,
}
impl<'a> Next<'a> {
    pub(super) fn new(
        handler: impl FnOnce(Request) -> HttpFuture<'a, Result<Response>> + Send + 'a,
    ) -> Self {
        Self {
            handler: Box::new(handler),
        }
    }
    pub fn run(self, request: Request) -> HttpFuture<'a, Result<Response>> {
        (self.handler)(request)
    }
}
pub(super) fn run<'a>(
    middleware: &'a [Arc<Middleware>],
    request: Request,
    next: Next<'a>,
) -> HttpFuture<'a, Result<Response>> {
    match middleware.split_first() {
        Some((first, rest)) => first(request, Next::new(move |request| run(rest, request, next))),
        None => next.run(request),
    }
}
