//! 默认语言及消息回退的应用默认值。

use openrust::config::{ConfigContext, ConfigError};
use openrust::translation::TranslationConfig;
use openrust::translation::Locale;

/// Locale 由公开解析入口校验和规范化；消息目录仍须包含所选语言。
pub fn config(_ctx: &ConfigContext) -> Result<TranslationConfig, ConfigError> {
    Ok(TranslationConfig {
        default_locale: Locale::new("en")?,
        fallback_locale: Locale::new("en")?,
    })
}
