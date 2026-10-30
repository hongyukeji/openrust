//! 账户凭据只进入认证服务，不实现 Debug 或把秘密放入 URL。

use openrust::validation::{Rule, Rules, Validate};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CredentialsInput {
    pub(crate) email: String,

    pub(crate) password: String,
}
#[cfg(feature = "auth-mfa")]
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FactorInput {
    pub(crate) email: String,

    pub(crate) password: String,

    pub(crate) code: String,
}
#[cfg(feature = "auth-mfa")]
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChallengeInput {
    pub(crate) challenge: String,

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

impl Validate for CredentialsInput {
    fn rules() -> Rules {
        Rules::new()
            .field(
                "email",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::email(),
                    Rule::length(None, Some(320)),
                ],
            )
            .field(
                "password",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(1024)),
                ],
            )
    }
}

#[cfg(feature = "auth-mfa")]
impl Validate for FactorInput {
    fn rules() -> Rules {
        Rules::new()
            .field(
                "email",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::email(),
                    Rule::length(None, Some(320)),
                ],
            )
            .field(
                "password",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(1024)),
                ],
            )
            .field(
                "code",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(6), Some(128)),
                ],
            )
    }
}

#[cfg(feature = "auth-mfa")]
impl Validate for ChallengeInput {
    fn rules() -> Rules {
        Rules::new()
            .field(
                "challenge",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(512)),
                ],
            )
            .field(
                "code",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(6), Some(128)),
                ],
            )
    }
}
