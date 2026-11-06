//! 默认应用控制器；可从 `routes/api.rs` 直接注册。

use openrust::core::ApiResponse;
use openrust::foundation::ApplicationState;
use openrust::http::axum::extract::State;

/// 返回应用名称。
pub async fn index(
    State(state): State<ApplicationState>,
) -> ApiResponse<std::collections::BTreeMap<&'static str, String>> {
    ApiResponse::success(std::collections::BTreeMap::from([(
        "name",
        state.config().app.name.clone(),
    )]))
}
