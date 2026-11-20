use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

/*
|--------------------------------------------------------------------------
| 订阅计费账户
|--------------------------------------------------------------------------
| CASHIER_DRIVER 默认 stripe。Stripe 与 Paddle 的 API 密钥、Webhook 密钥
| 分别配置。CASHIER_LIVE 默认 false；生产环境需显式启用并使用生产密钥。
| 配置加载与包启动不访问网络或数据库，API 凭据在解析账户时才检查。
*/
pub(crate) fn load(environment: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        "default":get(environment,"CASHIER_DRIVER","stripe")?,
        "drivers":{
            "stripe":{"driver":"stripe","secret":get(environment,"STRIPE_SECRET","")?,"webhook_secret":get(environment,"STRIPE_WEBHOOK_SECRET","")?,"live":live(environment)?},
            "paddle":{"driver":"paddle","secret":get(environment,"PADDLE_API_KEY","")?,"webhook_secret":get(environment,"PADDLE_WEBHOOK_SECRET","")?,"live":live(environment)?}
        }
    }))
}

fn get(env: &Environment, key: &str, default: &str) -> Result<String, ConfigError> {
    Ok(env.get_utf8(key)?.unwrap_or(default).to_owned())
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
