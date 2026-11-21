use openrust::config::{ConfigError, Environment, Value};
/*
|--------------------------------------------------------------------------
| Telescope 诊断记录
|--------------------------------------------------------------------------
| TELESCOPE_ENABLED 控制采集。包默认不访问数据库，实际记录使用应用数据库连接。
| 诊断界面的路径默认 telescope；访问授权由包原生门禁负责。
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
    let mut watchers = serde_json::Map::new();
    for (name, variable) in [
        ("BatchWatcher", "BATCH"),
        ("CacheWatcher", "CACHE"),
        ("ClientRequestWatcher", "CLIENT_REQUEST"),
        ("CommandWatcher", "COMMAND"),
        ("DumpWatcher", "DUMP"),
        ("EventWatcher", "EVENT"),
        ("ExceptionWatcher", "EXCEPTION"),
        ("GateWatcher", "GATE"),
        ("JobWatcher", "JOB"),
        ("LogWatcher", "LOG"),
        ("MailWatcher", "MAIL"),
        ("ModelWatcher", "MODEL"),
        ("NotificationWatcher", "NOTIFICATION"),
        ("QueryWatcher", "QUERY"),
        ("RedisWatcher", "REDIS"),
        ("RequestWatcher", "REQUEST"),
        ("ScheduleWatcher", "SCHEDULE"),
        ("ViewWatcher", "VIEW"),
    ] {
        watchers.insert(
            name.into(),
            serde_json::json!({"enabled":boolean(&format!("TELESCOPE_{variable}_WATCHER"),true)?}),
        );
    }
    watchers["CacheWatcher"]["hidden"] = serde_json::json!([]);
    watchers["CacheWatcher"]["ignore"] = serde_json::json!([]);
    watchers["ClientRequestWatcher"]["ignore_hosts"] = serde_json::json!([]);
    watchers["CommandWatcher"]["ignore"] = serde_json::json!([]);
    watchers["DumpWatcher"]["always"] = boolean("TELESCOPE_DUMP_WATCHER_ALWAYS", false)?.into();
    watchers["EventWatcher"]["ignore"] = serde_json::json!([]);
    watchers["GateWatcher"]["ignore_abilities"] = serde_json::json!([]);
    for watcher in ["GateWatcher", "QueryWatcher"] {
        watchers[watcher]["ignore_packages"] = true.into();
        watchers[watcher]["ignore_paths"] = serde_json::json!([]);
    }
    watchers["LogWatcher"]["level"] = "error".into();
    watchers["ModelWatcher"]["events"] = serde_json::json!(["eloquent.*"]);
    watchers["ModelWatcher"]["hydrations"] = true.into();
    watchers["QueryWatcher"]["slow"] = 100.into();
    let size = env
        .get_utf8("TELESCOPE_RESPONSE_SIZE_LIMIT")?
        .unwrap_or("64")
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidType {
            key: "TELESCOPE_RESPONSE_SIZE_LIMIT".into(),
            expected: "an unsigned integer",
        })?;
    watchers["RequestWatcher"]["size_limit"] = size.into();
    watchers["RequestWatcher"]["ignore_http_methods"] = serde_json::json!([]);
    watchers["RequestWatcher"]["ignore_status_codes"] = serde_json::json!([]);
    let delay = env
        .get_utf8("TELESCOPE_QUEUE_DELAY")?
        .unwrap_or("10")
        .parse::<u64>()
        .map_err(|_| ConfigError::InvalidType {
            key: "TELESCOPE_QUEUE_DELAY".into(),
            expected: "an unsigned integer",
        })?;
    Ok(serde_json::json!({
        "enabled":boolean("TELESCOPE_ENABLED",true)?, "domain":env.get_utf8("TELESCOPE_DOMAIN")?, "path":env.get_utf8("TELESCOPE_PATH")?.unwrap_or("telescope"), "driver":env.get_utf8("TELESCOPE_DRIVER")?.unwrap_or("database"),
        "storage":{"database":{"connection":env.get_utf8("DB_CONNECTION")?,"chunk":1000}},
        "queue":{"connection":env.get_utf8("TELESCOPE_QUEUE_CONNECTION")?,"queue":env.get_utf8("TELESCOPE_QUEUE")?,"delay":delay},
        "middleware":["web","openrust_telescope::http::middleware::Authorize"],
        "only_paths":[],"ignore_paths":["livewire*","nova-api*","pulse*","_boost*",".well-known*"],"ignore_commands":[], "watchers":watchers
    }))
}
