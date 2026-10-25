#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod extract;
pub mod health;
pub mod middleware;
pub mod plugin;
pub mod resource;
pub mod response;
pub mod routes;
pub mod serve;

pub use axum;
pub use extract::{Inject, Validated};
pub use plugin::{AppBuilderHttpExt, HttpPlugin, RouteRegistry, build_router};
pub use resource::{Action, Resource, ResourceActions};
pub use response::{Created, NoContent, PageParams, Paginated};
pub use routes::Routes;
pub use serve::{ServeCommand, serve};
