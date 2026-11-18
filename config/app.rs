use std::net::IpAddr;

use openrust::{
    config::{ConfigError, Environment, Value},
    translation::canonical_locale,
};
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
    let env_host = env.get_utf8("APP_HOST")?;
    let host = env_host.unwrap_or("127.0.0.1");
    let port = env.get_utf8("APP_PORT")?.unwrap_or("8000");
    let public_path = env.get_utf8("APP_PUBLIC_PATH")?;
    let (url, url_explicit) = resolve_url(env, env_host, port)?;
    let port = port
        .parse::<u16>()
        .map_or_else(|_| json!(port), |port| json!(port));
    let config = json!({
        "name": name,
        "version": get(env, "APP_VERSION", env!("CARGO_PKG_VERSION"))?,
        "env": get(env, "APP_ENV", "production")?,
        "debug": get(env, "APP_DEBUG", false)?,
        "host": host,
        "port": port,
        "url": url,
        "url_explicit": url_explicit,
        "frontend_url": get(env, "FRONTEND_URL", "http://localhost:3000")?,
        "asset_url": get(env, "ASSET_URL", Value::Null)?,
        "timezone": get(env, "APP_TIMEZONE", "UTC")?,
        "locale": locale(env, "APP_LOCALE", "en")?,
        "fallback_locale": locale(env, "APP_FALLBACK_LOCALE", "en")?,
        "faker_locale": locale(env, "APP_FAKER_LOCALE", "en_US")?,
        "cipher": get(env, "APP_CIPHER", "AES-256-CBC")?,
        "key": get(env, "APP_KEY", Value::Null)?,
        "previous_keys": list(env.get_utf8("APP_PREVIOUS_KEYS")?.unwrap_or_default()),
        "maintenance": {
            "driver": get(env, "APP_MAINTENANCE_DRIVER", "file")?,
            "store": get(env, "APP_MAINTENANCE_STORE", "database")?,
        },
        "disabled_runners": list(env.get_utf8("APP_DISABLED_RUNNERS")?.unwrap_or_default()),
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

fn resolve_url(
    env: &Environment,
    host: Option<&str>,
    port: &str,
) -> Result<(String, bool), ConfigError> {
    if let Some(url) = env.get_utf8("APP_URL")? {
        return Ok((url.to_owned(), true));
    }

    let host = host.filter(|host| !host.is_empty()).unwrap_or("localhost");
    let address = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    let url = match address.parse::<IpAddr>() {
        Ok(ip) if ip.is_unspecified() || ip.is_loopback() => format!("http://localhost:{port}"),
        Ok(IpAddr::V6(_)) => format!("http://[{address}]:{port}"),
        _ => format!("http://{host}:{port}"),
    };
    Ok((url, false))
}

fn locale(env: &Environment, key: &str, default: &str) -> Result<String, ConfigError> {
    canonical_locale(env.get_utf8(key)?.unwrap_or(default)).map_err(|_| ConfigError::InvalidType {
        key: key.into(),
        expected: "a language identifier such as en, zh_CN or zh_TW",
    })
}
