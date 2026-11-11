use std::any::Any;

use crate::{
    foundation::{Application, BootstrapError},
    http::{HttpError, HttpFuture, Next, Request, RequestParts, Response, ResponseParts},
};

/// Bootstraps HTTP services, dispatches requests, and terminates the lifecycle.
pub trait Kernel: Send + Sync {
    fn bootstrap(&mut self) -> HttpFuture<'_, Result<(), BootstrapError>>;
    fn get_application(&self) -> &Application;
    fn handle<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, Response>;
    fn terminate<'a>(
        &'a self,
        request: &'a RequestParts,
        response: &'a ResponseParts,
    ) -> HttpFuture<'a, ()>;
}

/// Handles a request around the downstream middleware and route.
pub trait Middleware: Any + Send + Sync {
    /// Returns the underlying middleware type for route priority ordering.
    fn middleware_type_id(&self) -> std::any::TypeId {
        Any::type_id(self)
    }

    /// Exposes the optional post-response lifecycle hook.
    fn terminable(&self) -> Option<&dyn TerminableMiddleware> {
        None
    }

    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>>;
}

/// Middleware with work that runs after the transport finishes the response.
pub trait TerminableMiddleware: Middleware {
    fn terminate<'a>(
        &'a self,
        request: &'a RequestParts,
        response: &'a ResponseParts,
    ) -> HttpFuture<'a, ()>;
}
