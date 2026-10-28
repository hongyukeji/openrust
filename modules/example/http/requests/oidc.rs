//! 回调只接收一次性 code/state；不接受请求选择 provider、issuer 或 callback 地址。
use openrust::validation::Validate;
#[derive(serde::Deserialize)]
pub(crate) struct CallbackQuery {
    pub(crate) state: String,
    pub(crate) code: Option<String>,
    pub(crate) error: Option<String>,
}
#[derive(serde::Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindInput {
    #[validate(email, length(max = 320))]
    pub(crate) email: String,
    #[validate(length(min = 1, max = 1024))]
    pub(crate) password: String,
    #[validate(length(min = 1, max = 256))]
    pub(crate) state: String,
    #[validate(length(min = 1, max = 4096))]
    pub(crate) code: String,
}
#[derive(serde::Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnbindInput {
    #[validate(email, length(max = 320))]
    pub(crate) email: String,
    #[validate(length(min = 1, max = 1024))]
    pub(crate) password: String,
    #[validate(length(min = 1, max = 1024))]
    pub(crate) subject: String,
}

/// 浏览器绑定的发起请求；身份来自 Session，不接受客户端 owner。
#[derive(serde::Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct BrowserBindInput {
    #[validate(email, length(max = 320))]
    pub(crate) email: String,
    #[validate(length(min = 1, max = 1024))]
    pub(crate) password: String,
}

/// 句柄只在受保护的请求体中传递，不作为重定向目标。
#[derive(serde::Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogoutInput {
    #[validate(length(min = 1, max = 512))]
    pub(crate) handle: String,
}
#[derive(serde::Deserialize, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttachSessionInput {
    #[validate(length(min = 1, max = 512))]
    pub(crate) handle: String,
    #[validate(email, length(max = 320))]
    pub(crate) email: String,
    #[validate(length(min = 1, max = 1024))]
    pub(crate) password: String,
}
