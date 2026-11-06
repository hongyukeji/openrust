//! HTTP Provider 与路由注册 API。
//!
//! 实现与应用路由装配入口位于 [`crate::routes`]。

pub use crate::routes::{HttpServiceProvider, RouteRegistry, build_router};
