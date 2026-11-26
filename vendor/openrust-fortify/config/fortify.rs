use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::config::Repository,
    http::Uri,
};
use serde_json::json;

pub(super) fn load(config: &dyn Repository, env: &Environment) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | Fortify 认证与功能
    |--------------------------------------------------------------------------
    |
    | 使用 web 守卫、Session 中间件与 email 用户名。功能默认值沿用固定
    | Fortify 源码；Passkeys 的实际接口由独立扩展单元提供。
    | PASSKEYS_USER_HANDLE_SECRET 未设置时沿用 app.key。
    |
    */
    let url = config.get("app.url").unwrap_or(Value::Null);
    let host = url
        .as_str()
        .and_then(|url| url.parse::<Uri>().ok())
        .and_then(|uri| uri.host().map(str::to_owned));
    let secret = env
        .get_utf8("PASSKEYS_USER_HANDLE_SECRET")?
        .map(Value::from)
        .unwrap_or_else(|| config.get("app.key").unwrap_or(Value::Null));
    Ok(json!({
        "guard":"web","middleware":["web"],"auth_middleware":"auth","passwords":"users","username":"email","email":"email","views":true,"home":"/home","prefix":"","domain":null,"lowercase_usernames":false,
        "limiters":{"login":null,"passkeys":null},
        "paths":{
            "login":null,"logout":null,"password":{"request":null,"reset":null,"email":null,"update":null,"confirm":null,"confirmation":null},"register":null,"verification":{"notice":null,"verify":null,"send":null},
            "user-profile-information":{"update":null},"user-password":{"update":null},
            "two-factor":{"login":null,"enable":null,"confirm":null,"disable":null,"qr-code":null,"secret-key":null,"recovery-codes":null},
            "passkey":{"login-options":null,"login":null,"confirm-options":null,"confirm":null,"registration-options":null,"store":null,"destroy":null}
        },
        "redirects":{"login":null,"logout":null,"password-confirmation":null,"register":null,"email-verification":null,"password-reset":null},
        "passkeys":{"relying_party_id":host,"allowed_origins":[url],"user_handle_secret":secret,"timeout":60000},
        "features":["registration","reset-passwords","email-verification","update-profile-information","update-passwords","two-factor-authentication","passkeys"]
    }))
}
