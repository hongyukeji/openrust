//! 回调只接收一次性 code/state；不接受请求选择 provider、issuer 或 callback 地址。
use openrust::validation::{Rule, Rules, Validate};
#[derive(serde::Deserialize)]
pub(crate) struct CallbackQuery {
    pub(crate) state: String,
    pub(crate) code: Option<String>,
    pub(crate) error: Option<String>,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindInput {
    pub(crate) email: String,

    pub(crate) password: String,

    pub(crate) state: String,

    pub(crate) code: String,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnbindInput {
    pub(crate) email: String,

    pub(crate) password: String,

    pub(crate) subject: String,
}

/// 浏览器绑定的发起请求；身份来自 Session，不接受客户端 owner。
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrowserBindInput {
    pub(crate) email: String,

    pub(crate) password: String,
}

/// 句柄只在受保护的请求体中传递，不作为重定向目标。
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogoutInput {
    pub(crate) handle: String,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttachSessionInput {
    pub(crate) handle: String,

    pub(crate) email: String,

    pub(crate) password: String,
}

impl Validate for BindInput {
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
                "state",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(256)),
                ],
            )
            .field(
                "code",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(4096)),
                ],
            )
    }
}

impl Validate for UnbindInput {
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
                "subject",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(1024)),
                ],
            )
    }
}

impl Validate for BrowserBindInput {
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

impl Validate for LogoutInput {
    fn rules() -> Rules {
        Rules::new().field(
            "handle",
            [
                Rule::required(),
                Rule::string(),
                Rule::length(Some(1), Some(512)),
            ],
        )
    }
}

impl Validate for AttachSessionInput {
    fn rules() -> Rules {
        Rules::new()
            .field(
                "handle",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(1), Some(512)),
                ],
            )
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
