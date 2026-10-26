#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

#[cfg(feature = "http-client")]
pub mod client;
pub mod client_ip;
pub mod extract;
pub mod health;
pub mod middleware;
pub mod plugin;
pub mod rate_limit;
pub mod resource;
pub mod response;
pub mod routes;
pub mod serve;
pub mod sort;

pub use crate::i18n::RequestLocale;
pub use axum;
#[cfg(feature = "http-client")]
pub use client::{
    HttpBody, HttpClient, HttpClientConfig, HttpClientError, HttpRequest, HttpResponse,
    HttpResponseBody, HttpTargetPolicy, HttpTransport, HttpTransportRequest, IdempotentHttpRequest,
    MultipartForm, RetryPolicy, TransportResponse,
};
pub use client_ip::ClientIp;
pub use extract::{Inject, Json, Path, Query, Validated};
pub use plugin::{AppBuilderHttpExt, HttpPlugin, RouteRegistry, build_router};
pub use rate_limit::{RateLimitKey, RateLimitLayer};
pub use resource::{Action, Resource, ResourceActions};
pub use response::{Created, Located, NoContent, PageParams, Paginated};
pub use routes::Routes;
pub use serve::{ServeCommand, serve};
pub use sort::{SortDirection, SortField, SortParams};
