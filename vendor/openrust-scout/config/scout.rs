use openrust::config::{ConfigError, Environment};
use serde_json::{Value, json};
pub fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | 默认搜索引擎与索引前缀
    |--------------------------------------------------------------------------
    | SCOUT_DRIVER 默认 collection；SCOUT_PREFIX 默认空字符串。
    */
    let boolean = |key: &str, default: bool| -> Result<bool, ConfigError> {
        match env.get_utf8(key)?.map(str::to_ascii_lowercase).as_deref() {
            None => Ok(default),
            Some("true" | "(true)" | "1") => Ok(true),
            Some("false" | "(false)" | "0") => Ok(false),
            _ => Err(ConfigError::InvalidType {
                key: key.into(),
                expected: "a boolean",
            }),
        }
    };
    let driver = env.get_utf8("SCOUT_DRIVER")?.unwrap_or("collection");
    let prefix = env.get_utf8("SCOUT_PREFIX")?.unwrap_or("");
    /*
    |--------------------------------------------------------------------------
    | 模型同步、事务与软删除
    |--------------------------------------------------------------------------
    | SCOUT_QUEUE、SCOUT_AFTER_COMMIT 和 SCOUT_SOFT_DELETE 默认关闭。
    | 队列连接与队列名称可在编译型配置的 queue 对象中指定。
    */
    Ok(
        json!({"driver":driver,"prefix":prefix,"queue":boolean("SCOUT_QUEUE",false)?,
        "after_commit":boolean("SCOUT_AFTER_COMMIT",false)?,
        "soft_delete":boolean("SCOUT_SOFT_DELETE",false)?,
        "chunk":{"searchable":500,"unsearchable":500},
        "identify":boolean("SCOUT_IDENTIFY",false)?}),
    )
}
