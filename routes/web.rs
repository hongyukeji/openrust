//! 框架基础路由；业务路由由应用模块通过公开 Plugin 注册。

pub(crate) fn register(routes: openrust::http::Routes) -> openrust::http::Routes {
    openrust::routes::framework_routes(routes)
}
