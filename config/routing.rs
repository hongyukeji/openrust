use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    let prefix = env.get_utf8("API_PREFIX")?.unwrap_or("/api");
    Ok(json!({
        "api_prefix": prefix.trim_matches('/'),
    }))
}
