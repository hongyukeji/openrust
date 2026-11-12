use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        "enabled": get(env, "RUNTIME_ENABLED", true)?,
        "http": get(env, "RUNTIME_HTTP", true)?,
        "queue": get(env, "RUNTIME_QUEUE", true)?,
        "schedule": get(env, "RUNTIME_SCHEDULE", true)?,
    }))
}
