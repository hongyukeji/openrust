use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, optional_integer};

pub(super) fn load(env: &Environment, database: &Value) -> Result<Value, ConfigError> {
    Ok(json!({
        "default": get(env, "QUEUE_CONNECTION", "database")?,
        "memory_limit": optional_integer(env, "QUEUE_WORKER_MEMORY_LIMIT")?,
        "kill_on_timeout": get(env, "QUEUE_WORKER_KILL_ON_TIMEOUT", true)?,
        "timed_out_exit_code": optional_integer(env, "QUEUE_WORKER_TIMED_OUT_EXIT_CODE")?,
        "stop_on_lost_connection": get(env, "QUEUE_WORKER_STOP_ON_LOST_CONNECTION", true)?,
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
                "queue": get(env, "BEANSTALKD_QUEUE", "default")?,
                "retry_after": get(env, "BEANSTALKD_QUEUE_RETRY_AFTER", 90)?,
                "block_for": 0,
                "after_commit": false,
            },
            "sqs": {
                "driver": "sqs",
                "key": get(env, "AWS_ACCESS_KEY_ID", Value::Null)?,
                "secret": get(env, "AWS_SECRET_ACCESS_KEY", Value::Null)?,
                "prefix": get(env, "SQS_PREFIX", "https://sqs.us-east-1.amazonaws.com/your-account-id")?,
                "queue": get(env, "SQS_QUEUE", "default")?,
                "suffix": get(env, "SQS_SUFFIX", Value::Null)?,
                "region": get(env, "AWS_DEFAULT_REGION", "us-east-1")?,
                "after_commit": false,
                "overflow": {
                    "enabled": get(env, "SQS_OVERFLOW_ENABLED", false)?,
                    "store": get(env, "SQS_OVERFLOW_STORE", Value::Null)?,
                    "always": false,
                    "delete_after_processing": true,
                    "flush_on_clear": get(env, "SQS_OVERFLOW_FLUSH_ON_CLEAR", false)?,
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
            "deferred": {"driver": "deferred"},
            "background": {"driver": "background"},
            "failover": {"driver": "failover", "connections": ["database", "deferred"]},
        },
        "batching": {"database": database, "table": "job_batches"},
        "failed": {
            "driver": get(env, "QUEUE_FAILED_DRIVER", "database-uuids")?,
            "database": database,
            "table": "failed_jobs",
        },
    }))
}
