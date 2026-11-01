//! 签名链接与加密配置结构。

use openrust::config::{ConfigContext, ConfigError};
use serde::Serialize;

#[derive(Serialize)]
struct KeyRing {
    active_key_id: &'static str,
    keys: std::collections::BTreeMap<String, String>,
}

#[derive(Serialize)]
struct SignedLinks {
    issuer: String,
    #[serde(flatten)]
    keys: KeyRing,
}

#[derive(Serialize)]
pub struct Security {
    signed_links: SignedLinks,
    encryption: KeyRing,
}

/// 密钥值由部署环境覆写，源码不持有私钥。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(ctx: &ConfigContext) -> Result<Security, ConfigError> {
    Ok(Security {
        signed_links: SignedLinks {
            issuer: ctx.env_or("APP_URL", "http://localhost:3000"),
            keys: KeyRing {
                active_key_id: "local",
                keys: std::collections::BTreeMap::new(),
            },
        },
        encryption: KeyRing {
            active_key_id: "local",
            keys: std::collections::BTreeMap::new(),
        },
    })
}
