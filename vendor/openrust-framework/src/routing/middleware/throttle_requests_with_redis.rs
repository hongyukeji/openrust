use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};

/// Applies a named or numeric limiter using a Redis duration counter.
pub struct ThrottleRequestsWithRedis {
    name: String,
}

impl ThrottleRequestsWithRedis {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Middleware for ThrottleRequestsWithRedis {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(super::throttle_requests::handle(
            &self.name, true, request, next,
        ))
    }
}
