use openrust::{
    config::{ConfigError, Environment, Value},
    http::Uri,
};
use serde_json::json;

pub(super) fn load(env: &Environment, app: &Value) -> Result<Value, ConfigError> {
    /*
    |--------------------------------------------------------------------------
    | Fortify 认证与路由
    |--------------------------------------------------------------------------
    |
    | 使用 web 守卫、users 密码代理和 email 用户名。安装配置将用户名转为
    | 小写，并使用应用 Provider 注册的命名限流。views=false 用于无界面应用。
    |
    */
    let url = app.get("url").cloned().unwrap_or(Value::Null);
    let host = url
        .as_str()
        .and_then(|url| url.parse::<Uri>().ok())
        .and_then(|uri| uri.host().map(str::to_owned));
    let secret = env
        .get_utf8("PASSKEYS_USER_HANDLE_SECRET")?
        .map(Value::from)
        .unwrap_or_else(|| app.get("key").cloned().unwrap_or(Value::Null));
    Ok(json!({
        "guard":"web","passwords":"users","username":"email","email":"email",
        "lowercase_usernames":true,"home":"/home","prefix":"","domain":null,
        "middleware":["web"],"views":true,
        "limiters":{"login":"login","two-factor":"two-factor","passkeys":"passkeys"},
        "passkeys":{"relying_party_id":host,"allowed_origins":[url],"user_handle_secret":secret,"timeout":60000},
        /*
        |--------------------------------------------------------------------------
        | 功能开关
        |--------------------------------------------------------------------------
        |
        | 沿用 Fortify 1.43.0 安装模板，email-verification 默认未启用。
        | 根 User 实现了邮件验证能力，使用它时须启用邮件验证路由。
        | Passkeys 尚待 API-F02 验收；使用 API-F01 时须移除 passkeys。
        |
        */
        "features":["registration","reset-passwords","update-profile-information","update-passwords","two-factor-authentication","passkeys"]
    }))
}

pub(super) fn options() -> Value {
    /*
    |--------------------------------------------------------------------------
    | 功能确认选项
    |--------------------------------------------------------------------------
    |
    | Rust 将原版 Features 方法的参数显式存入 fortify-options 配置组。
    | 双因素注册需要确认，并在管理凭据前要求近期密码确认。
    |
    */
    json!({"two-factor-authentication":{"confirm":true,"confirmPassword":true},"passkeys":{"confirmPassword":true}})
}
