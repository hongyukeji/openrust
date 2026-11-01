//! 视图目录默认值。

use openrust::config::{ConfigContext, ConfigError, ViewConfig};

/// 生成应用将视图放在 `resources/views/`。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(_ctx: &ConfigContext) -> Result<ViewConfig, ConfigError> {
    let mut view = ViewConfig::default();
    view.directory = std::path::PathBuf::from("resources/views");
    Ok(view)
}
