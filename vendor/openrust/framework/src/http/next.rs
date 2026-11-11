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
