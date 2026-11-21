//! Route registration and dispatch.

pub(crate) mod binding;
pub(crate) mod console;
mod controller_dispatcher;
mod error;
mod form_request_handler;
pub mod middleware;
mod model_response;
mod redirector;
mod resource;
mod response_output;
mod route;
pub(crate) mod route_cache;
mod route_group;
mod route_pattern;
mod route_url_generator;
mod router;
mod static_mount;
mod url_generator;

pub use binding::BindingContext;
pub use controller_dispatcher::ControllerDispatcher;
pub use error::RoutingError;
pub use middleware::MiddlewareEntry;
#[doc(hidden)]
pub use model_response::ModelResponse;
pub(crate) use resource::ResourceKind;
pub use resource::{ResourceAction, ResourceController};
pub use route::{Route, RouteMetadata};
pub use route_group::RouteGroup;
pub use router::{IntoRouteHandler, RouteInfo, Router};

pub use route_url_generator::RouteUrlGenerator;
pub use url_generator::{
    RouteParameter, RouteParameters, SignatureMode, SignatureQueryFilter, UrlError, UrlGenerator,
    UrlResult, UrlValue,
};

pub(crate) use url_generator::{encode_query_value, url_uri, valid_http_url};

pub(crate) use redirector::redirect_response;
pub use redirector::{RedirectOptions, Redirector};
