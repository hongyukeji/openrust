use super::Facade;
use crate::{
    container::ServiceKey, contracts::container::BindingResolutionError, foundation::Application,
    localization::TranslationResult,
};

/// 当前应用入口，复用 Application 与现有请求／任务作用域。
pub struct App;

impl Facade for App {
    type Service = Application;
    fn get_facade_accessor() -> ServiceKey {
        "app".into()
    }
}

impl App {
    /// 从当前应用解析框架版本；未进入应用作用域时返回绑定错误。
    pub fn version() -> Result<&'static str, BindingResolutionError> {
        Ok(Self::get_facade_root()?.version())
    }

    pub fn lang_path(path: &str) -> Result<std::path::PathBuf, BindingResolutionError> {
        Ok(Self::get_facade_root()?.lang_path(path))
    }

    pub fn use_lang_path(
        path: impl Into<std::path::PathBuf>,
    ) -> Result<std::sync::Arc<Application>, BindingResolutionError> {
        let app = Self::get_facade_root()?;
        app.use_lang_path(path)?;
        Ok(app)
    }

    pub fn get_locale() -> TranslationResult<String> {
        Self::application()?.get_locale()
    }
    /// 当前作用域实际使用的应用语言；不重新读取请求偏好或执行协商。
    pub fn current_locale() -> TranslationResult<String> {
        Self::application()?.current_locale()
    }
    pub fn is_locale(locale: &str) -> TranslationResult<bool> {
        Self::application()?.is_locale(locale)
    }
    pub async fn set_locale(locale: &str) -> TranslationResult<()> {
        Self::application()?.set_locale(locale).await
    }
    pub fn get_fallback_locale() -> TranslationResult<String> {
        Self::application()?.get_fallback_locale()
    }
    pub fn set_fallback_locale(fallback_locale: &str) -> TranslationResult<()> {
        Self::application()?.set_fallback_locale(fallback_locale)
    }

    fn application() -> TranslationResult<std::sync::Arc<Application>> {
        Self::get_facade_root().map_err(Into::into)
    }
}
