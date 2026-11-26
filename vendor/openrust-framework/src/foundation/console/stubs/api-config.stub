use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

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
    Ok(json!({"prefix": prefix.trim_matches('/')}))
}
