use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;
pub struct ScoutServiceProvider;
impl ServiceProvider for ScoutServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, crate::configuration::load(&env)?, "scout")?;
            }
            app.container().singleton::<crate::EngineManager, _>(|c| {
                Ok(Arc::new(crate::EngineManager::new(
                    c.get::<dyn openrust::contracts::config::Repository>("config")?,
                    c.make::<openrust::queue::QueueManager>()?,
                )))
            })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[("config/scout.rs", include_bytes!("../config/scout.rs"))],
                "scout-config",
            )?;
            self.commands(app, crate::commands::register);
            Ok(())
        })
    }
}
