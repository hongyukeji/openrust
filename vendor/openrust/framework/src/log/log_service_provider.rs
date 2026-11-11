use std::sync::Arc;

use crate::{
    contracts::{container::Container as _, foundation::Application as _, log},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{LogManager, context::Repository};

/// Registers the application log manager and logger binding.
pub struct LogServiceProvider;

impl ServiceProvider for LogServiceProvider {
    fn register<'a>(&'a mut self, application: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let root = application.base_path().to_path_buf();
            let owner = Arc::downgrade(&application.container_handle());
            let container = application.container();
            container.instance(Arc::new(super::context::Hooks::default()))?;
            container.scopedIf::<Repository, _>(|container| {
                if !container.has_active_scope() {
                    return Err(crate::contracts::container::BindingResolutionError::ScopeClosed);
                }
                Ok(Arc::new(Repository::default()))
            })?;
            container.singletonNamed("log", move |container| {
                let config = container.get::<dyn crate::contracts::config::Repository>("config")?;
                Ok(Arc::new(
                    LogManager::new(config, root.clone()).with_context_owner(owner.clone()),
                ))
            })?;
            container.alias("log", crate::container::ServiceKey::typed::<LogManager>())?;
            container.singleton::<dyn log::Logger, _>(|container| {
                let manager = container.get::<LogManager>("log")?;
                Ok(manager)
            })?;
            Ok(())
        })
    }
}
