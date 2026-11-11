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
            let root = app.base_path();
            let legacy = root.join("resources/lang");
            let lang = if legacy.is_dir() {
                legacy
            } else {
                root.join("lang")
            };
            container.singletonNamed("translation.loader", move |_| {
                Ok(Arc::new(FileLoader::new(lang.clone())))
            })?;
            container.singleton::<dyn Loader, _>(|container| {
                Ok(container.get::<FileLoader>("translation.loader")?)
            })?;
            container.singletonNamed("translator", |container| {
                let config = container.get::<dyn Repository>("config")?;
                let loader = container.get::<FileLoader>("translation.loader")?;
                let make = || {
                    let selected = locale(config.as_ref(), "app.locale")?;
                    let fallback = locale(config.as_ref(), "app.fallback_locale")?;
                    Translator::new((*loader).clone(), selected, fallback)
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
