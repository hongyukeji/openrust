use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{
    AnonymousNotifiable, ChannelManager, Notifiable, Notification, NotificationResult,
    NotificationRoute, channels::Channel, delivery,
};

/// Resolves notifications for the current application.
pub struct NotificationFacade;

impl Facade for NotificationFacade {
    type Service = ChannelManager;

    fn getFacadeAccessor() -> ServiceKey {
        "notifications".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Notification facade")]
impl NotificationFacade {
    pub fn channel(name: Option<&str>) -> NotificationResult<Arc<dyn Channel>> {
        Self::getFacadeRoot().map_err(delivery)?.channel(name)
    }

    pub fn route(
        channel: impl Into<String>,
        route: NotificationRoute,
    ) -> NotificationResult<AnonymousNotifiable> {
        AnonymousNotifiable::default().route(channel, route)
    }

    pub async fn send(
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
    ) -> NotificationResult<()> {
        Self::getFacadeRoot()
            .map_err(delivery)?
            .send(recipients, notification)
            .await
    }

    pub async fn sendNow(
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
        channels: Option<&[&str]>,
    ) -> NotificationResult<()> {
        Self::getFacadeRoot()
            .map_err(delivery)?
            .sendNow(recipients, notification, channels)
            .await
    }
}
