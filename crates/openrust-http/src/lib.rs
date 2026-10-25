#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod extract;
pub mod health;
pub mod middleware;
pub mod provider;
pub mod routes;
pub mod serve;

pub use axum;
pub use extract::{Inject, Validated};
pub use provider::{AppBuilderHttpExt, HttpProvider, RouteRegistry, build_router};
pub use routes::Routes;
pub use serve::{ServeCommand, serve};
