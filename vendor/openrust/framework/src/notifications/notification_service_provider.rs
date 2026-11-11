use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::{
        container::Container as _,
        notifications::{Dispatcher, Factory},
    },
    foundation::Application,
    http::HttpFuture,
    queue::QueueManager,
    support::{ProviderResult, ServiceProvider},
};

use super::{ChannelManager, SendQueuedNotifications};

/// Registers the notification channel manager and queued delivery handler.
pub struct NotificationServiceProvider;

impl ServiceProvider for NotificationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container_handle();
            container
                .singletonNamed("notifications", |_| Ok(Arc::new(ChannelManager::default())))?;
            container.alias("notifications", ServiceKey::typed::<ChannelManager>())?;
            container
                .singleton::<dyn Factory, _>(|container| Ok(container.make::<ChannelManager>()?))?;
            container.singleton::<dyn Dispatcher, _>(|container| {
                Ok(container.make::<ChannelManager>()?)
            })?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container_handle().make::<QueueManager>()?.register(
                SendQueuedNotifications::NAME,
                Arc::new(SendQueuedNotifications),
            )?;
            Ok(())
        })
    }
}
