//! 根应用的身份 API；业务内容由应用层维护。

use openrust::core::ApiResponse;
use openrust::foundation::ApplicationState;
use openrust::http::axum::extract::State;
use std::collections::BTreeMap;

/// 返回当前应用名称与本应用构建版本。
pub async fn index(
    State(state): State<ApplicationState>,
) -> ApiResponse<BTreeMap<&'static str, String>> {
    ApiResponse::success(BTreeMap::from([
        ("name", state.config().app.name.clone()),
        ("version", env!("CARGO_PKG_VERSION").to_owned()),
    ]))
}
