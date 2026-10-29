use openrust::prelude::*;
use openrust::{core::AppState, http::axum::extract::State};

pub fn register(routes: Routes) -> Routes {
{{routes}}
        .fallback(not_found)
}
{{auth_handler}}
async fn root(State(state): State<AppState>) -> Json<std::collections::BTreeMap<&'static str, String>> {
    Json(std::collections::BTreeMap::from([(
        "name",
        state.config().app.name.clone(),
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
