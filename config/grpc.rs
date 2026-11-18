use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | gRPC 服务与客户端
        |--------------------------------------------------------------------------
        |
        | GRPC_HOST 和 GRPC_PORT 配置服务监听地址，clients 用于登记具名客户端。
        |
        */
        "host": env.get_utf8("GRPC_HOST")?,
        "port": env.get_utf8("GRPC_PORT")?,
        "clients": {},
    }))
}
