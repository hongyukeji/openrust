use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | 个人访问令牌与 SPA
    |--------------------------------------------------------------------------
    |
    | SANCTUM_EXPIRATION 以分钟设置全局令牌时限，SANCTUM_TOKEN_PREFIX 设置令牌前缀。
    | SANCTUM_STATEFUL_DOMAINS 可指定第一方 SPA 的域名列表。
    |
    */
    let mut config = json!({
        "expiration": super::env::optional_integer(env, "SANCTUM_EXPIRATION")?,
        "token_prefix": env.get_utf8("SANCTUM_TOKEN_PREFIX")?.unwrap_or_default(),
    });
    if let Some(domains) = env.get_utf8("SANCTUM_STATEFUL_DOMAINS")? {
        config["stateful"] = json!(domains);
    }
    Ok(config)
}
