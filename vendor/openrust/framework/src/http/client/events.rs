//! Events for individual outgoing HTTP request attempts.

use crate::contracts::events::Event;

use super::{RecordedRequest, Response};

pub struct RequestSending {
    pub request: RecordedRequest,
}

pub struct ResponseReceived {
    pub request: RecordedRequest,
    pub response: Response,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionFailure {
    Connection,
    Timeout,
}

pub struct ConnectionFailed {
    pub request: RecordedRequest,
    pub exception: ConnectionFailure,
}

impl Event for RequestSending {
    fn name(&self) -> &str {
        r"OpenRust\Http\Client\Events\RequestSending"
    }
}

impl Event for ResponseReceived {
    fn name(&self) -> &str {
        r"OpenRust\Http\Client\Events\ResponseReceived"
    }
}

impl Event for ConnectionFailed {
    fn name(&self) -> &str {
        r"OpenRust\Http\Client\Events\ConnectionFailed"
    }
}
