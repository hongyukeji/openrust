use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    contracts::notifications::{Dispatcher, Factory},
    database::Transaction,
};

use super::{
    Notifiable, Notification, NotificationError, NotificationFake, NotificationFuture,
    NotificationQueueGuard, NotificationResult, NotificationSender,
    channels::{BroadcastChannel, Channel, DatabaseChannel, MailChannel, SlackChannel, SmsChannel},
};

type GuardHandler = dyn for<'a> Fn(&'a serde_json::Value) -> NotificationFuture<'a, NotificationResult<bool>>
    + Send
    + Sync;

/// Resolves notification channels and dispatches notifications.
pub struct ChannelManager {
    channels: RwLock<HashMap<String, Arc<dyn Channel>>>,
    fake: RwLock<Option<Arc<NotificationFake>>>,
    queue_guards: RwLock<HashMap<String, Arc<GuardHandler>>>,
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self {
            fake: RwLock::default(),
            queue_guards: RwLock::default(),
            channels: RwLock::new(
                [
                    ("mail".into(), Arc::new(MailChannel) as Arc<dyn Channel>),
                    (
                        "broadcast".into(),
                        Arc::new(BroadcastChannel) as Arc<dyn Channel>,
                    ),
                    (
                        "database".into(),
                        Arc::new(DatabaseChannel) as Arc<dyn Channel>,
                    ),
                    ("slack".into(), Arc::new(SlackChannel) as Arc<dyn Channel>),
                    (
                        "sms".into(),
                        Arc::new(SmsChannel::default()) as Arc<dyn Channel>,
                    ),
                ]
                .into(),
            ),
        }
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl ChannelManager {
    pub fn fake(&self) -> Arc<NotificationFake> {
        let fake = Arc::new(NotificationFake::default());
        *self.fake.write().expect("notification fake lock poisoned") = Some(fake.clone());
        fake
    }

    pub(super) fn active_fake(&self) -> Option<Arc<NotificationFake>> {
        self.fake
            .read()
            .expect("notification fake lock poisoned")
            .clone()
    }

    /// Registers a delivery decision in applications that dispatch or work notification queues.
    pub fn extendQueueGuard(
        &self,
        name: impl Into<String>,
        handler: impl for<'a> Fn(
            &'a serde_json::Value,
        ) -> NotificationFuture<'a, NotificationResult<bool>>
        + Send
        + Sync
        + 'static,
    ) {
        self.queue_guards
            .write()
            .expect("notification queue guard lock poisoned")
            .insert(name.into(), Arc::new(handler));
    }

    pub(super) async fn allows_queued(
        &self,
        guard: &NotificationQueueGuard,
    ) -> NotificationResult<bool> {
        let handler = self
            .queue_guards
            .read()
            .expect("notification queue guard lock poisoned")
            .get(&guard.name)
            .cloned()
            .ok_or_else(|| super::delivery("Notification queue guard is not registered."))?;
        handler(&guard.data).await
    }

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

    pub async fn send_now(
        &self,
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
        channels: Option<&[&str]>,
    ) -> NotificationResult<()> {
        NotificationSender::new(self)
            .send_now(recipients, notification, channels)
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
        Box::pin(self.send_now(recipients, notification, channels))
    }
}
