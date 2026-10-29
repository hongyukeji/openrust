//! 模块路由组合；具体处理器位于 http/controllers。

mod api;
mod web;

pub(crate) use api::response_marker_middleware;

#[allow(clippy::struct_excessive_bools)] // 编译能力独立决定示例路由是否可挂载。
pub(crate) struct RouteCapabilities {
    pub(crate) database: bool,
    pub(crate) view: bool,
    pub(crate) i18n: bool,
    pub(crate) summary_origin: Option<String>,
    #[cfg(all(
        feature = "auth-oidc",
        any(feature = "db-postgres", feature = "db-mysql")
    ))]
    pub(crate) oidc: bool,
}

pub(crate) fn register(
    routes: openrust::http::Routes,
    capabilities: &RouteCapabilities,
) -> openrust::http::Routes {
    web::register(api::register(routes, capabilities), capabilities.view)
}
