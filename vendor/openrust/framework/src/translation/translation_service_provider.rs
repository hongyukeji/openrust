use std::sync::Arc;

use crate::{
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
        translation::{Loader, Translator as TranslatorContract},
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{FileLoader, TranslationError, Translator};

/// Registers the file loader and application translator.
pub struct TranslationServiceProvider;

impl ServiceProvider for TranslationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            let paths = container.make::<crate::foundation::Paths>()?;
            container.singleton_named("translation.loader", move |_| {
                Ok(Arc::new(FileLoader::with_defaults(paths.lang_path(""))))
            })?;
            container.singleton::<dyn Loader, _>(|container| {
                Ok(container.get::<FileLoader>("translation.loader")?)
            })?;
            container.scoped::<super::translator::LocaleState, _>(|_| Ok(Arc::default()))?;
            let owner = Arc::downgrade(&app.container_handle());
            container.singleton_named("translator", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                let loader = container.get::<FileLoader>("translation.loader")?;
                let make = || -> Result<Translator, TranslationError> {
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
                    let translator =
                        Translator::with_loader(loader, default_locale, default_fallback)?
                            .with_owner(owner.clone());
                    translator.set_locale(&selected)?;
                    translator.setFallback(&fallback)?;
                    Ok(translator)
                };
                make()
                    .map(Arc::new)
                    .map_err(|source| BindingResolutionError::FactoryFailed {
                        service: "translator",
                        source: Box::new(source),
                    })
            })?;
            container.alias(
                "translator",
                crate::container::ServiceKey::typed::<Translator>(),
            )?;
            container.singleton::<dyn TranslatorContract, _>(|container| {
                Ok(container.get::<Translator>("translator")?)
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
