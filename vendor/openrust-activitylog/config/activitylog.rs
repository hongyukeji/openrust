use openrust::config::{ConfigError, Environment, Value};
/*
|--------------------------------------------------------------------------
| 操作审计
|--------------------------------------------------------------------------
| 包启用后登记迁移；显式调用时才访问数据库。默认使用应用连接、365天保留期，
| activitylog:clean 按保留期及可选日志名清理旧记录。显式传入的属性与变化默认保留；脱敏策略须显式开启。
*/
pub fn load(env: &Environment) -> Result<Value, ConfigError> {
    let boolean = |key: &str, default: bool| -> Result<bool, ConfigError> {
        match env.get_utf8(key)?.map(str::to_ascii_lowercase).as_deref() {
            None => Ok(default),
            Some("true" | "(true)" | "1") => Ok(true),
            Some("false" | "(false)" | "0") => Ok(false),
            _ => Err(ConfigError::InvalidType {
                key: key.into(),
                expected: "a boolean",
            }),
        }
    };
    Ok(serde_json::json!({
        "enabled": boolean("ACTIVITYLOG_ENABLED", true)?,
        "default_log_name": "default",
        "default_auth_driver": null,
        "buffer": {"enabled": boolean("ACTIVITYLOG_BUFFER_ENABLED", false)?},
        "include_soft_deleted_subjects": false,
        "activity_model": "openrust_activitylog::models::Activity",
        "actions": {
            "log_activity": "openrust_activitylog::actions::LogActivityAction",
            "clean_log": "openrust_activitylog::actions::CleanActivityLogAction"
        },
        "default_except_attributes": [],
        "redaction": {"enabled": false},
        "connection": null,
        "clean_after_days": 365,
        "properties": [],
        "attributes": [],
        "except": []
    }))
}
