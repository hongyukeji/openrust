use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::{env::get, reverb};

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    let cluster = env.get_utf8("PUSHER_APP_CLUSTER")?.unwrap_or("mt1");
    let pusher_host = format!("api-{cluster}.pusher.com");
    let pusher_host = env
        .get_utf8("PUSHER_HOST")?
        .filter(|value| !value.is_empty())
        .unwrap_or(&pusher_host);
    let scheme = get(env, "PUSHER_SCHEME", "https")?;
    Ok(json!({
        "default": get(env, "BROADCAST_CONNECTION", "null")?,
        "connections": {
            "reverb": {
                "driver": "reverb",
                "key": get(env, "REVERB_APP_KEY", Value::Null)?,
                "secret": get(env, "REVERB_APP_SECRET", Value::Null)?,
                "app_id": get(env, "REVERB_APP_ID", Value::Null)?,
                "options": reverb::publish_options(env)?,
                "client_options": {},
            },
            "pusher": {
                "driver": "pusher",
                "key": get(env, "PUSHER_APP_KEY", Value::Null)?,
                "secret": get(env, "PUSHER_APP_SECRET", Value::Null)?,
                "app_id": get(env, "PUSHER_APP_ID", Value::Null)?,
                "options": {
                    "cluster": get(env, "PUSHER_APP_CLUSTER", Value::Null)?,
                    "host": pusher_host,
                    "port": get(env, "PUSHER_PORT", 443)?,
                    "scheme": scheme,
                    "encrypted": true,
                    "useTLS": scheme == "https",
                },
                "client_options": {},
            },
            "ably": {"driver": "ably", "key": get(env, "ABLY_KEY", Value::Null)?},
            "mercure": {
                "driver": "mercure",
                "url": get(env, "MERCURE_URL", Value::Null)?,
                "public_url": get(env, "MERCURE_PUBLIC_URL", Value::Null)?,
                "secret": get(env, "MERCURE_JWT_SECRET", Value::Null)?,
                "encryption_key": get(env, "MERCURE_ENCRYPTION_KEY", Value::Null)?,
                "claims": {
                    "iss": get(env, "MERCURE_JWT_ISSUER", Value::Null)?,
                    "client_id": get(env, "APP_NAME", Value::Null)?,
                },
                "cookie_name": get(env, "MERCURE_COOKIE_NAME", Value::Null)?,
                "subscribe_expiration": get(env, "MERCURE_SUBSCRIBE_EXPIRATION", 5)?,
                "async": get(env, "MERCURE_ASYNC", false)?,
            },
            "log": {"driver": "log"},
            "null": {"driver": "null"},
        },
    }))
}
