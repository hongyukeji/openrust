use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;
use super::env::get;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 用户套件与认证功能
        |--------------------------------------------------------------------------
        |
        | 套件由 artisan init 显式安装。完整网页与仅 API 使用同一认证后端。
        | 注册和邮箱验证默认开启，2FA 与 Passkeys 由开发者按需启用。
        |
        */
        "profile": get(env, "USER_STARTER_PROFILE", "full")?,
        "registration": get(env, "USER_REGISTRATION", true)?,
        "email_verification": get(env, "USER_EMAIL_VERIFICATION", true)?,
        "two_factor": get(env, "USER_TWO_FACTOR", false)?,
        "passkeys": get(env, "USER_PASSKEYS", false)?,
        /*
        |--------------------------------------------------------------------------
        | 个人令牌与头像
        |--------------------------------------------------------------------------
        |
        | 令牌期限属于套件，不能修改原生 Sanctum 的默认契约。能力取自服务端
        | 白名单；普通客户端不能自行申请未授予的能力。头像由应用存储磁盘管理。
        |
        */
        "token_expiration_days": get(env, "USER_TOKEN_EXPIRATION_DAYS", 30)?,
        "token_abilities": ["account:read", "account:update", "tokens:manage"],
        "unverified_token_abilities": ["account:read","account:update","tokens:manage"],
        "avatar": {"disk":"public","max_kilobytes":2048,"max_dimension":4096},
        /*
        |--------------------------------------------------------------------------
        | 浏览器设备
        |--------------------------------------------------------------------------
        |
        | 默认使用database会话提供本人设备列表与撤销。file或Redis应用须显式
        | 设置USER_DEVICES=false关闭设备管理；全部凭据退出仍由版本策略保护。
        |
        */
        "devices": {"enabled": get(env, "USER_DEVICES", true)?},
        /*
        |--------------------------------------------------------------------------
        | 用户资源清理
        |--------------------------------------------------------------------------
        |
        | 头像替换与账户注销的清理任务使用同库数据库队列，和用户更改一起提交。
        | USER_CLEANUP_CONNECTION默认database，USER_CLEANUP_QUEUE默认user-resources。
        | 不改变应用默认队列；使用该连接和具名队列启动Worker。
        |
        */
        "cleanup": {
            "connection": get(env, "USER_CLEANUP_CONNECTION", "database")?,
            "queue": get(env, "USER_CLEANUP_QUEUE", "user-resources")?,
        },
        /*
        |--------------------------------------------------------------------------
        | 认证邮件队列
        |--------------------------------------------------------------------------
        |
        | 默认null保持原生同步发送。USER_MAIL_QUEUE设置队列名后，重置与验证
        | 通知加密入队；连接未配置时使用queue.default，Worker读取自己的邮件凭据。
        | 重试、退避和超时使用queue:work选项，需启动对应持久化队列的Worker。
        |
        */
        "mail": {
            "queue": get(env, "USER_MAIL_QUEUE", Value::Null)?,
            "connection": get(env, "USER_MAIL_QUEUE_CONNECTION", Value::Null)?,
        }
    }))
}
