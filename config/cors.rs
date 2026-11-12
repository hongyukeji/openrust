use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::list;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    let paths = env
        .get_utf8("CORS_PATHS")?
        .map_or_else(|| vec!["api/*", "sanctum/csrf-cookie"], list);

    Ok(json!({
        "paths": paths,
        "allowed_methods": ["*"],
        "allowed_origins": ["*"],
        "allowed_origins_patterns": [],
        "allowed_headers": ["*"],
        "exposed_headers": [],
        "max_age": 0,
        "supports_credentials": false,
    }))
}
