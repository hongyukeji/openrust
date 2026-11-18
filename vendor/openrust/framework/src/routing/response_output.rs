use std::{
    future::{Future, Ready, ready},
    sync::Arc,
};

use axum::body::Bytes;
use http::Response as NativeResponse;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::{
    contracts::routing::Handler,
    http::{HttpError, HttpFuture, Request, Response, ResponseOutput},
    support::{Collection, Fluent, Stringable},
};

use super::IntoRouteHandler;

#[doc(hidden)]
pub struct AdaptedResponse;

trait DirectOutput: ResponseOutput {}

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
impl<T: Serialize> DirectOutput for Vec<T> {}
impl<T: Serialize, const N: usize> DirectOutput for [T; N] {}
impl<T: Serialize> DirectOutput for Collection<T> {}
impl DirectOutput for Fluent {}

trait Output {
    fn convert(self) -> Result<Response, HttpError>;
}

impl<T: DirectOutput> Output for T {
    fn convert(self) -> Result<Response, HttpError> {
        self.into_response()
    }
}

impl<T: DirectOutput> Output for Result<T, HttpError> {
    fn convert(self) -> Result<Response, HttpError> {
        self.and_then(Output::convert)
    }
}

struct OutputHandler<F>(F);

impl<F, Fut, T> Handler for OutputHandler<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = T> + Send,
    T: Output,
{
    fn handle<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { (self.0)().await.convert() })
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
        Box::pin(async { (self.0)(request).await.convert() })
    }
}

impl<F, T> IntoRouteHandler<(Request, AdaptedResponse, T)> for F
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, T> + Send + Sync + 'static,
    T: Output,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(BorrowedOutputHandler(self))
    }
}
