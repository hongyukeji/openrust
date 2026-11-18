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
        /*
        |--------------------------------------------------------------------------
        | 会话存储与生命周期
        |--------------------------------------------------------------------------
        |
        | SESSION_DRIVER 选择存储后端，SESSION_LIFETIME 以分钟设置空闲有效期。
        | 可以设置浏览器关闭时过期，也可加密会话数据。
        |
        */
        "driver": get(env, "SESSION_DRIVER", "database")?,
        "lifetime": get(env, "SESSION_LIFETIME", 120)?,
        "expire_on_close": get(env, "SESSION_EXPIRE_ON_CLOSE", false)?,
        "encrypt": get(env, "SESSION_ENCRYPT", false)?,

        /*
        |--------------------------------------------------------------------------
        | 存储位置与连接
        |--------------------------------------------------------------------------
        |
        | 配置文件目录、数据库连接与会话表，或指定缓存驱动使用的仓储。
        |
        */
        "files": path(&env.storage_path(base_path, "framework/sessions"))?,
        "connection": get(env, "SESSION_CONNECTION", Value::Null)?,
        "table": get(env, "SESSION_TABLE", "sessions")?,
        "store": get(env, "SESSION_STORE", Value::Null)?,

        /*
        |--------------------------------------------------------------------------
        | 会话请求锁
        |--------------------------------------------------------------------------
        |
        | 使用缓存锁串行处理同一会话的请求。锁持有与等待时长均以秒计。
        |
        */
        "block": get(env, "SESSION_BLOCK", false)?,
        "block_store": get(env, "SESSION_BLOCK_STORE", Value::Null)?,
        "block_lock_seconds": get(env, "SESSION_BLOCK_LOCK_SECONDS", 10)?,
        "block_wait_seconds": get(env, "SESSION_BLOCK_WAIT_SECONDS", 10)?,

        /*
        |--------------------------------------------------------------------------
        | 过期会话清理
        |--------------------------------------------------------------------------
        |
        | 设置支持概率清理的驱动所使用的触发比例。
        |
        */
        "lottery": [2, 100],

        /*
        |--------------------------------------------------------------------------
        | 会话 Cookie
        |--------------------------------------------------------------------------
        |
        | 配置会话 Cookie 的名称、路径、域和浏览器安全属性。
        |
        */
        "cookie": get(env, "SESSION_COOKIE", cookie)?,
        "path": get(env, "SESSION_PATH", "/")?,
        "domain": get(env, "SESSION_DOMAIN", Value::Null)?,
        "secure": optional_bool(env, "SESSION_SECURE_COOKIE")?,
        "http_only": get(env, "SESSION_HTTP_ONLY", true)?,
        "same_site": same_site,
        "partitioned": get(env, "SESSION_PARTITIONED_COOKIE", false)?,
    }))
}
