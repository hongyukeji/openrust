//! 应用选择官方安装默认，并通过公开回调扩展为单框组合账号。

use openrust::{
    auth::fortify::{FortifyConfig, TwoFactorOptions},
    config::{ConfigContext, ConfigError},
};

/// 组合账号由应用回调解释，框架包回退的username=email保持不变。
#[allow(clippy::unnecessary_wraps, reason = "配置清单统一使用可失败函数")]
pub fn config(_: &ConfigContext) -> Result<FortifyConfig, ConfigError> {
    let mut config = FortifyConfig::installed();
    config.username = "account".into();
    // 邮箱/用户名/手机号的大小写由组合入口逐类处理，不统一改写账户输入。
    config.lowercase_usernames = false;
    Ok(config)
}

/// 启用绑定确认与近期密码确认，沿同一配置仓储登记。
#[allow(clippy::unnecessary_wraps, reason = "配置清单统一使用可失败函数")]
pub fn options(_: &ConfigContext) -> Result<serde_json::Value, ConfigError> {
    Ok(serde_json::json!({
        "two-factor-authentication": TwoFactorOptions::installed(),
    }))
}

/// 根应用认证profile；默认官方Web，AUTH_PROFILE=jwt-api才显式选用无Cookie API。
pub fn profile(ctx: &ConfigContext) -> Result<String, ConfigError> {
    let profile = ctx.env_or("AUTH_PROFILE", "web");
    if !matches!(profile.as_str(), "web" | "jwt-api") {
        return Err(ConfigError::Invalid(
            "AUTH_PROFILE必须为web或jwt-api".into(),
        ));
    }
    Ok(profile)
}
