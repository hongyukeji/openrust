//! 根应用的无状态 API 声明；启动装配负责统一前缀和中间件组。

/// 声明相对于应用 API 前缀的路由。
pub(crate) fn register(routes: openrust::routing::Router) -> openrust::routing::Router {
    routes.get("/", crate::app::http::controllers::application_info::index)
}
