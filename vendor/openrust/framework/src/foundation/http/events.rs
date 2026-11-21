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
    pub attributes: HashMap<String, ParameterValue>,
    pub routeName: Option<String>,
    pub duration: Duration,
}

impl RequestHandled {
    pub(super) fn new(request: &Request, response: &Response, duration: Duration) -> Self {
        let mut parts = request.parts().clone();
        parts.uri = request.uri().clone();
        Self {
            request: parts,
            response: response.parts().clone(),
            attributes: request.attributes().snapshot(),
            routeName: request.route_name.clone(),
            duration,
        }
    }
}

impl Event for RequestHandled {
    fn name(&self) -> &str {
        r"OpenRust\Foundation\Http\Events\RequestHandled"
    }
}
