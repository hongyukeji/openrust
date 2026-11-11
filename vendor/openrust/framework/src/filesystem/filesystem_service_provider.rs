use std::sync::Arc;

use crate::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::FilesystemManager;

/// Registers the application-scoped filesystem manager.
pub struct FilesystemServiceProvider;

impl ServiceProvider for FilesystemServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let root = app.base_path().to_path_buf();
            let container = app.container();
            container.singletonNamed("filesystem", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                Ok(Arc::new(FilesystemManager::new(config, root.clone())))
            })?;
            container.alias(
                "filesystem",
                crate::container::ServiceKey::typed::<FilesystemManager>(),
            )?;
            Ok(())
        })
    }
}
