//! 应用 HTTP 路由装配。

pub(crate) fn register(routes: openrust::http::Routes) -> openrust::http::Routes {
    openrust::routes::framework_routes(routes)
}
