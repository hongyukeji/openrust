//! 可选 HTML 视图的应用默认值。

use openrust::config::{ConfigContext, ConfigError, ViewConfig};

/// 只控制代码清单是否默认声明 view；显式 TOML、环境或前置覆盖仍可启用视图。
pub const ENABLED: bool = false;

/// 根应用不默认登记该区段；启用前还须编译 view feature。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<ViewConfig, ConfigError> {
    Ok(ViewConfig {
        directory: "resources/views".into(),
        auto_reload: false,
    })
}
