use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, list};

pub(super) struct Loaded<'a> {
    pub(super) config: Value,
    pub(super) name: &'a str,
    pub(super) public_path: Option<&'a str>,
    pub(super) url: String,
}

pub(super) fn load(env: &Environment) -> Result<Loaded<'_>, ConfigError> {
    let name = env.get_utf8("APP_NAME")?.unwrap_or("OpenRust");
    let host = env.get_utf8("APP_HOST")?.unwrap_or("127.0.0.1");
    let port = env.get_utf8("APP_PORT")?.unwrap_or("8000");
    let public_path = env.get_utf8("APP_PUBLIC_PATH")?;
    let url = env
        .get_utf8("APP_URL")?
        .map_or_else(|| format!("http://localhost:{port}"), str::to_owned);
    let port = port
        .parse::<u16>()
        .map_or_else(|_| json!(port), |port| json!(port));
    let config = json!({
        "name": name,
        "env": get(env, "APP_ENV", "production")?,
        "debug": get(env, "APP_DEBUG", false)?,
        "host": host,
        "port": port,
        "url": url,
        "frontend_url": get(env, "FRONTEND_URL", "http://localhost:3000")?,
        "asset_url": get(env, "ASSET_URL", Value::Null)?,
        "timezone": "UTC",
        "locale": get(env, "APP_LOCALE", "en")?,
        "fallback_locale": get(env, "APP_FALLBACK_LOCALE", "en")?,
        "faker_locale": get(env, "APP_FAKER_LOCALE", "en_US")?,
        "cipher": "AES-256-CBC",
        "key": get(env, "APP_KEY", Value::Null)?,
        "previous_keys": list(env.get_utf8("APP_PREVIOUS_KEYS")?.unwrap_or_default()),
        "maintenance": {
            "driver": get(env, "APP_MAINTENANCE_DRIVER", "file")?,
            "store": get(env, "APP_MAINTENANCE_STORE", "database")?,
        },
        "public_path": public_path,
        "public_url_prefix": env.get_utf8("APP_PUBLIC_URL_PREFIX")?.unwrap_or("/"),
    });
    Ok(Loaded {
        config,
        name,
        public_path,
        url,
    })
}
