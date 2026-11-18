use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | 默认并发驱动
    |--------------------------------------------------------------------------
    |
    | CONCURRENCY_DRIVER 选择 Concurrency 默认使用的驱动。
    |
    */
    Ok(json!({"default": get(env, "CONCURRENCY_DRIVER", "process")?}))
}
