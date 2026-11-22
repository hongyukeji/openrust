#![allow(non_snake_case, reason = "遵循 Laravel App Facade 方法名")]

use super::Facade;
use crate::{
    container::ServiceKey,
    foundation::Application,
    translation::{TranslationError, TranslationResult},
};

/// 当前应用的语言入口，复用 Application 与现有请求／任务作用域。
pub struct App;

impl Facade for App {
    type Service = Application;
    fn getFacadeAccessor() -> ServiceKey {
        "app".into()
    }
}

impl App {
    pub fn getLocale() -> TranslationResult<String> {
        Self::application()?.getLocale()
    }
    pub fn currentLocale() -> TranslationResult<String> {
        Self::application()?.currentLocale()
    }
    pub fn isLocale(locale: &str) -> TranslationResult<bool> {
        Self::application()?.isLocale(locale)
    }
    pub async fn setLocale(locale: &str) -> TranslationResult<()> {
        Self::application()?.setLocale(locale).await
    }
    pub fn getFallbackLocale() -> TranslationResult<String> {
        Self::application()?.getFallbackLocale()
    }
    pub fn setFallbackLocale(fallbackLocale: &str) -> TranslationResult<()> {
        Self::application()?.setFallbackLocale(fallbackLocale)
    }

    fn application() -> TranslationResult<std::sync::Arc<Application>> {
        Self::getFacadeRoot().map_err(TranslationError::Unavailable)
    }
}
