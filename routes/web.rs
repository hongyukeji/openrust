//! 框架基础路由；业务路由由应用模块通过公开 `ServiceProvider` 注册。

pub(crate) fn register(routes: openrust::http::Routes) -> openrust::http::Routes {
    routes.group(
        openrust::http::RouteGroup::new().middleware_group("api"),
        openrust::routes::framework_routes,
    )
}
