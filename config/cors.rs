use openrust::config::Value;
use serde_json::json;

pub(super) fn load() -> Value {
    json!({
        "default_paths": ["sanctum/csrf-cookie"],
        "allowed_methods": ["*"],
        "allowed_origins": ["*"],
        "allowed_origins_patterns": [],
        "allowed_headers": ["*"],
        "exposed_headers": [],
        "max_age": 0,
        "supports_credentials": false,
    })
}
