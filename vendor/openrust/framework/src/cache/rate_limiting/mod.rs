//! Named limiter results and request limit policies.

mod limit;
mod unlimited;

use crate::http::{HeaderMap, Request, Response};

pub use limit::Limit;
pub use unlimited::Unlimited;

/// Resolves a named limiter against the current request.
pub type Limiter = dyn Fn(&Request) -> LimiterResponse + Send + Sync;

/// Determines whether a completed response consumes an attempt.
pub type AfterCallback = dyn Fn(&Response) -> bool + Send + Sync;

/// Renders a rejected request with the throttling response headers.
pub type ResponseCallback = dyn Fn(&Request, HeaderMap) -> Response + Send + Sync;

/// The result of a named limiter callback.
pub enum LimiterResponse {
    Limit(Limit),
    Limits(Vec<Limit>),
    Unlimited(Unlimited),
    Response(Response),
}

/// Distinguishes a skipped attempt from a callback that returned a value.
pub enum Attempt<T> {
    Throttled,
    Executed(T),
}
