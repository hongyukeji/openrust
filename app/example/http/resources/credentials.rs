//! 认证返回 DTO，不泄露服务内部类型。

#[derive(serde::Serialize)]
pub(crate) struct CredentialsResponse {
    pub(crate) user_id: i64,
    pub(crate) token_type: &'static str,
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) refresh_expires_at: String,
}
