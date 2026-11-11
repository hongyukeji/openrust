use std::{future::Future, sync::Arc};

use crate::{
    contracts::routing::Handler,
    database::{
        DatabaseError, DatabaseResult,
        eloquent::{Collection, Model},
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
};

use super::IntoRouteHandler;

#[doc(hidden)]
pub struct ModelResponse;

trait ModelOutput: Send {
    fn into_response(self) -> Result<Response, HttpError>;
}

impl<M: Model> ModelOutput for M {
    fn into_response(self) -> Result<Response, HttpError> {
        let status = if self.state().wasRecentlyCreated() {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        };
        Response::json(status, self.toArray().map_err(internal)?)
    }
}

impl<M: Model> ModelOutput for Collection<M> {
    fn into_response(self) -> Result<Response, HttpError> {
        Response::json(StatusCode::OK, self.toArray().map_err(internal)?)
    }
}

impl<T: ModelOutput> ModelOutput for DatabaseResult<T> {
    fn into_response(self) -> Result<Response, HttpError> {
        match self {
            Ok(output) => output.into_response(),
            Err(DatabaseError::ModelNotFound(_)) => Err(HttpError::NotFound),
            Err(error) => Err(internal(error)),
        }
    }
}

fn internal(error: DatabaseError) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

impl<F, T> IntoRouteHandler<(ModelResponse, T)> for F
where
    F: Fn() -> T + Send + Sync + 'static,
    T: ModelOutput,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(Synchronous(self))
    }
}

struct Synchronous<F>(F);

impl<F, T> Handler for Synchronous<F>
where
    F: Fn() -> T + Send + Sync,
    T: ModelOutput,
{
    fn handle<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { (self.0)().into_response() })
    }
}

impl<F, Fut, T> IntoRouteHandler<(ModelResponse, Fut, T)> for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = T> + Send,
    T: ModelOutput,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(Asynchronous(self))
    }
}

struct Asynchronous<F>(F);

impl<F, Fut, T> Handler for Asynchronous<F>
where
    F: Fn() -> Fut + Send + Sync,
    Fut: Future<Output = T> + Send,
    T: ModelOutput,
{
    fn handle<'a>(&'a self, _: &'a mut Request) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { (self.0)().await.into_response() })
    }
}

impl<F, T> IntoRouteHandler<(Request, ModelResponse, T)> for F
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, T> + Send + Sync + 'static,
    T: ModelOutput,
{
    fn into_route_handler(self) -> Arc<dyn Handler> {
        Arc::new(Borrowed(self))
    }
}

struct Borrowed<F>(F);

impl<F, T> Handler for Borrowed<F>
where
    F: for<'a> Fn(&'a mut Request) -> HttpFuture<'a, T> + Send + Sync,
    T: ModelOutput,
{
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async { (self.0)(request).await.into_response() })
    }
}
