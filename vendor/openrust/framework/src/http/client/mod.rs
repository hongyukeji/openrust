//! Outbound HTTP requests, responses, and application-local testing.

mod facade;
mod factory;
mod pending_request;
mod pool;
mod response;

pub use facade::{Http, HttpClientServiceProvider};
pub use factory::{Factory, RecordedRequest, ResponseSequence};
pub use pending_request::PendingRequest;
pub use pool::Pool;
pub use reqwest::Request;
pub use reqwest::multipart::{Form, Part};
pub use response::Response;

use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Configuration,
    InvalidRequest,
    Connection,
    Timeout,
    BodyTooLarge,
    InvalidJson(serde_json::Error),
    UnreplayableRequest,
    StrayRequest,
    SequenceExhausted,
    RequestFailed(Box<Response>),
    Unavailable(crate::contracts::container::BindingResolutionError),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration => formatter.write_str("invalid HTTP client configuration"),
            Self::InvalidRequest => formatter.write_str("invalid outgoing HTTP request"),
            Self::Connection => formatter.write_str("outgoing HTTP connection failed"),
            Self::Timeout => formatter.write_str("outgoing HTTP request timed out"),
            Self::BodyTooLarge => {
                formatter.write_str("outgoing HTTP response exceeded its body limit")
            }
            Self::InvalidJson(_) => {
                formatter.write_str("outgoing HTTP response contained invalid JSON")
            }
            Self::UnreplayableRequest => {
                formatter.write_str("outgoing HTTP request cannot be replayed")
            }
            Self::SequenceExhausted => {
                formatter.write_str("outgoing HTTP fake sequence is exhausted")
            }
            Self::StrayRequest => formatter.write_str("outgoing HTTP request has no matching fake"),
            Self::RequestFailed(response) => write!(
                formatter,
                "outgoing HTTP request returned {}",
                response.status()
            ),
            Self::Unavailable(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn transport_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Timeout
    } else {
        Error::Connection
    }
}
