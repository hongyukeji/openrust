//! HTTP plugin 与路由注册 API 的兼容模块。
//!
//! 实现与应用路由装配入口位于 [`crate::routes`]。

pub use crate::routes::{AppBuilderHttpExt, HttpPlugin, RouteRegistry, build_router};
