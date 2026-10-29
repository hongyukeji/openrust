//! 示例网页路由，复用应用显式配置的 Views 服务。

pub(super) fn register(
    routes: openrust::http::Routes,
    view_enabled: bool,
) -> openrust::http::Routes {
    #[cfg(not(feature = "view"))]
    let _ = view_enabled;
    routes.group(
        openrust::http::RouteGroup::new()
            .prefix("/example/pages")
            .middleware_group("example.response"),
        |pages| {
            #[cfg(feature = "view")]
            let pages = if view_enabled {
                pages.get(
                    "/about",
                    crate::modules::example::http::controllers::pages::about_page,
                )
            } else {
                pages
            };
            pages.fallback(pages_not_found)
        },
    )
}

async fn pages_not_found(
    request: openrust::http::axum::extract::Request,
) -> impl openrust::http::axum::response::IntoResponse {
    (
        openrust::http::axum::http::StatusCode::NOT_FOUND,
        format!("示例页面不存在：{}", request.uri().path()),
    )
}
