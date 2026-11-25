use openrust::config::{ConfigError, Environment, Value};

/// 配置仅保存凭据引用；解析时从当前应用的环境快照读取，不写入配置缓存。
pub fn load(_: &Environment) -> Result<Value, ConfigError> {
    Ok(serde_json::json!({
        "configs": [{
            "name":"default", "revision":"1",
            "signing_secret":{"env":"WEBHOOK_CLIENT_SECRET"},
            "signature_header_name":"Signature", "signature_validator":"default",
            "webhook_profile":"process-everything", "webhook_response":"default",
            "webhook_model":"database", "scope":null, "connection":null, "process_webhook_job":"",
            "atomic_database_queue":false, "recover_dispatch":false, "recover_processing":null, "deduplicate_verified_events":false, "deduplicate_body_seconds":null, "store_headers":[], "store_attachments":true,
            "max_body_bytes":1048576,
            "verification_secrets":[]
        }],
        "delete_after_days":30,
        "add_unique_token_to_route_name":false
    }))
}
