//! 认证业务的类型化请求。

#[derive(serde::Deserialize)]
pub(crate) struct EmailPasswordInput {
    pub(crate) email: String,
    pub(crate) password: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct VerificationInput {
    pub(crate) email: String,
    pub(crate) code: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct EmailInput {
    pub(crate) email: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct RefreshInput {
    pub(crate) refresh_token: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct PasswordResetInput {
    pub(crate) email: String,
    pub(crate) code: String,
    pub(crate) new_password: String,
}
