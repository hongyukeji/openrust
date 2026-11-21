use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::{
        config::Repository, container::Container as _, foundation::Application as _, mail::Factory,
    },
    foundation::Application,
    http::HttpFuture,
    queue::QueueManager,
    support::{ProviderResult, ServiceProvider},
};

use super::{MailManager, SendQueuedMailable};

/// Registers the application mail factory and queued mail handler.
pub struct MailServiceProvider;

impl ServiceProvider for MailServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container_handle();
            container
                .scoped::<super::address_defaults::AddressOverrides, _>(|_| Ok(Arc::default()))?;
            let owner = Arc::downgrade(&container);
            let base = app.base_path().to_owned();
            container.singletonNamed("mail.manager", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                Ok(Arc::new(MailManager::new(
                    config,
                    owner.clone(),
                    base.clone(),
                )))
            })?;
            container.alias("mail.manager", ServiceKey::typed::<MailManager>())?;
            container
                .singleton::<dyn Factory, _>(|container| Ok(container.make::<MailManager>()?))?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container_handle()
                .make::<QueueManager>()?
                .register(SendQueuedMailable::NAME, Arc::new(SendQueuedMailable))?;
            let components: Vec<_> = super::markdown_components::publishable().collect();
            let views: Vec<_> = super::markdown::PUBLISHABLE_VIEWS
                .iter()
                .copied()
                .chain(
                    components
                        .iter()
                        .map(|(path, bytes)| (path.as_str(), *bytes)),
                )
                .collect();
            self.publishes(app, &views, "laravel-mail")
        })
    }
}
