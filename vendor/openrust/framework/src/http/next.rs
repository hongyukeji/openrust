use std::sync::Arc;

use crate::contracts::{http::Middleware, routing::Handler};

use super::{Request, Response};

/// A continuation consumed when middleware passes a request downstream.
pub struct Next<'a> {
    middleware: &'a [Arc<dyn Middleware>],
    handler: &'a dyn Handler,
}

impl<'a> Next<'a> {
    pub(crate) fn new(middleware: &'a [Arc<dyn Middleware>], handler: &'a dyn Handler) -> Self {
        Self {
            middleware,
            handler,
        }
    }

    #[cfg(feature = "sanctum")]
    pub(crate) async fn run_through(
        self,
        before: &[Arc<dyn Middleware>],
        request: &mut Request,
    ) -> Response {
        Next::new(before, &Continuation(self)).run(request).await
    }

    pub async fn run(self, request: &mut Request) -> Response {
        let result = match self.middleware.split_first() {
            Some((middleware, rest)) => {
                middleware
                    .handle(request, Self::new(rest, self.handler))
                    .await
            }
            None => self.handler.handle(request).await,
        };
        match result {
            Ok(response) => response,
            Err(error) => match request.exceptions.clone() {
                Some(handler) => handler.render_http(request, &error).await,
                None => error.bare_response(),
            },
        }
    }
}

#[cfg(feature = "sanctum")]
struct Continuation<'a>(Next<'a>);

#[cfg(feature = "sanctum")]
impl Handler for Continuation<'_> {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> super::HttpFuture<'a, Result<Response, super::HttpError>> {
        Box::pin(async move {
            Ok(Next::new(self.0.middleware, self.0.handler)
                .run(request)
                .await)
        })
    }
}
