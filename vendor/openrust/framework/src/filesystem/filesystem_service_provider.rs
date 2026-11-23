use std::sync::Arc;

use crate::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{Filesystem, FilesystemManager};

/// Registers the application-scoped filesystem manager.
pub struct FilesystemServiceProvider;

impl ServiceProvider for FilesystemServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let root = app.base_path().to_path_buf();
            let container = app.container();
            container.singleton_named("files", |_| Ok(Arc::new(Filesystem::new())))?;
            container.alias("files", crate::container::ServiceKey::typed::<Filesystem>())?;
            container.singleton_named("filesystem", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                Ok(Arc::new(FilesystemManager::new(config, root.clone())))
            })?;
            container.alias(
                "filesystem",
                crate::container::ServiceKey::typed::<FilesystemManager>(),
            )?;
            container.singleton::<dyn crate::contracts::filesystem::Factory, _>(|container| {
                Ok(container.make::<FilesystemManager>()?)
            })?;
            container.singleton::<dyn crate::contracts::filesystem::Filesystem, _>(
                |container| {
                    container
                        .make::<FilesystemManager>()?
                        .disk(None)
                        .map(|disk| disk as Arc<dyn crate::contracts::filesystem::Filesystem>)
                        .map_err(|source| {
                            crate::contracts::container::BindingResolutionError::FactoryFailed {
                                service: "filesystem.disk",
                                source: Box::new(source),
                            }
                        })
                },
            )?;
            container.singleton::<dyn crate::contracts::filesystem::Cloud, _>(|container| {
                let manager = container.make::<FilesystemManager>()?;
                manager
                    .disk(Some(&manager.get_default_cloud_driver().map_err(
                        |source| {
                            crate::contracts::container::BindingResolutionError::FactoryFailed {
                                service: "filesystem.cloud",
                                source: Box::new(source),
                            }
                        },
                    )?))
                    .map(|disk| disk as Arc<dyn crate::contracts::filesystem::Cloud>)
                    .map_err(|source| {
                        crate::contracts::container::BindingResolutionError::FactoryFailed {
                            service: "filesystem.cloud",
                            source: Box::new(source),
                        }
                    })
            })?;
            Ok(())
        })
    }
}
