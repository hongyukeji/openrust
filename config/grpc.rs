//! gRPC listener 默认值，仅启用 grpc feature 时编译。

use openrust::config::{ConfigContext, ConfigError, GrpcConfig};

/// 使用框架默认回环 listener。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<GrpcConfig, ConfigError> {
    Ok(GrpcConfig::default())
}
