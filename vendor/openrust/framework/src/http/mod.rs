//! Transport-independent types for the HTTP request lifecycle.

use std::{future::Future, pin::Pin};

mod body;
mod error;
mod next;
mod request;
mod response;
mod uploaded_file;

pub use ::http::{
    Extensions, HeaderMap, Method, StatusCode, Uri, request::Parts as RequestParts,
    response::Parts as ResponseParts,
};
pub use body::Body;
pub use error::HttpError;
pub use next::Next;
pub use request::Request;
pub use response::Response;
pub use uploaded_file::UploadedFile;

/// A request lifecycle step that can be awaited by a multithreaded runtime.
pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
