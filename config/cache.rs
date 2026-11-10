use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, path, slug};

pub(super) fn load(
    base_path: &Path,
    env: &Environment,
    app_name: &str,
) -> Result<Value, ConfigError> {
    let prefix = format!("{}-cache-", slug(app_name));
    let directory = base_path.join("storage/framework/cache/data");
    let directory = path(&directory)?;
    Ok(json!({
        "default": get(env, "CACHE_STORE", "database")?,
        "stores": {
            "array": {"driver": "array", "serialize": false},
            "session": {"driver": "session", "key": get(env, "SESSION_CACHE_KEY", "_cache")?},
            "database": {
                "driver": "database",
                "connection": get(env, "DB_CACHE_CONNECTION", Value::Null)?,
                "table": get(env, "DB_CACHE_TABLE", "cache")?,
                "lock_connection": get(env, "DB_CACHE_LOCK_CONNECTION", Value::Null)?,
                "lock_table": get(env, "DB_CACHE_LOCK_TABLE", Value::Null)?,
            },
            "file": {"driver": "file", "path": directory, "lock_path": directory},
            "storage": {
                "driver": "storage",
                "disk": get(env, "CACHE_STORAGE_DISK", Value::Null)?,
                "path": get(env, "CACHE_STORAGE_PATH", "framework/cache/data")?,
            },
            "memcached": {
                "driver": "memcached",
                "persistent_id": get(env, "MEMCACHED_PERSISTENT_ID", Value::Null)?,
                "sasl": [get(env, "MEMCACHED_USERNAME", Value::Null)?, get(env, "MEMCACHED_PASSWORD", Value::Null)?],
                "options": {},
                "servers": [{
                    "host": get(env, "MEMCACHED_HOST", "127.0.0.1")?,
                    "port": get(env, "MEMCACHED_PORT", 11211)?,
                    "weight": 100,
                }],
            },
            "redis": {
                "driver": "redis",
                "connection": get(env, "REDIS_CACHE_CONNECTION", "cache")?,
                "lock_connection": get(env, "REDIS_CACHE_LOCK_CONNECTION", "default")?,
            },
            "dynamodb": {
                "driver": "dynamodb",
                "key": get(env, "AWS_ACCESS_KEY_ID", Value::Null)?,
                "secret": get(env, "AWS_SECRET_ACCESS_KEY", Value::Null)?,
                "region": get(env, "AWS_DEFAULT_REGION", "us-east-1")?,
                "table": get(env, "DYNAMODB_CACHE_TABLE", "cache")?,
                "endpoint": get(env, "DYNAMODB_ENDPOINT", Value::Null)?,
            },
            "failover": {"driver": "failover", "stores": ["database", "array"]},
        },
        "prefix": get(env, "CACHE_PREFIX", prefix)?,
    }))
}
