//! 可编辑翻译来源配置；首次数据库安装先用文件模式并启用 `install_database`，再运行 `migrate`。

use crate::app::providers::translation_sources::SourceSettings;
use openrust::config::{ConfigContext, ConfigError};

/// 在统一配置清单中提供翻译来源的应用默认值。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<SourceSettings, ConfigError> {
    Ok(SourceSettings::default())
}
