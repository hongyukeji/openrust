//! 示例网页路由，复用应用显式配置的 Views 服务。

pub(super) fn register(
    routes: openrust::http::Routes,
    view_enabled: bool,
) -> openrust::http::Routes {
    #[cfg(feature = "view")]
    let routes = if view_enabled {
        routes.get(
            "/pages/about",
            crate::app::example::http::controllers::pages::about_page,
        )
    } else {
        routes
    };
    #[cfg(not(feature = "view"))]
    let _ = view_enabled;
    routes
}
