//! 签名链接与加密密钥环的应用默认值。

use openrust::config::{ConfigContext, ConfigError};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct KeyRing {
    active_key_id: &'static str,
    keys: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct SignedLinks {
    issuer: String,
    #[serde(flatten)]
    keys: KeyRing,
    max_ttl_secs: u64,
}

/// 安全应用默认项，显式呈现签名与加密密钥环。
#[derive(Serialize)]
pub struct Security {
    signed_links: SignedLinks,
    encryption: KeyRing,
}

/// 密钥由部署来源覆写；签发者默认继承同次最终 app.url，不读取请求 Host。
pub fn config(ctx: &ConfigContext) -> Result<Security, ConfigError> {
    Ok(Security {
        signed_links: SignedLinks {
            issuer: ctx.require("app.url")?,
            keys: KeyRing {
                active_key_id: "local",
                keys: BTreeMap::new(),
            },
            max_ttl_secs: 86_400,
        },
        encryption: KeyRing {
            active_key_id: "local",
            keys: BTreeMap::new(),
        },
    })
}
