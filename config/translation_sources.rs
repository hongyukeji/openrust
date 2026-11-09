//! 可编辑翻译来源配置；首次数据库安装先用文件模式并启用 `install_database`，再运行 `migrate`。

use crate::app::providers::translation_sources::SourceSettings;
use openrust::config::{ConfigContext, ConfigError};

/// 在统一配置清单中提供翻译来源的应用默认值。
///
/// `file_root` 为必需文件根。需要追加语言目录时，按优先级顺序填写
/// `paths`；只提供全局 JSON 的目录填写 `json_paths`。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<SourceSettings, ConfigError> {
    Ok(SourceSettings::default())
}
