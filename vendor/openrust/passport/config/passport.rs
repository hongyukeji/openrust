use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | Passport 认证守卫、密钥与数据库
    |--------------------------------------------------------------------------
    |
    | 默认使用 web 守卫。密钥未设置时从应用 storage 目录读取。
    | PASSPORT_PRIVATE_KEY、PASSPORT_PUBLIC_KEY 可提供 PEM 或文件路径。
    | PASSPORT_CONNECTION 未设置时沿用默认数据库连接。
    |
    */
    Ok(json!({
        "guard": "web",
        "middleware": [],
        "private_key": environment(env, "PASSPORT_PRIVATE_KEY")?,
        "public_key": environment(env, "PASSPORT_PUBLIC_KEY")?,
        "connection": environment(env, "PASSPORT_CONNECTION")?,
    }))
}

fn environment(env: &Environment, key: &str) -> Result<Value, ConfigError> {
    Ok(match env.get_utf8(key)? {
        None => Value::Null,
        Some(value) => match value.to_ascii_lowercase().as_str() {
            "null" | "(null)" => Value::Null,
            "true" | "(true)" => Value::Bool(true),
            "false" | "(false)" => Value::Bool(false),
            "empty" | "(empty)" => Value::from(""),
            _ => Value::from(
                value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .unwrap_or(value),
            ),
        },
    })
}
