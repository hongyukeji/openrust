//! 模块路由组合；具体处理器位于 http/controllers。

mod api;
mod web;

pub(crate) use api::response_marker_middleware;

#[allow(clippy::struct_excessive_bools)] // 编译能力独立决定示例路由是否可挂载。
pub(crate) struct RouteCapabilities {
    pub(crate) database: bool,
    #[cfg(feature = "session")]
    pub(crate) session: bool,
    pub(crate) view: bool,
    pub(crate) i18n: bool,
    pub(crate) summary_origin: Option<String>,
    #[cfg(feature = "auth-oidc")]
    pub(crate) oidc: bool,
}

pub(crate) fn register(
    routes: openrust::http::Routes,
    capabilities: &RouteCapabilities,
    repository: openrust::config::repository::ConfigRepository,
) -> openrust::http::Routes {
    let routes = routes.group(
        openrust::http::RouteGroup::new().middleware_group("api"),
        |routes| api::register(routes, capabilities, repository),
    );
    web::register(routes, capabilities.view)
}
