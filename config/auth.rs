use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        "defaults": {
            "guard": get(env, "AUTH_GUARD", "web")?,
            "passwords": get(env, "AUTH_PASSWORD_BROKER", "users")?,
        },
        "guards": {
            "web": {"driver": "session", "provider": "users"},
        },
        "providers": {
            "users": {"driver": "eloquent", "model": get(env, "AUTH_MODEL", "app::models::User")?},
        },
        "passwords": {
            "users": {
                "provider": "users",
                "table": get(env, "AUTH_PASSWORD_RESET_TOKEN_TABLE", "password_reset_tokens")?,
                "expire": 60,
                "throttle": 60,
            },
        },
        "password_timeout": get(env, "AUTH_PASSWORD_TIMEOUT", 10800)?,
    }))
}
