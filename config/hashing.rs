use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, optional_integer};

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    let verify = get(env, "HASH_VERIFY", true)?;
    Ok(json!({
        "driver": get(env, "HASH_DRIVER", "bcrypt")?,
        "bcrypt": {
            "rounds": get(env, "BCRYPT_ROUNDS", 12)?,
            "verify": verify,
            "limit": optional_integer(env, "BCRYPT_LIMIT")?,
        },
        "argon": {
            "memory": get(env, "ARGON_MEMORY", 65536)?,
            "threads": get(env, "ARGON_THREADS", 1)?,
            "time": get(env, "ARGON_TIME", 4)?,
            "verify": verify,
        },
        "rehash_on_login": true,
    }))
}
