use openrust::config::{ConfigError, Environment, Value};
use serde_json::json;

use super::env::{get, optional_integer};

pub(super) fn load(env: &Environment) -> Result<Value, ConfigError> {
    let verify = get(env, "HASH_VERIFY", true)?;
    Ok(json!({
        /*
        |--------------------------------------------------------------------------
        | 默认密码哈希算法
        |--------------------------------------------------------------------------
        |
        | HASH_DRIVER 选择算法，HASH_VERIFY 控制验证时是否检查散列使用的算法。
        |
        */
        "driver": get(env, "HASH_DRIVER", "bcrypt")?,

        /*
        |--------------------------------------------------------------------------
        | 算法成本参数
        |--------------------------------------------------------------------------
        |
        | Bcrypt 用轮数控制成本，Argon 使用内存、线程数和时间成本。
        |
        */
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

        /*
        |--------------------------------------------------------------------------
        | 登录时更新散列
        |--------------------------------------------------------------------------
        |
        | 启用后，登录成功且散列参数需要更新时重新生成密码散列。
        |
        */
        "rehash_on_login": true,
    }))
}
