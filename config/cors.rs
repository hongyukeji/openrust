use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::list;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    let paths = env
        .get_utf8("CORS_PATHS")?
        .map_or_else(|| vec!["api/*", "sanctum/csrf-cookie"], list);

    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 适用路径
        |--------------------------------------------------------------------------
        |
        | CORS_PATHS 接受逗号分隔的路径模式，用于限定跨域规则的适用范围。
        |
        */
        "paths": paths,

        /*
        |--------------------------------------------------------------------------
        | 跨域访问规则
        |--------------------------------------------------------------------------
        |
        | 配置允许的来源、方法与请求头。启用凭据时，显式列出可信来源。
        |
        */
        "allowed_methods": ["*"],
        "allowed_origins": ["*"],
        "allowed_origins_patterns": [],
        "allowed_headers": ["*"],
        "exposed_headers": [],
        "max_age": 0,
        "supports_credentials": false,
    }))
}
