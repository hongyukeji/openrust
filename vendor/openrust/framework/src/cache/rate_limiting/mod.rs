//! Named limiter results and request limit policies.

pub(crate) mod counters;
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

impl From<Limit> for LimiterResponse {
    fn from(limit: Limit) -> Self {
        Self::Limit(limit)
    }
}

impl From<Vec<Limit>> for LimiterResponse {
    fn from(limits: Vec<Limit>) -> Self {
        Self::Limits(limits)
    }
}

impl From<Unlimited> for LimiterResponse {
    fn from(unlimited: Unlimited) -> Self {
        Self::Unlimited(unlimited)
    }
}

impl From<Response> for LimiterResponse {
    fn from(response: Response) -> Self {
        Self::Response(response)
    }
}
