use openrust::config::{ConfigError, Environment, Value};
/*
|--------------------------------------------------------------------------
| Horizon 队列监督
|--------------------------------------------------------------------------
| 使用应用已有的具名 Redis 与队列连接，配置和模板由扩展包提供。
| 启用包不会启动进程或注册 HTTP 路由；显式运行 artisan horizon。
| prefix 应按应用和环境隔离；面板授权后可查看任务载荷和异常文本。
| trim 为记录保留分钟数；metrics.trim_snapshots 为指标快照条数。
*/
pub fn load(_: &Environment) -> Result<Value, ConfigError> {
    Ok(serde_json::json!({
        "use":"default", "prefix":"horizon:", "retention":3600,"fast_termination":false,"memory_limit":64,
        "trim":{"recent":60,"pending":60,"completed":60,"recent_failed":10080,"failed":10080,"monitored":10080},
        "metrics":{"snapshot_lock":300,"trim_snapshots":{"job":24,"queue":24}},
        "defaults":{"supervisor-1":{
            "connection":"redis", "queue":["default"], "balance":"auto",
            "autoScalingStrategy":"time", "minProcesses":1, "maxProcesses":1,
            "balanceMaxShift":1, "balanceCooldown":3, "tries":1,
            "timeout":60, "sleep":3, "memory":128, "maxJobs":0, "maxTime":0
        }}, "environments":{
            "production":{"supervisor-1":{"maxProcesses":10,"balanceMaxShift":1,"balanceCooldown":3}},
            "local":{"supervisor-1":{"maxProcesses":3}}
        }
    }))
}
