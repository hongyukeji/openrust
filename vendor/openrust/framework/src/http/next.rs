use std::sync::Arc;

use crate::contracts::{
    debug::{Exception, ExceptionHandler as _},
    http::Middleware,
    routing::Handler,
};

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
        result.unwrap_or_else(|error| {
            if let Some(handler) = &request.exceptions {
                let exception = Exception::Http(&error);
                let _ = handler.report(&exception);
                handler.render(request, &exception)
            } else {
                error.default_response(request, None)
            }
        })
    }
}
