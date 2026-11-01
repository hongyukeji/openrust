//! 示例模块的可编辑 Rust 配置函数。

use openrust::config::{ConfigContext, ConfigError};

/// 与应用共享同一个 `ConfigRepository` 的模块区段。
pub(crate) fn boot() -> openrust::config::ConfigManifest {
    openrust::config::ConfigManifest::new().section("example", config)
}

/// 只有显式启用策略演示时才登记无法缓存的进程本地工厂。
pub(crate) fn runtime_policy_manifest() -> openrust::config::ConfigManifest {
    openrust::config::ConfigManifest::new().typed("example.runtime_policy", runtime_policy)
}

/// 在应用同次 dotenv 快照上创建模块配置。
pub(crate) fn config(ctx: &ConfigContext) -> Result<serde_json::Value, ConfigError> {
    // 仅登记此文件拥有的部署设置，避免完整 typed 默认值把宿主可选策略覆盖成 null。
    Ok(serde_json::json!({
        "greeting": ctx.env_or("EXAMPLE_GREETING", "Hello from modules/example"),
        "feature": {"enabled": ctx.bool_or("EXAMPLE_FEATURE_ENABLED", true)?},
    }))
}

/// 仅在启用模块时构造的进程本地策略值；由 Provider 消费。
pub(crate) struct ExampleRuntimePolicy {
    pub(crate) greeting_prefix: String,
}

/// typed 配置工厂不能写入配置缓存，需在每次引导时重新求值。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest typed 工厂统一的可失败契约"
)]
pub(crate) fn runtime_policy(ctx: &ConfigContext) -> Result<ExampleRuntimePolicy, ConfigError> {
    Ok(ExampleRuntimePolicy {
        greeting_prefix: ctx.env_or("EXAMPLE_GREETING_PREFIX", "Example"),
    })
}
