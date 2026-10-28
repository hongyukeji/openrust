//! 欢迎入口只接受空对象，目的地从已验证账户记录读取。

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WelcomeInput {}
