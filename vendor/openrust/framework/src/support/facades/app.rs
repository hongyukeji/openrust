#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use super::Facade;
use crate::{
    container::ServiceKey, contracts::container::BindingResolutionError, foundation::Application,
    translation::TranslationResult,
};

/// 当前应用入口，复用 Application 与现有请求／任务作用域。
pub struct App;

impl Facade for App {
    type Service = Application;
    fn getFacadeAccessor() -> ServiceKey {
        "app".into()
    }
}

impl App {
    /// 从当前应用解析框架版本；未进入应用作用域时返回绑定错误。
    pub fn version() -> Result<&'static str, BindingResolutionError> {
        Ok(Self::getFacadeRoot()?.version())
    }

    pub fn langPath(path: &str) -> Result<std::path::PathBuf, BindingResolutionError> {
        Ok(Self::getFacadeRoot()?.langPath(path))
    }

    pub fn useLangPath(
        path: impl Into<std::path::PathBuf>,
    ) -> Result<std::sync::Arc<Application>, BindingResolutionError> {
        let app = Self::getFacadeRoot()?;
        app.useLangPath(path)?;
        Ok(app)
    }

    pub fn getLocale() -> TranslationResult<String> {
        Self::application()?.getLocale()
    }
    /// 当前作用域实际使用的应用语言；不重新读取请求偏好或执行协商。
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
        Self::getFacadeRoot().map_err(Into::into)
    }
}
