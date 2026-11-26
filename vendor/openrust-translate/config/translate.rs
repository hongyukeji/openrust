use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | 默认文本翻译服务与具名配置
    |--------------------------------------------------------------------------
    |
    | TRANSLATE_DRIVER 选择默认服务，应用可在 drivers 中声明其他具名配置。
    |
    */
    Ok(json!({"default": env.get_utf8("TRANSLATE_DRIVER")?, "drivers": {}}))
}
