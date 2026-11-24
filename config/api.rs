use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | API 路由前缀
    |--------------------------------------------------------------------------
    |
    | 通过 API_PREFIX 设置。默认路由前缀为 /api。
    | 多级前缀可写为 api/admin，首尾斜杠会自动去除。
    |
    */
    let prefix = env.get_utf8("API_PREFIX")?.unwrap_or("/api");
    Ok(json!({
        "prefix": prefix.trim_matches('/'),

        /*
        |--------------------------------------------------------------------------
        | 业务 API 响应
        |--------------------------------------------------------------------------
        |
        | 通过 API_RESPONSE_ENVELOPE 设置，默认启用。
        | 为 API 或协商 JSON 的异常及普通 Resource 启用统一业务封装。
        | 消息读取当前请求语言。设为 false 保留原生异常和 Resource 格式；
        | 显式返回 ApiResponse 的路由始终使用业务封装。
        |
        */
        "response_envelope": get(env, "API_RESPONSE_ENVELOPE", true)?,
    }))
}
