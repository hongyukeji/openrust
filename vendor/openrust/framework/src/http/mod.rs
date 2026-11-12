//! Transport-independent types for the HTTP request lifecycle.

use std::{future::Future, pin::Pin};

mod api_response;
mod body;
pub mod client;
mod error;
pub mod middleware;
mod next;
mod request;
pub mod resources;
mod response;
mod uploaded_file;

pub use ::http::{
    Extensions, HeaderMap, Method, StatusCode, Uri, request::Parts as RequestParts,
    response::Parts as ResponseParts,
};
pub use api_response::{ApiResponse, PageMeta};
pub use body::Body;
pub use error::{HttpError, abort};
pub use next::Next;
pub(crate) use request::context as request_context;
pub(crate) use request::parse_query;
pub use request::{Request, RequestAttributes};
pub use response::Response;
pub use uploaded_file::UploadedFile;
pub(crate) use uploaded_file::random_name as random_upload_name;

/// A request lifecycle step that can be awaited by a multithreaded runtime.
pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
