use std::sync::Arc;

use crate::{
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        localization::{Loader, Localizer as LocalizerContract},
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{FileLoader, Localizer, TranslationError};

/// Registers the file loader and application localizer.
pub struct LocalizationServiceProvider;

impl ServiceProvider for LocalizationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            let paths = container.make::<crate::foundation::Paths>()?;
            container.singleton::<FileLoader, _>(move |_| {
                Ok(Arc::new(FileLoader::with_defaults(paths.lang_path(""))))
            })?;
            container.singleton_named::<dyn Loader, _>("lang.loader", |container| {
                Ok(container.make::<FileLoader>()?)
            })?;
            container.alias(
                "lang.loader",
                crate::container::ServiceKey::typed::<dyn Loader>(),
            )?;
            container.scoped::<super::localizer::LocaleState, _>(|_| Ok(Arc::default()))?;
            let owner = Arc::downgrade(&app.container_handle());
            container.singleton_named("lang", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                let loader = container.make::<FileLoader>()?;
                let make = || -> Result<Localizer, TranslationError> {
                    let selected = locale(config.as_ref(), "app.locale")?;
                    let fallback = locale(config.as_ref(), "app.fallback_locale")?;
                    // 首次解析也可能发生在请求内；常驻默认值不能取自请求配置。
                    let (default_locale, default_fallback) =
                        crate::container::ScopeSnapshot::without(container).run(|| {
                            Ok::<_, TranslationError>((
                                locale(config.as_ref(), "app.locale")?,
                                locale(config.as_ref(), "app.fallback_locale")?,
                            ))
                        })?;
                    let localizer =
                        Localizer::with_loader(loader, default_locale, default_fallback)?
                            .with_owner(owner.clone());
                    localizer.set_locale(&selected)?;
                    localizer.set_fallback(&fallback)?;
                    Ok(localizer)
                };
                make()
                    .map(Arc::new)
                    .map_err(|source| BindingResolutionError::FactoryFailed {
                        service: "lang",
                        source: Box::new(source),
                    })
            })?;
            container.alias("lang", crate::container::ServiceKey::typed::<Localizer>())?;
            container.singleton::<dyn LocalizerContract, _>(|container| {
                Ok(container.get::<Localizer>("lang")?)
            })?;
            Ok(())
        })
    }
}

fn locale(config: &dyn Repository, key: &str) -> Result<String, TranslationError> {
    match config.get(key) {
        Some(serde_json::Value::String(locale)) => Ok(locale),
        None => Ok("en".to_owned()),
        _ => Err(TranslationError::InvalidLocale),
    }
}
