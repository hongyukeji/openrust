//! Route registration and dispatch.

pub(crate) mod binding;
pub(crate) mod console;
mod controller_dispatcher;
mod error;
pub mod middleware;
mod model_response;
mod redirector;
mod route;
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
pub use route::Route;
pub use route_group::RouteGroup;
pub use router::{IntoRouteHandler, RouteInfo, Router};

pub use route_url_generator::RouteUrlGenerator;
pub use url_generator::{
    RouteParameter, RouteParameters, SignatureMode, SignatureQueryFilter, UrlError, UrlGenerator,
    UrlResult, UrlValue,
};

pub(crate) use url_generator::url_uri;

pub use redirector::{RedirectOptions, Redirector};
