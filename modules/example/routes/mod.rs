//! 模块路由组合；具体处理器位于 http/controllers。

mod api;
mod web;

pub(crate) use api::ExampleResponseMarker;

#[allow(clippy::struct_excessive_bools)] // 编译能力独立决定示例路由是否可挂载。
pub(crate) struct RouteCapabilities {
    #[cfg(feature = "auth-fortify")]
    pub(crate) jwt_api_guard: Option<String>,
    pub(crate) database: bool,
    pub(crate) cookie: bool,
    #[cfg(feature = "session")]
    pub(crate) session: bool,
    pub(crate) view: bool,
    pub(crate) i18n: bool,
    pub(crate) logging_demo: bool,
    pub(crate) summary_origin: Option<String>,
    #[cfg(feature = "auth-oidc")]
    pub(crate) oidc: bool,
}

pub(crate) fn register(
    routes: openrust::routing::Router,
    capabilities: &RouteCapabilities,
    repository: openrust::config::repository::ConfigRepository,
) -> openrust::routing::Router {
    let routes = routes.group(
        openrust::routing::RouteGroup::new().middleware_group("api"),
        |routes| api::register(routes, capabilities, repository),
    );
    web::register(routes, capabilities.view)
}

/// 仅显式配置 reporting 连接时提供具名连接演示，不触发默认数据库业务选择。
pub(crate) fn reporting_connection(routes: openrust::routing::Router) -> openrust::routing::Router {
    routes.get(
        "/api/example/reporting-connection",
        super::http::controllers::database_connection::show,
    )
}
