use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub fn load(env: &Environment) -> Result<Value, ConfigError> {
    // 驱动使用应用声明的具名配置。此处只选择默认配置，不读取凭据或联系供应商。
    Ok(json!({
        "default": env.get_utf8("MODERATION_DRIVER")?,
        "credential_directory": env.get_utf8("MODERATION_CREDENTIAL_DIRECTORY")?,
        "profile_revisions": {},
        "account_rate_limits": {},
        "drivers": {}
    }))
}
