use std::{
    future::{Future, Ready, ready},
    sync::Arc,
};

use axum::body::Bytes;
use http::Response as NativeResponse;

use crate::{
    contracts::routing::Handler,
    http::{HttpError, HttpFuture, Request, Response},
};

use super::IntoRouteHandler;

#[doc(hidden)]
pub struct NativeResponseOutput;

trait Output {
    fn convert(self) -> Result<Response, HttpError>;
}

impl<B> Output for NativeResponse<B>
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: Into<axum::BoxError>,
{
    fn convert(self) -> Result<Response, HttpError> {
        Ok(self.into())
    }
}

impl<B> Output for Result<NativeResponse<B>, HttpError>
where
    NativeResponse<B>: Output,
{
    fn convert(self) -> Result<Response, HttpError> {
        self.and_then(Output::convert)
    }
}

struct NativeHandler<F>(F);

impl<F, Fut, T> Handler for NativeHandler<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = T> + Send,
    T: Output,
{
    fn handle<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { (self.0)().await.convert() })
    }
}

impl<F, T> IntoRouteHandler<(NativeResponseOutput, T)> for F
where
    F: Fn() -> T + Send + Sync + 'static,
    T: Output + Send,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        let action = move || -> Ready<T> { ready(self()) };
        Arc::new(NativeHandler(action))
    }
}

impl<F, Fut, T> IntoRouteHandler<(NativeResponseOutput, Fut, T)> for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = T> + Send,
    T: Output,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(NativeHandler(self))
    }
}

struct BorrowedNativeHandler<F>(F);

impl<F, T> Handler for BorrowedNativeHandler<F>
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

impl<F, T> IntoRouteHandler<(Request, NativeResponseOutput, T)> for F
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, T> + Send + Sync + 'static,
    T: Output,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(BorrowedNativeHandler(self))
    }
}
