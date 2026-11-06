//! 根应用 Web 路由；默认静态首页由 server 配置挂载，可在此追加应用页面。

pub(crate) fn register(routes: openrust::routing::Router) -> openrust::routing::Router {
    routes
}
