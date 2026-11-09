//! 可编辑翻译来源配置；首次数据库安装先用文件模式并启用 `install_database`，再运行 `migrate`。

use crate::app::providers::translation_sources::SourceSettings;
use openrust::config::{ConfigContext, ConfigError};

/// 在统一配置清单中提供翻译来源的应用默认值。
///
/// 缺省 `file_root` 时优先使用 `resources/lang` 目录，其次使用根 `lang`。
/// 显式 `lang` 固定使用根 `lang`，允许目录缺失。其余显式目录必须存在。
/// 按优先级顺序填写附加 `paths`。只提供全局 JSON 的目录填写 `json_paths`。
#[allow(
    clippy::unnecessary_wraps,
    reason = "保持 ConfigManifest 配置函数统一的可失败契约"
)]
pub fn config(_ctx: &ConfigContext) -> Result<SourceSettings, ConfigError> {
    Ok(SourceSettings::default())
}
