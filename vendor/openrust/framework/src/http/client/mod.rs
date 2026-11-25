//! Outbound HTTP requests, responses, and application-local testing.

mod batch;
mod body;
mod destination;
mod digest;
pub mod events;
mod facade;
mod factory;
mod middleware;
mod pending_request;
mod pool;
mod response;
mod sink;
mod transport;
mod uri_template;

pub use batch::{Batch, BatchProgress, BatchResults};
pub use body::ResponseResource;
pub use facade::{Http, HttpClientServiceProvider};
pub use factory::{
    Factory, RecordedMultipartField, RecordedRequest, RecordedRequestDataSource, ResponseSequence,
};
pub use middleware::{Middleware, Next};
pub use pending_request::PendingRequest;
pub use pool::Pool;
pub use reqwest::multipart::{Form, Part};
pub use reqwest::{Certificate as TlsCertificate, Identity as TlsIdentity, Request};
pub use response::{Response, TransferStats};
pub use sink::Sink;

use std::fmt;

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Configuration,
    Sink(std::io::Error),
    ResponseBody(std::io::Error),
    InvalidRequest,
    DestinationMismatch,
    InvalidUriTemplate,
    Connection,
    Timeout,
    BodyTooLarge,
    InvalidJson(serde_json::Error),
    UnreplayableRequest,
    OutsideLifecycle,
    RedirectLimit,
    StrayRequest,
    SequenceExhausted,
    RequestFailed(Box<Response>),
    Event(crate::events::EventError),
    Unavailable(crate::contracts::container::BindingResolutionError),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResponseBody(error) => write!(formatter, "读取 HTTP 响应正文失败：{error}"),
            Self::Sink(error) => write!(formatter, "保存 HTTP 响应失败：{error}"),
            Self::Configuration => formatter.write_str("invalid HTTP client configuration"),
            Self::InvalidRequest => formatter.write_str("invalid outgoing HTTP request"),
            Self::DestinationMismatch => formatter.write_str("HTTP 请求目标与固定连接地址不匹配"),
            Self::InvalidUriTemplate => {
                formatter.write_str("invalid HTTP URI template or parameter")
            }
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
            Self::RedirectLimit => {
                formatter.write_str("outgoing HTTP request exceeded its redirect limit")
            }
            Self::SequenceExhausted => {
                formatter.write_str("outgoing HTTP fake sequence is exhausted")
            }
            Self::OutsideLifecycle => {
                formatter.write_str("deferred HTTP batch requires an active application task")
            }
            Self::StrayRequest => formatter.write_str("outgoing HTTP request has no matching fake"),
            Self::RequestFailed(response) => write!(
                formatter,
                "outgoing HTTP request returned {}",
                response.status()
            ),
            Self::Event(error) => error.fmt(formatter),
            Self::Unavailable(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sink(error) | Self::ResponseBody(error) => Some(error),
            Self::InvalidJson(error) => Some(error),
            Self::Event(error) => Some(error),
            Self::Unavailable(error) => Some(error),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn transport_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Timeout
    } else {
        Error::Connection
    }
}
