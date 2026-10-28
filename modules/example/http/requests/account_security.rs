//! 账户凭据只进入认证服务，不实现 Debug 或把秘密放入 URL。

use openrust::validation::Validate;

#[derive(serde::Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialsInput {
    #[validate(email, length(max = 320))]
    pub(crate) email: String,
    #[validate(length(min = 1, max = 1024))]
    pub(crate) password: String,
}
#[cfg(feature = "auth-mfa")]
#[derive(serde::Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct FactorInput {
    #[validate(email, length(max = 320))]
    pub(crate) email: String,
    #[validate(length(min = 1, max = 1024))]
    pub(crate) password: String,
    #[validate(length(min = 6, max = 128))]
    pub(crate) code: String,
}
#[cfg(feature = "auth-mfa")]
#[derive(serde::Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChallengeInput {
    #[validate(length(min = 1, max = 512))]
    pub(crate) challenge: String,
    #[validate(length(min = 6, max = 128))]
    pub(crate) code: String,
}
#[derive(Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Density {
    Compact,
    #[default]
    Comfortable,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreferenceInput {
    pub(crate) density: Density,
}
