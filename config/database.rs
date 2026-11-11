use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, optional_integer, path, slug};

pub(super) fn load(
    base_path: &Path,
    env: &Environment,
    app_name: &str,
) -> Result<Value, ConfigError> {
    let sqlite = base_path.join("database/database.sqlite");
    let url = get(env, "DB_URL", Value::Null)?;
    let mask_bindings = get(env, "DB_MASK_BINDINGS", false)?;
    let mysql = mysql_connection(env, "mysql", &url, &mask_bindings)?;
    let mariadb = mysql_connection(env, "mariadb", &url, &mask_bindings)?;
    let direct = [
        ("host", get(env, "DB_DIRECT_HOST", Value::Null)?),
        ("port", json!(optional_integer(env, "DB_DIRECT_PORT")?)),
        ("username", get(env, "DB_DIRECT_USERNAME", Value::Null)?),
        ("password", get(env, "DB_DIRECT_PASSWORD", Value::Null)?),
        ("sslmode", get(env, "DB_DIRECT_SSLMODE", Value::Null)?),
    ]
    .into_iter()
    .filter(|(_, value)| !value.is_null() && value.as_str() != Some(""))
    .map(|(key, value)| (key.to_owned(), value))
    .collect::<serde_json::Map<_, _>>();

    Ok(json!({
        "default": get(env, "DB_CONNECTION", "sqlite")?,
        "connections": {
            "sqlite": {
                "driver": "sqlite",
                "url": url,
                "database": get(env, "DB_DATABASE", path(&sqlite)?)?,
                "prefix": get(env, "DB_PREFIX", "")?,
                "prefix_indexes": null,
                "mask_bindings_in_exception_messages": mask_bindings,
                "foreign_key_constraints": get(env, "DB_FOREIGN_KEYS", true)?,
                "busy_timeout": null,
                "journal_mode": null,
                "synchronous": null,
                "transaction_mode": "DEFERRED",
                "pragmas": {},
            },
            "mysql": mysql,
            "mariadb": mariadb,
            "pgsql": {
                "driver": "pgsql",
                "url": url,
                "host": get(env, "DB_HOST", "127.0.0.1")?,
                "port": get(env, "DB_PORT", 5432)?,
                "database": get(env, "DB_DATABASE", "openrust")?,
                "username": get(env, "DB_USERNAME", "root")?,
                "password": get(env, "DB_PASSWORD", "")?,
                "charset": get(env, "DB_CHARSET", "utf8")?,
                "prefix": get(env, "DB_PREFIX", "")?,
                "prefix_indexes": true,
                "mask_bindings_in_exception_messages": mask_bindings,
                "search_path": "public",
                "sslmode": "prefer",
                "pooled": get(env, "DB_POOLED", false)?,
                "direct": direct,
            },
            "sqlsrv": {
                "driver": "sqlsrv",
                "url": url,
                "host": get(env, "DB_HOST", "localhost")?,
                "port": get(env, "DB_PORT", 1433)?,
                "database": get(env, "DB_DATABASE", "openrust")?,
                "username": get(env, "DB_USERNAME", "root")?,
                "password": get(env, "DB_PASSWORD", "")?,
                "charset": get(env, "DB_CHARSET", "utf8")?,
                "prefix": get(env, "DB_PREFIX", "")?,
                "prefix_indexes": true,
                "mask_bindings_in_exception_messages": mask_bindings,
            },
        },
        "migrations": {"table": "migrations", "update_date_on_publish": true},
        "redis": {
            "client": get(env, "REDIS_CLIENT", "redis")?,
            "options": {
                "cluster": get(env, "REDIS_CLUSTER", "redis")?,
                "prefix": get(env, "REDIS_PREFIX", format!("{}-database-", slug(app_name)))?,
                "persistent": get(env, "REDIS_PERSISTENT", false)?,
            },
            "default": redis_connection(env, "REDIS_DB", 0)?,
            "cache": redis_connection(env, "REDIS_CACHE_DB", 1)?,
        },
    }))
}

fn mysql_connection(
    env: &Environment,
    driver: &str,
    url: &Value,
    mask_bindings: &Value,
) -> Result<Value, ConfigError> {
    let ssl_ca = get(env, "MYSQL_ATTR_SSL_CA", Value::Null)?;
    let options = if ssl_ca.is_null() || ssl_ca.as_str() == Some("") {
        json!({})
    } else {
        json!({"ssl_ca": ssl_ca})
    };
    Ok(json!({
        "driver": driver,
        "url": url,
        "host": get(env, "DB_HOST", "127.0.0.1")?,
        "port": get(env, "DB_PORT", 3306)?,
        "database": get(env, "DB_DATABASE", "openrust")?,
        "username": get(env, "DB_USERNAME", "root")?,
        "password": get(env, "DB_PASSWORD", "")?,
        "unix_socket": get(env, "DB_SOCKET", "")?,
        "charset": get(env, "DB_CHARSET", "utf8mb4")?,
        "collation": get(env, "DB_COLLATION", "utf8mb4_unicode_ci")?,
        "prefix": get(env, "DB_PREFIX", "")?,
        "prefix_indexes": true,
        "mask_bindings_in_exception_messages": mask_bindings,
        "strict": true,
        "engine": null,
        "options": options,
    }))
}

fn redis_connection(
    env: &Environment,
    database_key: &str,
    database: u64,
) -> Result<Value, ConfigError> {
    Ok(json!({
        "url": get(env, "REDIS_URL", Value::Null)?,
        "host": get(env, "REDIS_HOST", "127.0.0.1")?,
        "username": get(env, "REDIS_USERNAME", Value::Null)?,
        "password": get(env, "REDIS_PASSWORD", Value::Null)?,
        "port": get(env, "REDIS_PORT", 6379)?,
        "database": get(env, database_key, database)?,
        "max_retries": get(env, "REDIS_MAX_RETRIES", 3)?,
        "backoff_algorithm": get(env, "REDIS_BACKOFF_ALGORITHM", "decorrelated_jitter")?,
        "backoff_base": get(env, "REDIS_BACKOFF_BASE", 100)?,
        "backoff_cap": get(env, "REDIS_BACKOFF_CAP", 1000)?,
        "command_retries": get(env, "REDIS_COMMAND_RETRIES", 0)?,
    }))
}
