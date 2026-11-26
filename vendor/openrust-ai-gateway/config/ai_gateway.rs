use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        "enabled": env.get_utf8("AI_GATEWAY_ENABLED")?.is_some_and(|value| {
            value.eq_ignore_ascii_case("true") || value == "1"
        }),
        "unversioned_paths": false,
        "models": {
            // "support-chat": {"provider": "deepseek", "model": "上游模型 ID"},
        },
        "account_session_ttl": null,
        "accounts": {
            // "deepseek": {"cooldown_seconds": 30, "max_concurrent_requests": 4},
        },
        "groups": {
            // "support-team": {"models": ["support-chat"], "providers": ["deepseek"]},
        },
        "usage": {
            "enabled": false,
            "connection": null,
        },
        "billing": {
            "enabled": false,
            "version": "",
            "upstream": {},
            "downstream": {},
        },
        "managed_keys": {
            "enabled": false,
            "connection": null,
        },
        "keys": {
            // "support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"]},
        }
    }))
}
