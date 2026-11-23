use std::sync::Arc;

use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
    translation::Translator,
};

use super::{DatabasePresenceVerifier, Factory};

/// Registers the application's validation factory.
pub struct ValidationServiceProvider;

impl ServiceProvider for ValidationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.singleton::<crate::foundation::http::FormRequestDefaults, _>(|_| {
                Ok(Arc::new(
                    crate::foundation::http::FormRequestDefaults::default(),
                ))
            })?;
            container.singleton_named("validator", |container| {
                let mut factory = Factory::new(container.get::<Translator>("translator")?);
                factory.set_presence_verifier(DatabasePresenceVerifier::new(
                    container.make::<dyn crate::database::ConnectionResolverInterface>()?,
                ));
                Ok(Arc::new(factory))
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
