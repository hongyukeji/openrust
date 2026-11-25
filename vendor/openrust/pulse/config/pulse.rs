use openrust::config::{ConfigError, Environment, Value};
use openrust_pulse::recorders::Servers;
/*
|--------------------------------------------------------------------------
| Pulse 监控入口与开关
|--------------------------------------------------------------------------
| PULSE_ENABLED控制采集；PULSE_DOMAIN和PULSE_PATH配置面板，默认路径pulse。
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
    let number = |key: &str, default: &str| -> Result<f64, ConfigError> {
        env.get_utf8(key)?
            .unwrap_or(default)
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| ConfigError::InvalidType {
                key: key.into(),
                expected: "a finite number",
            })
    };
    let path = env.get_utf8("PULSE_PATH")?.unwrap_or("pulse");
    let mut recorders = serde_json::Map::new();
    for (name, variable) in [
        ("CacheInteractions", "CACHE_INTERACTIONS"),
        ("Exceptions", "EXCEPTIONS"),
        ("Queues", "QUEUES"),
        ("SlowJobs", "SLOW_JOBS"),
        ("SlowOutgoingRequests", "SLOW_OUTGOING_REQUESTS"),
        ("SlowQueries", "SLOW_QUERIES"),
        ("SlowRequests", "SLOW_REQUESTS"),
        ("UserJobs", "USER_JOBS"),
        ("UserRequests", "USER_REQUESTS"),
    ] {
        recorders.insert(name.into(),serde_json::json!({"enabled":boolean(&format!("PULSE_{variable}_ENABLED"),true)?,"sample_rate":number(&format!("PULSE_{variable}_SAMPLE_RATE"),"1")?,"ignore":[]}));
        if name.starts_with("Slow") {
            recorders[name]["threshold"] =
                number(&format!("PULSE_{variable}_THRESHOLD"), "1000")?.into();
        }
    }
    recorders["CacheInteractions"]["ignore"] = serde_json::json!([
        r"/(^laravel_vapor_job_attemp(t?)s:)/",
        r"/^.+@.+\|(?:(?:\d+\.\d+\.\d+\.\d+)|[0-9a-fA-F:]+)(?::timer)?$/",
        r"/^[a-zA-Z0-9]{40}$/",
        r"/^illuminate:/",
        r"/^openrust:/",
        r"/^laravel:pulse:/",
        r"/^laravel:reverb:/",
        r"/^nova/",
        r"/^telescope:/"
    ]);
    recorders["CacheInteractions"]["groups"] =
        serde_json::json!({"/^job-exceptions:.*/":"job-exceptions:*"});
    recorders["SlowOutgoingRequests"]["groups"] = serde_json::json!({});
    recorders["SlowQueries"]["ignore"] =
        serde_json::json!([r#"/(["`])pulse_[\w]+?\1/"#, r#"/(["`])telescope_[\w]+?\1/"#]);
    recorders["SlowQueries"]["location"] = boolean("PULSE_SLOW_QUERIES_LOCATION", true)?.into();
    recorders["SlowQueries"]["max_query_length"] = env
        .get_utf8("PULSE_SLOW_QUERIES_MAX_QUERY_LENGTH")?
        .map(|v| {
            v.parse::<u64>().map_err(|_| ConfigError::InvalidType {
                key: "PULSE_SLOW_QUERIES_MAX_QUERY_LENGTH".into(),
                expected: "an unsigned integer",
            })
        })
        .transpose()?
        .into();
    recorders["Exceptions"]["location"] = boolean("PULSE_EXCEPTIONS_LOCATION", true)?.into();
    for name in ["SlowRequests", "UserRequests"] {
        recorders[name]["ignore"] = serde_json::json!([format!("#^/{path}$#"), "#^/telescope#"]);
    }
    let server_name = env
        .get_utf8("PULSE_SERVER_NAME")?
        .map(str::to_owned)
        .unwrap_or_else(Servers::default_server_name);
    recorders.insert("Servers".into(),serde_json::json!({"server_name":server_name,"directories":env.get_utf8("PULSE_SERVER_DIRECTORIES")?.unwrap_or("/").split(':').collect::<Vec<_>>()}));
    /*
    |--------------------------------------------------------------------------
    | Pulse 数据存储与采集
    |--------------------------------------------------------------------------
    | 默认database存储、storage直接采集，可通过Redis流分离采集与持久化。
    | 包配置随Provider合并；缓存命中后沿用快照，不再读取环境变量。
    */
    Ok(
        serde_json::json!({"enabled":boolean("PULSE_ENABLED",true)?,"domain":env.get_utf8("PULSE_DOMAIN")?,"path":path,
            "storage":{"driver":env.get_utf8("PULSE_STORAGE_DRIVER")?.unwrap_or("database"),"trim":{"keep":env.get_utf8("PULSE_STORAGE_KEEP")?.unwrap_or("7 days")},"database":{"connection":env.get_utf8("PULSE_DB_CONNECTION")?,"chunk":1000}},
            "ingest":{"driver":env.get_utf8("PULSE_INGEST_DRIVER")?.unwrap_or("storage"),"buffer":env.get_utf8("PULSE_INGEST_BUFFER")?.unwrap_or("5000").parse::<u64>().map_err(|_|ConfigError::InvalidType{key:"PULSE_INGEST_BUFFER".into(),expected:"an unsigned integer"})?,"trim":{"lottery":[1,1000],"keep":env.get_utf8("PULSE_INGEST_KEEP")?.unwrap_or("7 days")},"redis":{"connection":env.get_utf8("PULSE_REDIS_CONNECTION")?,"chunk":1000}},
            "cache":env.get_utf8("PULSE_CACHE_DRIVER")?,"middleware":["web","openrust_pulse::http::middleware::Authorize"],"recorders":recorders
        }),
    )
}
