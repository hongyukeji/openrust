use super::env::get;
use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | OpenID Connect 身份提供方
    |--------------------------------------------------------------------------
    |
    | 显式启用 oidc 编译特性后，work 使用 OIDC_ISSUER、OIDC_CLIENT_ID、
    | OIDC_CLIENT_SECRET 与 OIDC_REDIRECT_URI。默认只访问 issuer 同源 HTTPS
    | 端点；跨源需在 allowed_origins 明确列出。未配置时不会添加登录路由。
    | OIDC_POST_LOGOUT_REDIRECT_URI 为已在提供方登记的固定退出回调，不读取请求目标。
    | OIDC_REPLAY_STORE 缺省使用默认缓存；多副本必须选择共享原子缓存。
    |
    */
    Ok(json!({"providers":{"work":{
        "issuer":get(env,"OIDC_ISSUER",Value::Null)?,
        "client_id":get(env,"OIDC_CLIENT_ID",Value::Null)?,
        "client_secret":get(env,"OIDC_CLIENT_SECRET",Value::Null)?,
        "redirect":get(env,"OIDC_REDIRECT_URI",Value::Null)?,
        "post_logout_redirect":get(env,"OIDC_POST_LOGOUT_REDIRECT_URI",Value::Null)?,
        "replay_store":get(env,"OIDC_REPLAY_STORE",Value::Null)?,
        "scopes":["openid","profile","email"],
        "algorithms":["RS256"],
        "attempt_ttl":600,
        "max_token_age":600,
        "timeout":10,
        "leeway":0,
        "userinfo":true,
        "allow_loopback_http":false
    }}}))
}
