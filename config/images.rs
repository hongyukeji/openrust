use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | 图像处理驱动
    |--------------------------------------------------------------------------
    |
    | IMAGE_DRIVER 选择默认图像处理驱动。
    |
    */
    Ok(json!({"default": get(env, "IMAGE_DRIVER", "image")?}))
}
