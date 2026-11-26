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
            container.instance(Arc::new(super::browser::Control::default()))?;
            container.scoped::<super::browser::Buffer, _>(|_| {
                Ok(Arc::new(super::browser::Buffer::default()))
            })?;
            let listeners = Arc::new(super::events::Listeners::default());
            container.instance(listeners.clone())?;
            container.instance(Arc::new(super::context::Hooks::default()))?;
            let defaults = Arc::new(super::channel_context::ChannelContext::default());
            container.scoped::<super::channel_context::ChannelContext, _>(move |container| {
                Ok(if container.has_active_scope() {
                    Arc::new(super::channel_context::ChannelContext(
                        std::sync::Mutex::new(
                            defaults.0.lock().expect("日志默认上下文锁中毒").clone(),
                        ),
                    ))
                } else {
                    defaults.clone()
                })
            })?;
            container.scoped_if::<Repository, _>(|container| {
                if !container.has_active_scope() {
                    return Err(crate::contracts::container::BindingResolutionError::ScopeClosed);
                }
                Ok(Arc::new(Repository::default()))
            })?;
            container.singleton_named("log", move |container| {
                let config = container.get::<dyn crate::contracts::config::Repository>("config")?;
                Ok(Arc::new(
                    LogManager::new(config, root.clone())
                        .with_paths(container.make::<crate::foundation::Paths>()?)
                        .with_context_owner(owner.clone())
                        .with_listeners(listeners.clone()),
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
