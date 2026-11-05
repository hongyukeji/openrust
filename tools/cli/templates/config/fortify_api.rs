//! 显式JWT API认证profile；不改变官方Fortify Web默认配置。
use openrust::{
    auth::fortify::FortifyJwtApiConfig,
    config::{ConfigContext, ConfigError},
};
#[allow(clippy::unnecessary_wraps, reason = "应用配置清单统一使用可失败函数")]
pub fn config(_: &ConfigContext) -> Result<FortifyJwtApiConfig, ConfigError> {
    Ok(FortifyJwtApiConfig {
        guard: "api".into(),
        prefix: "/auth".into(),
    })
}
