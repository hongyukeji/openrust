use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    // 对应 Laravel 的 User::class，配置缓存保留类名，类型工厂随应用编译。
    let auth_model = openrust::auth::model!(crate::app::models::User);
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 默认认证配置
        |--------------------------------------------------------------------------
        |
        | AUTH_GUARD 选择默认守卫，AUTH_PASSWORD_BROKER 选择密码重置服务。
        |
        */
        "defaults": {
            "guard": get(env, "AUTH_GUARD", "web")?,
            "passwords": get(env, "AUTH_PASSWORD_BROKER", "users")?,
        },

        /*
        |--------------------------------------------------------------------------
        | 守卫与用户提供器
        |--------------------------------------------------------------------------
        |
        | web 守卫通过会话认证，使用 users 提供器加载用户。AUTH_MODEL 指定用户模型。
        |
        */
        "guards": {
            "web": {"driver": "session", "provider": "users"},
        },
        "providers": {
            "users": {"driver": "eloquent", "model": get(env, "AUTH_MODEL", auth_model)?},
        },

        /*
        |--------------------------------------------------------------------------
        | 密码重置
        |--------------------------------------------------------------------------
        |
        | expire 以分钟设置令牌有效期，throttle 以秒设置重复申请间隔。
        |
        */
        "passwords": {
            "users": {
                "provider": "users",
                "table": get(env, "AUTH_PASSWORD_RESET_TOKEN_TABLE", "password_reset_tokens")?,
                "expire": 60,
                "throttle": 60,
            },
        },

        /*
        |--------------------------------------------------------------------------
        | 密码确认有效期
        |--------------------------------------------------------------------------
        |
        | AUTH_PASSWORD_TIMEOUT 以秒计，过期后敏感操作需重新确认密码。
        |
        */
        "password_timeout": get(env, "AUTH_PASSWORD_TIMEOUT", 10800)?,
    }))
}
