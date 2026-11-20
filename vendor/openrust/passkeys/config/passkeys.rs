use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub fn load(env: &Environment, app: &Value) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | WebAuthn 依赖方与来源
    |--------------------------------------------------------------------------
    |
    | RP ID 从 APP_URL 的主机推导；来源包含端口，须精确匹配且不带路径。
    | 正式环境使用 HTTPS，开发可使用 HTTP localhost。
    |
    */
    let url = app.get("url").and_then(Value::as_str).unwrap_or_default();
    let host = url
        .parse::<openrust::http::Uri>()
        .ok()
        .and_then(|uri| uri.host().map(str::to_owned));
    /*
    |--------------------------------------------------------------------------
    | 用户句柄密钥与时限
    |--------------------------------------------------------------------------
    |
    | PASSKEYS_USER_HANDLE_SECRET 回退 APP_KEY。轮换应用密钥时保持句柄密钥
    | 稳定。timeout 以毫秒计，服务端和浏览器均使用此有效期。
    |
    */
    let secret = env
        .get_utf8("PASSKEYS_USER_HANDLE_SECRET")?
        .map(Value::from)
        .unwrap_or_else(|| app.get("key").cloned().unwrap_or_default());
    /*
    |--------------------------------------------------------------------------
    | 认证与路由
    |--------------------------------------------------------------------------
    |
    | 默认 web 守卫与中间件。管理操作要求密码确认，选项与提交使用每分钟
    | 六次的路由限流；登录后的默认跳转路径为根路径。
    |
    */
    Ok(
        json!({"relying_party_id":host,"allowed_origins":[url],"user_handle_secret":secret,"timeout":60000,"guard":"web","middleware":["web"],"management_middleware":["password.confirm"],"throttle":"throttle:6,1","redirect":"/"}),
    )
}
