use serde::{Deserialize, Serialize};

use crate::{
    contracts::events::Event,
    database::Transaction,
    events::ListenerResponse,
    queue::DispatchOptions,
    support::facades::{Event as Events, Lang},
};

use super::{
    ChannelManager, Notifiable, Notification, NotificationResult, SendQueuedNotifications,
    channels::ChannelResponse,
    delivery,
    messages::{DatabaseMessage, MailDelivery},
    new_id,
};

/// The lifecycle event of a delivery to one channel.
pub struct NotificationEvent {
    name: &'static str,
    pub id: String,
    pub kind: String,
    pub channel: String,
    pub response: Option<ChannelResponse>,
    pub error: Option<String>,
}

impl Event for NotificationEvent {
    fn name(&self) -> &str {
        self.name
    }
}

#[derive(Deserialize, Serialize)]
pub(super) struct Delivery {
    pub(super) id: String,
    pub(super) kind: String,
    pub(super) channel: String,
    pub(super) locale: Option<String>,
}

impl Delivery {
    fn event(
        &self,
        name: &'static str,
        response: Option<ChannelResponse>,
        error: Option<String>,
    ) -> NotificationEvent {
        NotificationEvent {
            name,
            id: self.id.clone(),
            kind: self.kind.clone(),
            channel: self.channel.clone(),
            response,
            error,
        }
    }

    pub(super) async fn send(
        &self,
        operation: impl Future<Output = NotificationResult<Option<ChannelResponse>>>,
    ) -> NotificationResult<()> {
        let before = Events::until(self.event("NotificationSending", None, None))
            .await
            .map_err(delivery)?;
        let denied = match before {
            Some(ListenerResponse::Stop) => true,
            Some(ListenerResponse::Value(value)) => value.downcast_ref::<bool>() == Some(&false),
            None => false,
        };
        if denied {
            Events::dispatch(self.event("NotificationSkipped", None, None))
                .await
                .map_err(delivery)?;
            return Ok(());
        }
        match operation.await {
            Ok(response) => {
                Events::dispatch(self.event("NotificationSent", response, None))
                    .await
                    .map_err(delivery)?;
                Ok(())
            }
            Err(error) => {
                let _ = Events::dispatch(self.event(
                    "NotificationFailed",
                    None,
                    Some(error.to_string()),
                ))
                .await;
                Err(error)
            }
        }
    }

    pub(super) async fn localized<T>(
        &self,
        operation: impl Future<Output = NotificationResult<T>>,
    ) -> NotificationResult<T> {
        match &self.locale {
            Some(locale) => Lang::withLocale(locale, operation)
                .await
                .map_err(delivery)?,
            None => operation.await,
        }
    }
}

/// Coordinates recipients, channels, locale, and delivery.
pub struct NotificationSender<'a> {
    manager: &'a ChannelManager,
    locale: Option<String>,
}

#[allow(non_snake_case, reason = "Matches Laravel's NotificationSender API")]
impl<'a> NotificationSender<'a> {
    pub fn new(manager: &'a ChannelManager) -> Self {
        Self {
            manager,
            locale: None,
        }
    }

    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.locale = Some(locale.into());
        self
    }

    pub async fn send(
        &self,
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
        mut transaction: Option<&mut Transaction>,
    ) -> NotificationResult<()> {
        for recipient in recipients {
            let channels = notification.via(*recipient)?;
            let prepared = Prepared::new(notification)?;
            for channel in channels {
                self.deliver(
                    *recipient,
                    &prepared,
                    &channel,
                    notification.queue_options(*recipient, &channel),
                    transaction.as_deref_mut(),
                )
                .await?;
            }
        }
        Ok(())
    }

    pub async fn sendNow(
        &self,
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
        channels: Option<&[&str]>,
    ) -> NotificationResult<()> {
        for recipient in recipients {
            let channels = match channels {
                Some(channels) => channels
                    .iter()
                    .map(|channel| (*channel).to_owned())
                    .collect(),
                None => notification.via(*recipient)?,
            };
            let prepared = Prepared::new(notification)?;
            for channel in channels {
                self.deliver(*recipient, &prepared, &channel, None, None)
                    .await?;
            }
        }
        Ok(())
    }

    async fn deliver(
        &self,
        recipient: &dyn Notifiable,
        notification: &Prepared<'_>,
        channel: &str,
        options: Option<DispatchOptions>,
        transaction: Option<&mut Transaction>,
    ) -> NotificationResult<()> {
        let driver = self.manager.channel(Some(channel))?;
        let locale = notification
            .locale()
            .or(self.locale.as_deref())
            .or_else(|| recipient.preferred_locale())
            .map(str::to_owned)
            .or_else(|| Lang::getLocale().ok());
        let delivery = Delivery {
            id: notification.id.clone(),
            kind: notification.kind().into(),
            channel: channel.into(),
            locale,
        };
        delivery
            .localized(async {
                if !notification.inner.should_send(recipient, channel) {
                    Events::dispatch(delivery.event("NotificationSkipped", None, None))
                        .await
                        .map_err(super::delivery)?;
                } else if let Some(options) = options {
                    if let Some(payload) = driver.queue_payload(recipient, notification).await? {
                        SendQueuedNotifications::dispatch(&delivery, payload, options, transaction)
                            .await?;
                    }
                } else {
                    delivery.send(driver.send(recipient, notification)).await?;
                }
                Ok(())
            })
            .await
    }
}

struct Prepared<'a> {
    id: String,
    inner: &'a dyn Notification,
}

impl<'a> Prepared<'a> {
    fn new(inner: &'a dyn Notification) -> NotificationResult<Self> {
        let id = match inner.id() {
            Some(id) => id.to_owned(),
            None => new_id()?,
        };
        Ok(Self { id, inner })
    }
}

impl Notification for Prepared<'_> {
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }
    fn kind(&self) -> &str {
        self.inner.kind()
    }
    fn locale(&self) -> Option<&str> {
        self.inner.locale()
    }
    fn via(&self, recipient: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        self.inner.via(recipient)
    }
    fn to_mail(&self, recipient: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        self.inner.to_mail(recipient)
    }
    fn to_database(
        &self,
        recipient: &dyn Notifiable,
    ) -> NotificationResult<Option<DatabaseMessage>> {
        self.inner.to_database(recipient)
    }
    fn to_array(&self, recipient: &dyn Notifiable) -> NotificationResult<Option<DatabaseMessage>> {
        self.inner.to_array(recipient)
    }
}
