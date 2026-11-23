#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use super::{Application, events::LocaleUpdated};
use crate::{
    contracts::{
        config::Repository, container::Container as _, events::Dispatcher as _,
        foundation::Application as _,
    },
    events::{Dispatcher, EventContext},
    translation::{TranslationError, TranslationResult, Translator, canonical_locale},
};
use std::sync::Arc;

impl Application {
    /// 读取当前应用／请求的语言配置。
    pub fn get_locale(&self) -> TranslationResult<String> {
        self.locale_config("app.locale")
    }

    pub fn current_locale(&self) -> TranslationResult<String> {
        self.get_locale()
    }

    pub fn is_locale(&self, locale: &str) -> TranslationResult<bool> {
        Ok(canonical_locale(&self.get_locale()?)? == canonical_locale(locale)?)
    }

    pub fn get_fallback_locale(&self) -> TranslationResult<String> {
        self.locale_config("app.fallback_locale")
    }

    /// 按配置、译器、事件的顺序切换语言，等待事件监听器完成。
    pub async fn set_locale(&self, locale: &str) -> TranslationResult<()> {
        let locale = canonical_locale(locale)?;
        let config = self.container().get::<dyn Repository>("config")?;
        let previousLocale = config
            .get("app.locale")
            .and_then(|value| value.as_str().map(str::to_owned))
            .map(|locale| canonical_locale(&locale))
            .transpose()?;
        config
            .set("app.locale", locale.clone().into())
            .map_err(source)?;
        self.container()
            .get::<Translator>("translator")?
            .set_locale(&locale)?;
        let events = self.container().get::<Dispatcher>("events")?;
        events
            .dispatch(
                Arc::new(LocaleUpdated {
                    locale,
                    previousLocale,
                }),
                &mut EventContext::new(),
            )
            .await
            .map_err(source)?;
        Ok(())
    }

    pub fn set_fallback_locale(&self, fallback_locale: &str) -> TranslationResult<()> {
        let locale = canonical_locale(fallback_locale)?;
        let config = self.container().get::<dyn Repository>("config")?;
        config
            .set("app.fallback_locale", locale.clone().into())
            .map_err(source)?;
        self.container()
            .get::<Translator>("translator")?
            .setFallback(&locale)
    }

    fn locale_config(&self, key: &str) -> TranslationResult<String> {
        self.container()
            .get::<dyn Repository>("config")?
            .get(key)
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or(TranslationError::InvalidLocale)
            .and_then(|locale| canonical_locale(&locale))
    }
}

fn source(error: impl std::error::Error + Send + Sync + 'static) -> TranslationError {
    TranslationError::Source(Box::new(error))
}
