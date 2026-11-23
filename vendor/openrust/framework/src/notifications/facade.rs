use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{
    AnonymousNotifiable, ChannelManager, Notifiable, Notification, NotificationFake,
    NotificationResult, NotificationRoute, channels::Channel, delivery,
};

/// Resolves notifications for the current application.
pub struct NotificationFacade;

impl Facade for NotificationFacade {
    type Service = ChannelManager;

    fn get_facade_accessor() -> ServiceKey {
        "notifications".into()
    }
}

impl NotificationFacade {
    pub fn fake() -> NotificationResult<Arc<NotificationFake>> {
        Ok(Self::get_facade_root().map_err(delivery)?.fake())
    }

    pub fn channel(name: Option<&str>) -> NotificationResult<Arc<dyn Channel>> {
        Self::get_facade_root().map_err(delivery)?.channel(name)
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
        Self::get_facade_root()
            .map_err(delivery)?
            .send(recipients, notification)
            .await
    }

    pub async fn send_now(
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
        channels: Option<&[&str]>,
    ) -> NotificationResult<()> {
        Self::get_facade_root()
            .map_err(delivery)?
            .send_now(recipients, notification, channels)
            .await
    }
}
