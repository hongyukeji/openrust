#![doc = include_str!("README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub(crate) mod authority;
#[cfg(feature = "http-client")]
pub mod client;
pub mod client_ip;
pub mod cookie;
pub mod domain;
pub mod extract;
pub mod health;
#[cfg(feature = "metrics")]
pub mod metrics;
pub mod middleware;
pub mod plugin;
pub mod precognition;
pub mod rate_limit;
pub mod redirect;
pub mod request;
pub mod resource;
pub mod response;
pub mod route;
pub mod routes;
pub mod routing;
pub mod serve;
pub(crate) mod site;
pub mod sort;
pub mod transport;
pub mod url;

pub use crate::i18n::RequestLocale;
pub use axum;
#[cfg(feature = "http-client")]
pub use client::{
    CircuitBreakerPolicy, DependencyClient, DependencyPolicy, DependencyRequest, HttpBody,
    HttpClient, HttpClientConfig, HttpClientError, HttpRequest, HttpResponse, HttpResponseBody,
    HttpTargetPolicy, HttpTransport, HttpTransportRequest, IdempotentDependencyRequest,
    IdempotentHttpRequest, MultipartForm, RetryPolicy, TransportResponse,
};
pub use client_ip::ClientIp;
pub use cookie::{
    Cookie, CookieFactory, CookieJar, CookieOptions, CookieResponse, CookieResponseExt,
    CookieServiceProvider, SameSite,
};
pub use domain::DomainParams;
pub use extract::{Inject, Json, Path, Query, Validated};
pub use plugin::{HttpServiceProvider, RouteRegistry, build_router};
pub use precognition::handle_precognitive_requests;
pub use rate_limit::{RateLimitKey, RateLimitLayer};
#[cfg(feature = "session")]
pub use redirect::RedirectErrors;
pub use redirect::{RedirectResponse, Redirector};
pub use request::{
    InputNormalization, InputOutcome, InputOutcomeMut, InputTruthy, Request, RequestInfo, Truthy,
};
pub use resource::{Action, Resource, ResourceActions};
pub use response::{Created, Located, NoContent, PageParams, Paginated};
pub use route::{Route, RouteResolver};
pub use routes::{RouteGroup, Routes};
pub use routing::{ManagedHandler, RouteMethods};
pub use serve::{ServeCommand, serve};
pub use sort::{SortDirection, SortField, SortParams};
pub use transport::RequestSecurity;
pub use url::RouteUrlGenerator;
