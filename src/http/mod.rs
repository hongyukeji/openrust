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
pub mod resource;
pub mod response;
pub mod routes;
pub mod serve;

pub use axum;
#[cfg(feature = "http-client")]
pub use client::{
    HttpBody, HttpClient, HttpClientConfig, HttpClientError, HttpRequest, HttpResponse,
    HttpResponseBody, HttpTransport, HttpTransportRequest, IdempotentHttpRequest, MultipartForm,
    RetryPolicy, TransportResponse,
};
pub use client_ip::ClientIp;
pub use extract::{Inject, Json, Path, Query, Validated};
pub use plugin::{AppBuilderHttpExt, HttpPlugin, RouteRegistry, build_router};
pub use resource::{Action, Resource, ResourceActions};
pub use response::{Created, Located, NoContent, PageParams, Paginated};
pub use routes::Routes;
pub use serve::{ServeCommand, serve};
