use serde::{Deserialize, Serialize};

use crate::{
    broadcasting::{BroadcastChannel, BroadcastPayload},
    contracts::events::Event,
    database::Transaction,
    events::ListenerResponse,
    queue::DispatchOptions,
    support::facades::{Event as Events, Lang},
};

use super::{
    ChannelManager, Notifiable, Notification, NotificationQueueGuard, NotificationResult,
    NotificationRoute, NotificationTarget, RecordedNotification, SendQueuedNotifications,
    channels::ChannelResponse,
    delivery,
    messages::{BroadcastMessage, DatabaseMessage, MailDelivery},
    new_id,
    slack::SlackMessage,
};

/// The lifecycle event of a delivery to one channel.
pub struct NotificationEvent {
    name: &'static str,
    pub id: String,
    pub kind: String,
    pub channel: String,
    pub notifiable: String,
    pub queued: bool,
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
    #[serde(default)]
    pub(super) notifiable: String,
    #[serde(default)]
    pub(super) queued: bool,
    pub(super) locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) guard: Option<NotificationQueueGuard>,
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
            notifiable: self.notifiable.clone(),
            queued: self.queued,
            response,
            error,
        }
    }

    pub(super) async fn skipped(&self) -> NotificationResult<()> {
        Events::dispatch(self.event("NotificationSkipped", None, None))
            .await
            .map_err(delivery)?;
        Ok(())
    }

    pub(super) async fn send(
        &self,
        operation: impl Future<Output = NotificationResult<Option<ChannelResponse>>>,
    ) -> NotificationResult<()> {
        if self.can_send().await? {
            self.complete(operation.await).await
        } else {
            self.skipped().await
        }
    }

    pub(super) async fn can_send(&self) -> NotificationResult<bool> {
        let before = Events::until(self.event("NotificationSending", None, None))
            .await
            .map_err(delivery)?;
        let denied = match before {
            Some(ListenerResponse::Stop) => true,
            Some(ListenerResponse::Value(value)) => value.downcast_ref::<bool>() == Some(&false),
            None => false,
        };
        Ok(!denied)
    }

    pub(super) async fn complete(
        &self,
        result: NotificationResult<Option<ChannelResponse>>,
    ) -> NotificationResult<()> {
        match result {
            Ok(response) => {
                let sms = if let Some(ChannelResponse::Sms(submission)) = &response {
                    if submission.status != super::sms::SmsSubmissionStatus::Logged {
                        let context = crate::log::LogContext::from_iter([
                            ("notification_id".into(), serde_json::json!(self.id)),
                            ("submission".into(), serde_json::json!(submission)),
                        ]);
                        let _ = crate::log::Log::info(
                            "The SMS provider accepted the submission.",
                            &context,
                        );
                    }
                    true
                } else {
                    false
                };
                if let Err(error) =
                    Events::dispatch(self.event("NotificationSent", response, None)).await
                {
                    if !sms {
                        return Err(delivery(error));
                    }
                    let context = crate::log::LogContext::from_iter([(
                        "notification_id".into(),
                        serde_json::json!(self.id),
                    )]);
                    let _ = crate::log::Log::error("The SMS submission listener failed.", &context);
                }
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
            Some(locale) => Lang::with_locale(locale, operation)
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
            if let Some(fake) = self.manager.active_fake() {
                if let Some(record) = self.prepare_fake(*recipient, &prepared, channels)? {
                    fake.record(record);
                }
                continue;
            }
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

    pub async fn send_now(
        &self,
        recipients: &[&dyn Notifiable],
        notification: &dyn Notification,
        channels: Option<&[&str]>,
    ) -> NotificationResult<()> {
        for recipient in recipients {
            let channels = match channels {
                Some(channels) if !channels.is_empty() => channels
                    .iter()
                    .map(|channel| (*channel).to_owned())
                    .collect(),
                _ => notification.via(*recipient)?,
            };
            let prepared = Prepared::new(notification)?;
            if let Some(fake) = self.manager.active_fake() {
                if let Some(record) = self.prepare_fake(*recipient, &prepared, channels)? {
                    fake.record(record);
                }
                continue;
            }
            for channel in channels {
                self.deliver(*recipient, &prepared, &channel, None, None)
                    .await?;
            }
        }
        Ok(())
    }

    fn preferred_locale(
        &self,
        recipient: &dyn Notifiable,
        notification: &dyn Notification,
    ) -> NotificationResult<Option<String>> {
        notification
            .locale()
            .or(self.locale.as_deref())
            .or_else(|| recipient.preferred_locale())
            .map(str::to_owned)
            .or_else(|| Lang::get_locale().ok())
            .as_deref()
            .map(crate::translation::canonical_locale)
            .transpose()
            .map_err(delivery)
    }

    fn prepare_fake(
        &self,
        recipient: &dyn Notifiable,
        notification: &Prepared<'_>,
        mut channels: Vec<String>,
    ) -> NotificationResult<Option<RecordedNotification>> {
        let anonymous = recipient.as_anonymous();
        channels.retain(|channel| {
            !(anonymous.is_some() && channel == "database")
                && notification.inner.should_send(recipient, channel)
        });
        if channels.is_empty() {
            return Ok(None);
        }
        let target = if let Some(anonymous) = anonymous {
            NotificationTarget::OnDemand(anonymous.routes().clone())
        } else if let Some(key) = recipient.notification_key() {
            NotificationTarget::Key(key)
        } else if let Some(NotificationRoute::Database(route)) =
            recipient.route_notification_for("database", notification)?
        {
            NotificationTarget::Database(route)
        } else {
            NotificationTarget::Unkeyed
        };
        Ok(Some(RecordedNotification {
            id: notification.id.clone(),
            kind: notification.kind().into(),
            channels,
            locale: self.preferred_locale(recipient, notification)?,
            target,
            data: notification.inner.fake_data(),
        }))
    }

    async fn deliver(
        &self,
        recipient: &dyn Notifiable,
        notification: &Prepared<'_>,
        channel: &str,
        options: Option<DispatchOptions>,
        transaction: Option<&mut Transaction>,
    ) -> NotificationResult<()> {
        if channel == "database" && recipient.as_anonymous().is_some() {
            return Ok(());
        }
        let driver = self.manager.channel(Some(channel))?;
        let locale = self.preferred_locale(recipient, notification)?;
        let delivery = Delivery {
            id: notification.id.clone(),
            kind: notification.kind().into(),
            channel: channel.into(),
            notifiable: recipient.notification_tag(),
            queued: options.is_some(),
            locale,
            guard: if options.is_some() {
                notification.inner.queue_guard(recipient, channel)?
            } else {
                None
            },
        };
        delivery
            .localized(async {
                if delivery.guard.is_none() && !notification.inner.should_send(recipient, channel) {
                    delivery.skipped().await?;
                } else if let Some(options) = options {
                    if let Some(payload) = driver.queue_payload(recipient, notification).await? {
                        SendQueuedNotifications::dispatch(
                            &delivery,
                            payload,
                            options,
                            notification.inner.queue_job_options(recipient, channel),
                            notification.inner.queue_middleware(recipient, channel),
                            notification.inner.should_encrypt(recipient, channel),
                            transaction,
                        )
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
    fn to_sms(
        &self,
        recipient: &dyn Notifiable,
        to: String,
    ) -> NotificationResult<Option<super::sms::SmsMessage>> {
        self.inner.to_sms(recipient, to)
    }
    fn to_slack(&self, recipient: &dyn Notifiable) -> NotificationResult<Option<SlackMessage>> {
        self.inner.to_slack(recipient)
    }
    fn to_legacy_slack(
        &self,
        recipient: &dyn Notifiable,
    ) -> NotificationResult<Option<super::messages::SlackMessage>> {
        self.inner.to_legacy_slack(recipient)
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

    fn to_broadcast(
        &self,
        recipient: &dyn Notifiable,
    ) -> NotificationResult<Option<BroadcastMessage>> {
        self.inner.to_broadcast(recipient)
    }
    fn broadcast_on(&self) -> Vec<BroadcastChannel> {
        self.inner.broadcast_on()
    }
    fn broadcast_with(&self) -> Option<BroadcastPayload> {
        self.inner.broadcast_with()
    }
    fn broadcast_as(&self) -> Option<&str> {
        self.inner.broadcast_as()
    }
    fn broadcast_type(&self) -> &str {
        self.inner.broadcast_type()
    }
}
