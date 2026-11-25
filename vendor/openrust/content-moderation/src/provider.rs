use openrust::{
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

use crate::{
    ModerationCredentialProvider, ModerationManager, configuration, credentials::FileCredentials,
    drivers,
};

pub struct ModerationServiceProvider;

impl ServiceProvider for ModerationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, configuration::load(&env)?, "moderation")?;
            }
            let container = app.container();
            container.singleton_if::<dyn ModerationCredentialProvider, _>(|container| {
                Ok(Arc::new(FileCredentials(container.get("config")?)))
            })?;
            container.singleton_named("moderation", |container| {
                let manager = ModerationManager::new(container.get("config")?);
                drivers::register(
                    &manager,
                    container.make()?,
                    container.make()?,
                    container.make()?,
                    container.make()?,
                );
                Ok(Arc::new(manager))
            })?;
            container.alias("moderation", ServiceKey::typed::<ModerationManager>())?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[(
                    "config/moderation.rs",
                    include_bytes!("../config/moderation.rs"),
                )],
                "moderation-config",
            )
        })
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(ModerationServiceProvider)]
}
