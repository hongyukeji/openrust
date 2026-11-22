use std::{
    future::{Future, Ready, ready},
    sync::Arc,
};

use axum::body::Bytes;
use http::Response as NativeResponse;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    contracts::{routing::Handler, support::Responsable},
    http::{HttpError, HttpFuture, RedirectResponse, Request, Response},
    support::{Collection, Fluent, Stringable},
};

use super::IntoRouteHandler;

#[doc(hidden)]
pub struct AdaptedResponse;

// Keeps fallible borrowed handlers returning Response on their original path.
trait DirectOutput: Responsable {}

impl<B> DirectOutput for NativeResponse<B>
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
}

impl DirectOutput for &'static str {}
impl DirectOutput for String {}
impl DirectOutput for Stringable {}
impl DirectOutput for Value {}
impl DirectOutput for Map<String, Value> {}
impl<T: Serialize + Send> DirectOutput for Vec<T> {}
impl<T: Serialize + Send, const N: usize> DirectOutput for [T; N] {}
impl<T: Serialize + Send> DirectOutput for Collection<T> {}
impl DirectOutput for Fluent {}
impl DirectOutput for RedirectResponse {}
impl DirectOutput for crate::image::Image {}
impl DirectOutput for crate::view::View {}

trait Output {
    fn respond(
        self,
        request: &mut Request,
    ) -> impl Future<Output = Result<Response, HttpError>> + Send;
}

impl<T: Responsable> Output for T {
    async fn respond(self, request: &mut Request) -> Result<Response, HttpError> {
        self.toResponse(request).await
    }
}

impl<T: Responsable> Output for Result<T, HttpError> {
    async fn respond(self, request: &mut Request) -> Result<Response, HttpError> {
        self?.toResponse(request).await
    }
}

struct OutputHandler<F>(F);

impl<F, Fut, T> Handler for OutputHandler<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = T> + Send,
    T: Output,
{
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { (self.0)().await.respond(request).await })
    }
}

impl<F, T> IntoRouteHandler<(AdaptedResponse, T)> for F
where
    F: Fn() -> T + Send + Sync + 'static,
    T: Output + Send,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        let action = move || -> Ready<T> { ready(self()) };
        Arc::new(OutputHandler(action))
    }
}

impl<F, Fut, T> IntoRouteHandler<(AdaptedResponse, Fut, T)> for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = T> + Send,
    T: Output,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(OutputHandler(self))
    }
}

struct BorrowedOutputHandler<F>(F);

impl<F, T> Handler for BorrowedOutputHandler<F>
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, T> + Send + Sync,
    T: Output,
{
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { (self.0)(request).await.respond(request).await })
    }
}

impl<F, T> IntoRouteHandler<(Request, AdaptedResponse, T)> for F
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, T> + Send + Sync + 'static,
    T: Responsable,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(BorrowedOutputHandler(self))
    }
}

impl<F, T> IntoRouteHandler<(Request, AdaptedResponse, Result<T, HttpError>)> for F
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, Result<T, HttpError>> + Send + Sync + 'static,
    T: DirectOutput,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(BorrowedOutputHandler(self))
    }
}
