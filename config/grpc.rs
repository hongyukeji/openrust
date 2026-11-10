use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    Ok(json!({
        "host": env.get_utf8("GRPC_HOST")?,
        "port": env.get_utf8("GRPC_PORT")?,
        "clients": {},
    }))
}
