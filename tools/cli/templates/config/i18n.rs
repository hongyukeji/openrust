//! 国际化默认值。

use openrust::config::{ConfigContext, ConfigError};

/// 默认及回退 locale 均为英文。
#[allow(clippy::unnecessary_wraps, reason = "保持 ConfigManifest 配置函数统一的可失败契约")]
pub fn config(_ctx: &ConfigContext) -> Result<openrust::i18n::I18nConfig, ConfigError> {
    Ok(openrust::i18n::I18nConfig::default())
}
