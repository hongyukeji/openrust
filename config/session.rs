use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, optional_bool, path, snake};

pub(super) fn load(
    base_path: &Path,
    env: &Environment,
    app_name: &str,
) -> Result<Value, ConfigError> {
    let cookie = format!("{}_session", snake(app_name));
    let same_site = match env.get_utf8("SESSION_SAME_SITE")? {
        Some(_) => get(env, "SESSION_SAME_SITE", Value::Null)?,
        None => Value::from("lax"),
    };
    Ok(json!({
        "driver": get(env, "SESSION_DRIVER", "database")?,
        "lifetime": get(env, "SESSION_LIFETIME", 120)?,
        "expire_on_close": get(env, "SESSION_EXPIRE_ON_CLOSE", false)?,
        "encrypt": get(env, "SESSION_ENCRYPT", false)?,
        "files": path(&base_path.join("storage/framework/sessions"))?,
        "connection": get(env, "SESSION_CONNECTION", Value::Null)?,
        "table": get(env, "SESSION_TABLE", "sessions")?,
        "store": get(env, "SESSION_STORE", Value::Null)?,
        "block": get(env, "SESSION_BLOCK", false)?,
        "block_store": get(env, "SESSION_BLOCK_STORE", Value::Null)?,
        "block_lock_seconds": get(env, "SESSION_BLOCK_LOCK_SECONDS", 10)?,
        "block_wait_seconds": get(env, "SESSION_BLOCK_WAIT_SECONDS", 10)?,
        "lottery": [2, 100],
        "cookie": get(env, "SESSION_COOKIE", cookie)?,
        "path": get(env, "SESSION_PATH", "/")?,
        "domain": get(env, "SESSION_DOMAIN", Value::Null)?,
        "secure": optional_bool(env, "SESSION_SECURE_COOKIE")?,
        "http_only": get(env, "SESSION_HTTP_ONLY", true)?,
        "same_site": same_site,
        "partitioned": get(env, "SESSION_PARTITIONED_COOKIE", false)?,
    }))
}
