use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, optional_integer};

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        "enabled": get(env, "REVERB_ENABLED", false)?,
        "mode": get(env, "REVERB_MODE", "embedded")?,
        "default": get(env, "REVERB_SERVER", "reverb")?,
        "servers": {
            "reverb": {
                "host": get(env, "REVERB_SERVER_HOST", "0.0.0.0")?,
                "port": get(env, "REVERB_SERVER_PORT", 8080)?,
                "path": get(env, "REVERB_SERVER_PATH", "")?,
                "hostname": get(env, "REVERB_HOST", Value::Null)?,
                "options": {"tls": {}},
                "max_request_size": get(env, "REVERB_MAX_REQUEST_SIZE", 10_000)?,
                "scaling": {
                    "enabled": get(env, "REVERB_SCALING_ENABLED", false)?,
                    "channel": get(env, "REVERB_SCALING_CHANNEL", "reverb")?,
                    "server": {
                        "url": get(env, "REDIS_URL", Value::Null)?,
                        "host": get(env, "REDIS_HOST", "127.0.0.1")?,
                        "port": get(env, "REDIS_PORT", 6379)?,
                        "username": get(env, "REDIS_USERNAME", Value::Null)?,
                        "password": get(env, "REDIS_PASSWORD", Value::Null)?,
                        "database": get(env, "REDIS_DB", 0)?,
                        "timeout": get(env, "REDIS_TIMEOUT", 60)?,
                    },
                },
                "pulse_ingest_interval": get(env, "REVERB_PULSE_INGEST_INTERVAL", 15)?,
                "telescope_ingest_interval": get(env, "REVERB_TELESCOPE_INGEST_INTERVAL", 15)?,
            },
        },
        "apps": {
            "provider": "config",
            "apps": [{
                "key": get(env, "REVERB_APP_KEY", Value::Null)?,
                "secret": get(env, "REVERB_APP_SECRET", Value::Null)?,
                "app_id": get(env, "REVERB_APP_ID", Value::Null)?,
                "options": publish_options(env)?,
                "allowed_origins": ["*"],
                "ping_interval": get(env, "REVERB_APP_PING_INTERVAL", 60)?,
                "activity_timeout": get(env, "REVERB_APP_ACTIVITY_TIMEOUT", 30)?,
                "max_connections": optional_integer(env, "REVERB_APP_MAX_CONNECTIONS")?,
                "max_message_size": get(env, "REVERB_APP_MAX_MESSAGE_SIZE", 10_000)?,
                "accept_client_events_from": get(env, "REVERB_APP_ACCEPT_CLIENT_EVENTS_FROM", "members")?,
            }],
        },
    }))
}

pub(super) fn publish_options(env: &Environment) -> Result<Value, ConfigError> {
    let scheme = get(env, "REVERB_SCHEME", "https")?;
    Ok(json!({
        "host": get(env, "REVERB_HOST", Value::Null)?,
        "port": get(env, "REVERB_PORT", 443)?,
        "scheme": scheme,
        "useTLS": scheme == "https",
    }))
}
