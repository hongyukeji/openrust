use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    let mut config = json!({
        "expiration": super::env::optional_integer(env, "SANCTUM_EXPIRATION")?,
        "token_prefix": env.get_utf8("SANCTUM_TOKEN_PREFIX")?.unwrap_or_default(),
    });
    if let Some(domains) = env.get_utf8("SANCTUM_STATEFUL_DOMAINS")? {
        config["stateful"] = json!(domains);
    }
    Ok(config)
}
