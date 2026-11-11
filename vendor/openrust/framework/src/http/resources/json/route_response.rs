use std::{future::Future, sync::Arc};

use crate::{
    contracts::routing::Handler,
    http::{HttpFuture, Request, Response},
    routing::IntoRouteHandler,
};

use super::{JsonResource, ResourceResult};

#[doc(hidden)]
pub struct ResourceRoute;

trait ResourceOutput: Send {
    fn respond(self, request: &Request) -> ResourceResult<Response>;
}

impl<T: JsonResource + Send> ResourceOutput for T {
    fn respond(self, request: &Request) -> ResourceResult<Response> {
        self.toResponse(request)
    }
}

impl<T: JsonResource + Send> ResourceOutput for ResourceResult<T> {
    fn respond(self, request: &Request) -> ResourceResult<Response> {
        self?.toResponse(request)
    }
}

impl<F, T> IntoRouteHandler<(ResourceRoute, T)> for F
where
    F: Fn() -> T + Send + Sync + 'static,
    T: ResourceOutput,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(SyncResource(self))
    }
}

struct SyncResource<F>(F);

impl<F, T> Handler for SyncResource<F>
where
    F: Fn() -> T + Send + Sync,
    T: ResourceOutput,
{
    fn handle<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, ResourceResult<Response>> {
        let response = (self.0)().respond(request);
        Box::pin(async move { response })
    }
}

struct AsyncResource<F>(F);

impl<F, Fut, T> IntoRouteHandler<(ResourceRoute, Fut, T)> for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = T> + Send,
    T: ResourceOutput,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(AsyncResource(self))
    }
}

impl<F, Fut, T> Handler for AsyncResource<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = T> + Send,
    T: ResourceOutput,
{
    fn handle<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, ResourceResult<Response>> {
        Box::pin(async move { (self.0)().await.respond(request) })
    }
}

struct BorrowedResource<F>(F);

impl<F, T> IntoRouteHandler<(Request, ResourceRoute, T)> for F
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, T> + Send + Sync + 'static,
    T: ResourceOutput,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(BorrowedResource(self))
    }
}

impl<F, T> Handler for BorrowedResource<F>
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, T> + Send + Sync,
    T: ResourceOutput,
{
    fn handle<'a>(&'a self, request: &'a mut Request) -> HttpFuture<'a, ResourceResult<Response>> {
        Box::pin(async move { (self.0)(request).await.respond(request) })
    }
}
