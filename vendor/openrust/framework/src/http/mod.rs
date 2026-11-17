//! Transport-independent types for the HTTP request lifecycle.

use std::{future::Future, pin::Pin};

mod api_response;
mod binary_file_response;
mod body;
pub mod client;
mod download;
mod error;
mod jsonp_response;
pub mod middleware;
mod next;
mod redirect_response;
mod request;
pub mod resources;
mod response;
mod streamed_response;
mod uploaded_file;

pub use ::http::{
    Extensions, HeaderMap, Method, StatusCode, Uri, request::Parts as RequestParts,
    response::Parts as ResponseParts,
};
pub use api_response::{ApiResponse, PageMeta};
pub use body::Body;
pub(crate) use download::filename_header;
pub use error::{HttpError, abort};
pub use next::Next;
pub use redirect_response::RedirectResponse;
pub(crate) use request::context as request_context;
pub use request::{Request, RequestAttributes};
pub(crate) use request::{parse_query, parse_uri_query};
pub use response::Response;
pub use streamed_response::{StreamedEvent, StreamedJson};
pub use uploaded_file::UploadedFile;
pub(crate) use uploaded_file::random_name as random_upload_name;

/// A request lifecycle step that can be awaited by a multithreaded runtime.
pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(feature = "sanctum")]
pub(crate) use request::matches_pattern;
