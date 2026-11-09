//! 使用 Passport 公钥和当前令牌仓储保护示例资源。

use openrust::http::{Inject, Json, Request};
use openrust_passport::{OAuthError, ResourceServer};

pub(crate) async fn show(
    Inject(server): Inject<ResourceServer>,
    request: Request,
) -> Result<Json<serde_json::Value>, OAuthError> {
    server.validate_authenticated_request(&request).await?;
    let client = request
        .attributes()
        .get("oauth_client_id")
        .ok()
        .flatten()
        .and_then(|value| value.downcast::<String>().ok())
        .ok_or_else(|| OAuthError::server_error("OAuth client projection unavailable"))?;
    Ok(Json(serde_json::json!({ "client_id": client.as_str() })))
}
