use openrust::config::{ConfigError, Environment, Value};
/*
|--------------------------------------------------------------------------
| Pennant 特性存储
|--------------------------------------------------------------------------
| PENNANT_STORE 默认 database，也可使用 array；数据库表由包迁移创建。
| 每个存储独立定义特性和取值，作用域缓存随 HTTP、命令和队列任务隔离。
*/
pub fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(serde_json::json!({
        "default": env.get_utf8("PENNANT_STORE")?.unwrap_or("database"),
        "stores": {
            "array": { "driver":"array" },
            "database": { "driver":"database", "connection":null, "table":"features" }
        }
    }))
}
