use std::path::Path;

use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, path};

pub(super) fn load(base_path: &Path, env: &Environment) -> Result<Value, ConfigError> {
    let compiled = base_path.join("storage/framework/views");
    Ok(json!({
        "paths": [path(&base_path.join("resources/views"))?],
        "compiled": get(env, "VIEW_COMPILED_PATH", path(&compiled)?)?,
    }))
}
