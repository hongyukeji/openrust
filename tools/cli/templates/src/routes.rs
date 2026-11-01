use openrust::prelude::*;
use crate::app::http::controllers::home::index as root;

pub fn register(routes: Routes) -> Routes {
    routes
        .group(RouteGroup::new().middleware_group("api"), |routes| {
{{routes}}
        })
        .fallback(not_found)
}
{{auth_handler}}
async fn not_found(
    request: openrust::http::axum::extract::Request,
) -> impl openrust::http::axum::response::IntoResponse {
    (
        openrust::http::axum::http::StatusCode::NOT_FOUND,
        format!("未找到路由：{}", request.uri().path()),
    )
}
