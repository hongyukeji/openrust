use std::sync::Arc;

use crate::{
    contracts::{
        config::Repository, container::Container as _, events::Dispatcher as _,
        foundation::Application as _,
    },
    events::{Delivery, Dispatcher},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{AuthManager, passwords::PasswordBrokerManager};

/// Registers the application's authentication factory.
pub struct AuthServiceProvider;

impl ServiceProvider for AuthServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            let app_container = Arc::downgrade(&app.container_handle());
            container.singletonNamed("auth", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                Ok(Arc::new(AuthManager::new(app_container.clone(), config)))
            })?;
            container.alias("auth", crate::container::ServiceKey::typed::<AuthManager>())?;
            let owner = Arc::downgrade(&app.container_handle());
            container.singletonNamed("auth.password", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                Ok(Arc::new(PasswordBrokerManager::new(config, owner.clone())))
            })?;
            container.alias(
                "auth.password",
                crate::container::ServiceKey::typed::<PasswordBrokerManager>(),
            )?;
            container.singletonNamed("gate", |_| {
                Ok(Arc::new(super::access::GateRegistry::default()))
            })?;
            container.alias(
                "gate",
                crate::container::ServiceKey::typed::<super::access::GateRegistry>(),
            )?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().make::<Dispatcher>()?.listen(
                "Registered",
                Arc::new(super::listeners::SendEmailVerificationNotification),
                Delivery::Sync,
            )?;
            Ok(())
        })
    }
}
