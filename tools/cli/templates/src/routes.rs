use openrust::prelude::*;

pub fn register(routes: Routes) -> Routes {
{{routes}}
        .fallback(not_found)
}
{{auth_handler}}
async fn root() -> Json<std::collections::BTreeMap<&'static str, &'static str>> {
    Json(std::collections::BTreeMap::from([(
        "name",
        env!("CARGO_PKG_NAME"),
    )]))
}

async fn not_found(
    request: openrust::http::axum::extract::Request,
) -> impl openrust::http::axum::response::IntoResponse {
    (
        openrust::http::axum::http::StatusCode::NOT_FOUND,
        format!("未找到路由：{}", request.uri().path()),
    )
}
