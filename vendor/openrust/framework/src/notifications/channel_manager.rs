use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    contracts::notifications::{Dispatcher, Factory},
    database::Transaction,
};

use super::{
    Notifiable, Notification, NotificationError, NotificationFuture, NotificationResult,
    NotificationSender,
    channels::{Channel, DatabaseChannel, MailChannel},
};

/// Resolves notification channels and dispatches notifications.
pub struct ChannelManager {
    channels: RwLock<HashMap<String, Arc<dyn Channel>>>,
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self {
            channels: RwLock::new(
                [
                    ("mail".into(), Arc::new(MailChannel) as Arc<dyn Channel>),
                    (
                        "database".into(),
                        Arc::new(DatabaseChannel) as Arc<dyn Channel>,
                    ),
                ]
                .into(),
            ),
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's notification API")]
impl ChannelManager {
    pub fn extend(&self, name: impl Into<String>, channel: Arc<dyn Channel>) {
        self.channels
            .write()
            .expect("notification channels lock poisoned")
            .insert(name.into(), channel);
    }

    pub fn channel(&self, name: Option<&str>) -> NotificationResult<Arc<dyn Channel>> {
        let name = name.unwrap_or("mail");
        self.channels
            .read()
            .expect("notification channels lock poisoned")
            .get(name)
            .cloned()
            .ok_or_else(|| NotificationError::UnknownChannel { name: name.into() })
    }

    pub async fn send(
        &self,
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
    ) -> NotificationResult<()> {
        self.sendInTransaction(recipients, notification, None).await
    }

    pub async fn sendInTransaction(
        &self,
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
        transaction: Option<&mut Transaction>,
    ) -> NotificationResult<()> {
        NotificationSender::new(self)
            .send(recipients, notification, transaction)
            .await
    }

    pub async fn sendNow(
        &self,
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
        channels: Option<&[&str]>,
    ) -> NotificationResult<()> {
        NotificationSender::new(self)
            .sendNow(recipients, notification, channels)
            .await
    }
}

impl Factory for ChannelManager {
    fn channel(&self, name: Option<&str>) -> NotificationResult<Arc<dyn Channel>> {
        self.channel(name)
    }
}

impl Dispatcher for ChannelManager {
    fn send<'a>(
        &'a self,
        recipients: &'a [&'a dyn Notifiable],
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<()>> {
        Box::pin(self.send(recipients, notification))
    }

    fn send_now<'a>(
        &'a self,
        recipients: &'a [&'a dyn Notifiable],
        notification: &'a dyn Notification,
        channels: Option<&'a [&'a str]>,
    ) -> NotificationFuture<'a, NotificationResult<()>> {
        Box::pin(self.sendNow(recipients, notification, channels))
    }
}
