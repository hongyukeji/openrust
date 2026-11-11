use std::sync::Arc;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
    translation::Translator,
};

use super::Factory;

/// Registers the application's validation factory.
pub struct ValidationServiceProvider;

impl ServiceProvider for ValidationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.singletonNamed("validator", |container| {
                Ok(Arc::new(Factory::new(
                    container.get::<Translator>("translator")?,
                )))
            })?;
            container.alias(
                "validator",
                crate::container::ServiceKey::typed::<Factory>(),
            )?;
            container.singleton::<dyn crate::contracts::validation::Factory, _>(|container| {
                let factory = container.get::<Factory>("validator")?;
                Ok(factory)
            })?;
            Ok(())
        })
    }
}
