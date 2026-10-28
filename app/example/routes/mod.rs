//! 模块路由组合；具体处理器位于 http/controllers。

mod api;
mod web;

pub(crate) fn register(
    routes: openrust::http::Routes,
    database_enabled: bool,
    view_enabled: bool,
) -> openrust::http::Routes {
    web::register(api::register(routes, database_enabled), view_enabled)
}
