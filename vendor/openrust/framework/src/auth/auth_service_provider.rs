use std::sync::Arc;

use crate::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::AuthManager;

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
            Ok(())
        })
    }
}
