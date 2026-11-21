//! Events emitted by the application HTTP kernel.

use std::{collections::HashMap, time::Duration};

use crate::{
    container::ParameterValue,
    contracts::events::Event,
    http::{Request, RequestParts, Response, ResponseParts},
};

/// The completed request's metadata and server context, before transport sends its body.
///
/// Streaming bodies are never polled or buffered to create this event. Metadata and
/// the attribute map are detached snapshots; shared values retain their normal ownership.
#[allow(non_snake_case, reason = "Matches Laravel route naming")]
pub struct RequestHandled {
    pub request: RequestParts,
    pub response: ResponseParts,
    /// Shared bytes only for responses whose content is already buffered.
    pub responseContent: Option<axum::body::Bytes>,
    pub attributes: HashMap<String, ParameterValue>,
    pub routeName: Option<String>,
    /// Already-decoded input; an unread body is not consumed for observation.
    pub payload: Option<serde_json::Value>,
    /// Unix milliseconds captured when the kernel begins handling the request.
    pub requestStartedAt: i64,
    /// Elapsed time measured by a monotonic clock.
    pub duration: Duration,
}

impl RequestHandled {
    pub(super) fn new(
        request: &Request,
        response: &Response,
        request_started_at: i64,
        duration: Duration,
    ) -> Self {
        let mut parts = request.parts().clone();
        parts.uri = request.uri().clone();
        Self {
            request: parts,
            response: response.parts().clone(),
            responseContent: response.getContent().cloned(),
            attributes: request.attributes().snapshot(),
            routeName: request.route_name.clone(),
            payload: request.inputSnapshot(),
            requestStartedAt: request_started_at,
            duration,
        }
    }
}

impl Event for RequestHandled {
    fn name(&self) -> &str {
        r"OpenRust\Foundation\Http\Events\RequestHandled"
    }
}

/// Metadata captured after entering a request scope and before global middleware.
/// The body remains owned by the request and is never polled for this event.
pub struct RequestReceived {
    pub request: RequestParts,
}
impl RequestReceived {
    pub const NAME: &'static str = r"OpenRust\Foundation\Http\Events\RequestReceived";
    pub(super) fn new(request: &Request) -> Self {
        let mut parts = request.parts().clone();
        parts.uri = request.uri().clone();
        Self { request: parts }
    }
}
impl Event for RequestReceived {
    fn name(&self) -> &str {
        Self::NAME
    }
}
