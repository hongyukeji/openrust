use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

/*
|--------------------------------------------------------------------------
| 订阅计费账户
|--------------------------------------------------------------------------
| CASHIER_DRIVER 默认 stripe。Stripe 与 Paddle 的 API 密钥、Webhook 密钥
| 分别配置。CASHIER_LIVE 默认 false；生产环境需显式启用并使用生产密钥。
| Paddle 原生 PADDLE_AUTH_CODE 优先于 PADDLE_API_KEY，PADDLE_SANDBOX 默认 false。
| 显式 CASHIER_LIVE 兼容旧账户模式并优先于 PADDLE_SANDBOX。
| drivers 为额外具名账户，覆盖同名供应商原生配置；包启动不访问网络或数据库。
*/
pub(crate) fn load(environment: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        "default":get(environment,"CASHIER_DRIVER","stripe")?,
        "key":get(environment,"STRIPE_KEY","")?,
        "secret":get(environment,"STRIPE_SECRET","")?,
        "webhook":{"secret":get(environment,"STRIPE_WEBHOOK_SECRET","")?},
        "live":live(environment)?,
        "paddle":{
            "api_key":environment.get_utf8("PADDLE_AUTH_CODE")?.or(environment.get_utf8("PADDLE_API_KEY")?).unwrap_or(""),
            "webhook_secret":get(environment,"PADDLE_WEBHOOK_SECRET","")?,
            "sandbox":match environment.get_utf8("CASHIER_LIVE")? { Some(_) => !live(environment)?, None => boolean(environment,"PADDLE_SANDBOX",false)? }
        },
        "drivers":{}

    }))
}

fn get(env: &Environment, key: &str, default: &str) -> Result<String, ConfigError> {
    Ok(env.get_utf8(key)?.unwrap_or(default).to_owned())
}
fn boolean(env: &Environment, key: &str, default: bool) -> Result<bool, ConfigError> {
    match env.get_utf8(key)? {
        None => Ok(default),
        Some("true" | "(true)" | "1") => Ok(true),
        Some("false" | "(false)" | "0") => Ok(false),
        _ => Err(ConfigError::InvalidType {
            key: key.into(),
            expected: "a boolean",
        }),
    }
}
fn live(env: &Environment) -> Result<bool, ConfigError> {
    match env
        .get_utf8("CASHIER_LIVE")?
        .unwrap_or("false")
        .to_ascii_lowercase()
        .as_str()
    {
        "true" | "(true)" | "1" => Ok(true),
        "false" | "(false)" | "0" => Ok(false),
        _ => Err(ConfigError::InvalidType {
            key: "CASHIER_LIVE".into(),
            expected: "a boolean",
        }),
    }
}
