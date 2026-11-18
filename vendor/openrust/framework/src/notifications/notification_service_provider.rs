use std::sync::Arc;

use crate::{
    container::ServiceKey,
    contracts::{
        config::Repository,
        container::Container as _,
        notifications::{Dispatcher, Factory},
    },
    events::Dispatcher as EventDispatcher,
    foundation::Application,
    http::HttpFuture,
    queue::QueueManager,
    support::{ProviderResult, ServiceProvider},
};

use super::{ChannelManager, SendQueuedNotifications, channels::BroadcastNotificationCreated};

/// Registers the notification channel manager and queued delivery handler.
pub struct NotificationServiceProvider;

impl ServiceProvider for NotificationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container_handle();
            container
                .singletonNamed("notifications", |_| Ok(Arc::new(ChannelManager::default())))?;
            container.singleton::<super::sms::SmsManager, _>(|container| {
                Ok(Arc::new(super::sms::SmsManager::new(
                    container.get::<dyn Repository>("config")?,
                )))
            })?;
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
            let container = app.container_handle();
            container
                .make::<EventDispatcher>()?
                .registerBroadcast::<BroadcastNotificationCreated>();
            container.make::<QueueManager>()?.register(
                SendQueuedNotifications::NAME,
                Arc::new(SendQueuedNotifications),
            )?;
            self.publishes(
                app,
                &[(
                    "resources/views/vendor/notifications/email.blade.html",
                    super::messages::EMAIL_VIEW.as_bytes(),
                )],
                "laravel-notifications",
            )
        })
    }
}
