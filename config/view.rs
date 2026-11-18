use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, path};

pub(super) fn load(base_path: &Path, env: &Environment) -> Result<Value, ConfigError> {
    let compiled = base_path.join("storage/framework/views");
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 视图目录与编译缓存
        |--------------------------------------------------------------------------
        |
        | paths 指定视图搜索目录。VIEW_COMPILED_PATH 可以覆盖视图编译缓存目录。
        |
        */
        "paths": [path(&base_path.join("resources/views"))?],
        "compiled": get(env, "VIEW_COMPILED_PATH", path(&compiled)?)?,
    }))
}
