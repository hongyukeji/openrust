use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, list, optional_integer, path};

pub(super) fn load(
    base_path: &Path,
    env: &Environment,
    app_name: &str,
) -> Result<Value, ConfigError> {
    let log_path = env.storage_path(base_path, "logs/openrust.log");
    let path = path(&log_path)?;
    let level = get(env, "LOG_LEVEL", "debug")?;
    let stack = env.get_utf8("LOG_STACK")?.unwrap_or("single");

    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 默认日志渠道
        |--------------------------------------------------------------------------
        |
        | LOG_CHANNEL 选择默认渠道，LOG_STACK 以逗号分隔指定堆叠渠道中的各输出。
        |
        */
        "default": get(env, "LOG_CHANNEL", "stack")?,

        /*
        |--------------------------------------------------------------------------
        | 弃用日志
        |--------------------------------------------------------------------------
        |
        | 设置弃用提示的日志渠道，以及是否附带调用栈。
        |
        */
        "deprecations": {
            "channel": get(env, "LOG_DEPRECATIONS_CHANNEL", "null")?,
            "trace": get(env, "LOG_DEPRECATIONS_TRACE", false)?,
        },

        /*
        |--------------------------------------------------------------------------
        | 日志输出渠道
        |--------------------------------------------------------------------------
        |
        | 渠道可写入文件、标准错误或外部服务。LOG_LEVEL 控制读取该变量的渠道的最低级别。
        |
        */
        "channels": {
            "stack": {"driver": "stack", "channels": list(stack), "ignore_exceptions": false},
            "single": {"driver": "single", "path": path, "level": level, "replace_placeholders": true},
            "daily": {
                "driver": "daily",
                "path": path,
                "level": level,
                "max_files": get(env, "LOG_DAILY_DAYS", 14)?,
                "replace_placeholders": true,
            },
            "monthly": {"driver": "monthly", "path": path, "level": level, "max_files": 3, "replace_placeholders": true},
            "slack": {
                "driver": "slack",
                "url": get(env, "LOG_SLACK_WEBHOOK_URL", Value::Null)?,
                "username": get(env, "LOG_SLACK_USERNAME", format!("{app_name} Log"))?,
                "emoji": get(env, "LOG_SLACK_EMOJI", ":boom:")?,
                "level": get(env, "LOG_LEVEL", "critical")?,
                "replace_placeholders": true,
            },
            "papertrail": {
                "driver": "papertrail",
                "level": level,
                "host": get(env, "PAPERTRAIL_URL", Value::Null)?,
                "port": optional_integer(env, "PAPERTRAIL_PORT")?,
                "replace_placeholders": true,
            },
            "stderr": {
                "driver": "stderr",
                "level": level,
                "formatter": get(env, "LOG_STDERR_FORMATTER", Value::Null)?,
                "replace_placeholders": true,
            },
            "syslog": {
                "driver": "syslog",
                "level": level,
                "facility": get(env, "LOG_SYSLOG_FACILITY", "user")?,
                "replace_placeholders": true,
            },
            "errorlog": {"driver": "errorlog", "level": level, "replace_placeholders": true},
            "null": {"driver": "null"},
            "emergency": {"path": path},
        },
    }))
}
