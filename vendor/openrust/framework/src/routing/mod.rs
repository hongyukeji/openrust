//! Route registration and dispatch.

pub(crate) mod binding;
pub(crate) mod console;
mod controller_dispatcher;
mod error;
pub mod middleware;
mod model_response;
mod route;
mod route_group;
mod route_pattern;
mod router;
mod static_mount;

pub use binding::BindingContext;
pub use controller_dispatcher::ControllerDispatcher;
pub use error::RoutingError;
pub use middleware::MiddlewareEntry;
#[doc(hidden)]
pub use model_response::ModelResponse;
pub use route::Route;
pub use route_group::RouteGroup;
pub use router::{IntoRouteHandler, RouteInfo, Router};
