//! 路由声明、匹配元数据、URL生成和重定向目标。
//!
//! Router持有唯一声明状态，Axum承担实际网络适配；完整官方Routing、Contracts及Provider行为仍按对应矩阵验收。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod action;
pub(crate) mod domain;
pub mod methods;
pub mod redirector;
pub mod registration;
pub mod resource;
pub mod route;
pub mod router;
pub mod url_generator;

pub use action::RouteAction;
pub use methods::{
    ManagedHandler, RouteMethods, any, delete, get, head, on, options, patch, post, put, query,
};
pub use redirector::Redirector;
pub use registration::RouteRegistry;
pub use resource::{Action, Resource, ResourceActions};
pub use route::{Route, RouteResolver};
pub use router::{RouteGroup, Router};
pub use url_generator::{UrlGenerator, UrlSigningKeys};
