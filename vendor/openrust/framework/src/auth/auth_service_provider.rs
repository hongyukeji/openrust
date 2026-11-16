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
            #[cfg(feature = "sanctum")]
            container.singleton::<crate::sanctum::Sanctum, _>(|container| {
                Ok(Arc::new(crate::sanctum::Sanctum::configured(
                    container.get::<dyn Repository>("config")?,
                )))
            })?;
            let owner = Arc::downgrade(&app.container_handle());
            container.singletonNamed("auth.password", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                Ok(Arc::new(PasswordBrokerManager::new(config, owner.clone())))
            })?;
            container.alias(
                "auth.password",
                crate::container::ServiceKey::typed::<PasswordBrokerManager>(),
            )?;
            let owner = Arc::downgrade(&app.container_handle());
            container.singletonNamed("gate", move |_| {
                Ok(Arc::new(super::access::GateRegistry::for_application(
                    owner.clone(),
                )))
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
            #[cfg(feature = "sanctum")]
            app.publishable()
                .lock()
                .expect("publish registry lock")
                .extend(crate::sanctum_service_provider::publishable_resources());
            app.container().make::<Dispatcher>()?.listen(
                "Registered",
                Arc::new(super::listeners::SendEmailVerificationNotification),
                Delivery::Sync,
            )?;
            Ok(())
        })
    }
}
