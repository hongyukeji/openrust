//! Route registration and dispatch.

pub(crate) mod binding;
mod error;
pub mod middleware;
mod route;
mod route_group;
mod route_pattern;
mod router;
mod static_mount;

pub use binding::BindingContext;
pub use error::RoutingError;
pub use middleware::MiddlewareEntry;
pub use route::Route;
pub use route_group::RouteGroup;
pub use router::{IntoRouteHandler, Router};
