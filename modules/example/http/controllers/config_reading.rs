//! 公开业务问候语，固定键由服务端选择。

use openrust::config::repository::ConfigRepository;
use openrust::http::Json;

pub(crate) fn show(repository: &ConfigRepository) -> openrust::Result<Json<serde_json::Value>> {
    let greeting = crate::modules::example::services::config_reading::greeting(repository)?;
    Ok(Json(serde_json::json!({"greeting": greeting})))
}

pub(crate) fn feature(repository: &ConfigRepository) -> openrust::Result<Json<serde_json::Value>> {
    let enabled = crate::modules::example::services::config_reading::feature_enabled(repository)?;
    Ok(Json(serde_json::json!({"enabled": enabled})))
}

pub(crate) fn review(repository: &ConfigRepository) -> openrust::Result<Json<serde_json::Value>> {
    let greeting = crate::modules::example::services::config_reading::review(repository)?;
    Ok(Json(serde_json::json!({"greeting": greeting})))
}
