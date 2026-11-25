use openrust::config::{ConfigError, Environment, Value};

pub fn load(_: &Environment) -> Result<Value, ConfigError> {
    Ok(serde_json::json!({
        "queue":"default", "connection":null, "http_verb":"post",
        "signer":"default", "signature_header_name":"Signature",
        "timestamp_header_name":"Timestamp", "webhook_id_header_name":"Webhook-Id",
        "headers":{"Content-Type":"application/json"}, "timeout_in_seconds":3, "tries":3,
        "backoff_strategy":"exponential", "retryable_status_codes":null,
        "respect_retry_after":false, "retry_with_jitter":false,
        "webhook_job":"default", "verify_ssl":true,
        "throw_exception_on_failure":false, "proxy":null, "tags":[]
    }))
}
