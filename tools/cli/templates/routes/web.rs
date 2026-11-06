//! 应用 Web 路由与全局 fallback；需要会话的路由应显式声明 `web` middleware 组。

use openrust::prelude::*;

pub fn register(routes: Router) -> Router {
    routes.fallback(not_found)
}

async fn not_found(
    request: openrust::http::axum::extract::Request,
) -> impl openrust::http::axum::response::IntoResponse {
    (
        openrust::http::axum::http::StatusCode::NOT_FOUND,
        format!("未找到路由：{}", request.uri().path()),
    )
}
