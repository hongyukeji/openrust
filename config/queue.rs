use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, optional_integer};

pub(super) fn load(env: &Environment, database: &Value) -> Result<Value, ConfigError> {
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 默认队列连接
        |--------------------------------------------------------------------------
        |
        | QUEUE_CONNECTION 选择队列后端。sync 在当前进程执行，数据库和 Redis 等后端由 Worker 消费。
        |
        */
        "default": get(env, "QUEUE_CONNECTION", "database")?,

        /*
        |--------------------------------------------------------------------------
        | Worker 运行限制
        |--------------------------------------------------------------------------
        |
        | 配置内存限制、超时退出及连接丢失时的处理。
        |
        */
        "memory_limit": optional_integer(env, "QUEUE_WORKER_MEMORY_LIMIT")?,
        "kill_on_timeout": get(env, "QUEUE_WORKER_KILL_ON_TIMEOUT", true)?,
        "timed_out_exit_code": optional_integer(env, "QUEUE_WORKER_TIMED_OUT_EXIT_CODE")?,
        "stop_on_lost_connection": get(env, "QUEUE_WORKER_STOP_ON_LOST_CONNECTION", true)?,

        /*
        |--------------------------------------------------------------------------
        | 队列连接与重投递
        |--------------------------------------------------------------------------
        |
        | 配置队列名称、重投递时间和事务提交后派发。
        | Worker 超时应短于任务可被重新领取的时间。
        |
        */
        "connections": {
            "sync": {"driver": "sync"},
            "database": {
                "driver": "database",
                "connection": get(env, "DB_QUEUE_CONNECTION", Value::Null)?,
                "table": get(env, "DB_QUEUE_TABLE", "jobs")?,
                "queue": get(env, "DB_QUEUE", "default")?,
                "retry_after": get(env, "DB_QUEUE_RETRY_AFTER", 90)?,
                "after_commit": false,
            },
            "beanstalkd": {
                "driver": "beanstalkd",
                "host": get(env, "BEANSTALKD_QUEUE_HOST", "localhost")?,
                "port": get(env, "BEANSTALKD_QUEUE_PORT", 11300)?,
                "queue": get(env, "BEANSTALKD_QUEUE", "default")?,
                "retry_after": get(env, "BEANSTALKD_QUEUE_RETRY_AFTER", 90)?,
                "block_for": 0,
                "after_commit": false,
            },
            "sqs": {
                "driver": "sqs",
                "key": get(env, "AWS_ACCESS_KEY_ID", Value::Null)?,
                "secret": get(env, "AWS_SECRET_ACCESS_KEY", Value::Null)?,
                "token": get(env, "AWS_SESSION_TOKEN", Value::Null)?,
                "endpoint": get(env, "SQS_ENDPOINT", Value::Null)?,
                "prefix": get(env, "SQS_PREFIX", "https://sqs.us-east-1.amazonaws.com/your-account-id")?,
                "queue": get(env, "SQS_QUEUE", "default")?,
                "suffix": get(env, "SQS_SUFFIX", Value::Null)?,
                "region": get(env, "AWS_DEFAULT_REGION", "us-east-1")?,
                "after_commit": false,
                "overflow": {
                    "enabled": get(env, "SQS_OVERFLOW_ENABLED", false)?,
                    "store": get(env, "SQS_OVERFLOW_STORE", Value::Null)?,
                    "always": get(env, "SQS_OVERFLOW_ALWAYS", false)?,
                    "delete_after_processing": get(env, "SQS_OVERFLOW_DELETE_AFTER_PROCESSING", true)?,
                    "flush_on_clear": get(env, "SQS_OVERFLOW_FLUSH_ON_CLEAR", false)?,
                    "ttl_seconds": optional_integer(env, "SQS_OVERFLOW_TTL_SECONDS")?,
                },
            },
            "redis": {
                "driver": "redis",
                "connection": get(env, "REDIS_QUEUE_CONNECTION", "default")?,
                "queue": get(env, "REDIS_QUEUE", "default")?,
                "retry_after": get(env, "REDIS_QUEUE_RETRY_AFTER", 90)?,
                "block_for": null,
                "after_commit": false,
            },
            "null": {"driver": "null"},
            "deferred": {"driver": "deferred"},
            "background": {"driver": "background"},
            "failover": {"driver": "failover", "connections": ["database", "deferred"]},
        },

        /*
        |--------------------------------------------------------------------------
        | 批次与失败任务
        |--------------------------------------------------------------------------
        |
        | 配置任务批次和失败任务的记录方式，数据库连接取自默认数据库配置。
        |
        */
        "batching": {"database": database, "table": "job_batches"},
        "failed": {
            "driver": get(env, "QUEUE_FAILED_DRIVER", "database-uuids")?,
            "database": database,
            "table": "failed_jobs",
        },
    }))
}
