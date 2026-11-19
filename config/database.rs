use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};
use openrust::support::Str;
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
    let database = get(env, "DB_DATABASE", database_name(app_name))?;
    let mysql = mysql_connection(env, "mysql", &url, &mask_bindings, &database)?;
    let mariadb = mysql_connection(env, "mariadb", &url, &mask_bindings, &database)?;

    /*
    |--------------------------------------------------------------------------
    | PostgreSQL 直连端点
    |--------------------------------------------------------------------------
    |
    | DB_DIRECT_* 用于配置直接访问数据库的参数，例如供迁移命令绕开事务池。
    |
    */
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
        /*
        |--------------------------------------------------------------------------
        | 默认数据库连接
        |--------------------------------------------------------------------------
        |
        | DB_CONNECTION 选择下方连接。DB_URL 可提供连接地址，DB_PREFIX 设置表名前缀。
        |
        */
        "default": get(env, "DB_CONNECTION", "sqlite")?,

        /*
        |--------------------------------------------------------------------------
        | 数据库连接
        |--------------------------------------------------------------------------
        |
        | SQLite 使用数据库文件，其余连接按驱动配置地址和账号。
        | DB_MASK_BINDINGS 可在数据库异常中隐藏绑定值。
        |
        */
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
                "database": database,
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
                "database": database,
                "username": get(env, "DB_USERNAME", "root")?,
                "password": get(env, "DB_PASSWORD", "")?,
                "ssl_ca": get(env, "DB_SSL_CA", "")?,
                "charset": get(env, "DB_CHARSET", "utf8")?,
                "prefix": get(env, "DB_PREFIX", "")?,
                "prefix_indexes": true,
                "mask_bindings_in_exception_messages": mask_bindings,
            },
        },

        /*
        |--------------------------------------------------------------------------
        | 迁移账本
        |--------------------------------------------------------------------------
        |
        | 记录已执行的迁移，并配置发布迁移文件时是否更新时间戳。
        |
        */
        "migrations": {"table": "migrations", "update_date_on_publish": true},

        /*
        |--------------------------------------------------------------------------
        | Redis 连接
        |--------------------------------------------------------------------------
        |
        | 默认连接使用 REDIS_DB。未设置 REDIS_CACHE_DB 时，缓存连接沿用 REDIS_DB。
        |
        */
        "redis": {
            "client": get(env, "REDIS_CLIENT", "redis")?,
            "options": {
                "cluster": get(env, "REDIS_CLUSTER", "redis")?,
                "prefix": get(env, "REDIS_PREFIX", format!("{}-database-", slug(app_name)))?,
                "persistent": get(env, "REDIS_PERSISTENT", false)?,
            },
            "default": redis_connection(env, "REDIS_DB", 0)?,
            "cache": redis_connection(env, "REDIS_CACHE_DB", get(env, "REDIS_DB", 0)?)?,
        },
    }))
}

/*
 * 为服务端数据库派生默认库名。显式 DB_DATABASE 不经过转换。
 * 复用 Str 的 ASCII 转写，将分隔符折叠为下划线，保留小写字母和数字。
 * 去掉开头的数字及下划线，空结果使用 app，最多保留 63 字节以兼容 PostgreSQL 默认上限。
 */
fn database_name(app_name: &str) -> String {
    let ascii = Str::ascii(app_name, "en");
    let name = ascii
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("_");
    let name =
        name.trim_start_matches(|character: char| character.is_ascii_digit() || character == '_');
    let name = name[..name.len().min(63)].trim_end_matches('_');
    if name.is_empty() {
        return "app".into();
    }
    name.to_owned()
}

/*
 * 组装 MySQL 和 MariaDB 共用的连接配置。
 *
 * MYSQL_ATTR_SSL_CA 为非空路径时，将它作为 ssl_ca 传给驱动。
 * 未设置、填空字符串或显式设为空值时，省略 ssl_ca，由驱动使用默认配置。
 */
fn mysql_connection(
    env: &Environment,
    driver: &str,
    url: &Value,
    mask_bindings: &Value,
    database: &Value,
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
        "database": database,
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
    database: impl Into<Value>,
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
