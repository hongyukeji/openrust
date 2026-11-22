//! Transport-independent types for the HTTP request lifecycle.

use std::{future::Future, pin::Pin};

mod api_response;
mod binary_file_response;
mod body;
pub mod client;
mod download;
mod error;
mod http_response_exception;
mod json_formatter;
mod jsonp_response;
pub mod middleware;
mod next;
mod proxy;
mod redirect_response;
mod request;
pub mod resources;
mod response;
mod response_factory;
mod streamed_response;
mod trusted_hosts;
mod uploaded_file;

pub use ::http::{
    Extensions, HeaderMap, Method, StatusCode, Uri, request::Parts as RequestParts,
    response::Parts as ResponseParts,
};
pub use api_response::{ApiResponse, PageMeta};
pub use body::Body;
pub(crate) use download::filename_header;
pub use error::{HttpError, abort, abortResponsable, abortResponse};
pub use http_response_exception::HttpResponseException;
pub use next::Next;
pub(crate) use proxy::{ProxyPolicy, ProxyRequest};
pub use proxy::{RequestSecurity, TrustedProxyHeaders};
pub use redirect_response::RedirectResponse;
pub(crate) use request::context as request_context;
pub use request::{Request, RequestAttributes, RequestField, RequestValue};
pub(crate) use request::{parse_query, parse_uri_query};
pub use response::Response;
pub(crate) use response::ResponseKind;
pub use response::ResponseOutput;
pub(crate) use response_factory::ResponseServiceProvider;
pub use response_factory::{ResponseFactory, ResponseMacro};
pub use streamed_response::{StreamedEvent, StreamedJson};
pub(crate) use trusted_hosts::TrustedHosts;
pub(crate) use uploaded_file::random_name as random_upload_name;
pub use uploaded_file::{UploadedFile, UploadedFileValue};

/// A request lifecycle step that can be awaited by a multithreaded runtime.
pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) use request::matches_pattern;
