//! Notification delivery channels.

mod broadcast_channel;
mod database_channel;
mod mail_channel;
mod slack_channel;
mod sms_channel;
mod vonage_sms_channel;

use super::{Notifiable, Notification, NotificationFuture, NotificationResult};

pub use broadcast_channel::{BroadcastChannel, BroadcastNotificationCreated};
pub use database_channel::DatabaseChannel;
pub use mail_channel::MailChannel;
pub use slack_channel::SlackChannel;
pub use sms_channel::SmsChannel;
pub(super) use vonage_sms_channel::VonageDriver;
pub use vonage_sms_channel::VonageSmsChannel;

/// The result of a channel delivery, available to notification events.
pub enum ChannelResponse {
    Mail(crate::mail::SentMessage),
    Database(super::DatabaseNotification),
    Custom(serde_json::Value),
}

/// Delivers a notification through one channel.
pub trait Channel: Send + Sync {
    fn send<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<ChannelResponse>>>;

    fn queue_payload<'a>(
        &'a self,
        _recipient: &'a dyn Notifiable,
        _notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<serde_json::Value>>> {
        Box::pin(async {
            Err(super::delivery(
                "This channel does not support queue delivery.",
            ))
        })
    }

    fn send_queued(
        &self,
        _payload: serde_json::Value,
    ) -> NotificationFuture<'_, NotificationResult<Option<ChannelResponse>>> {
        Box::pin(async {
            Err(super::delivery(
                "This channel does not support queue delivery.",
            ))
        })
    }
}

pub(super) fn secure_endpoint(endpoint: &str) -> Result<bool, url::ParseError> {
    let url = url::Url::parse(endpoint)?;
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name == "localhost",
        None => false,
    };
    Ok(
        (url.scheme() == "https" || (url.scheme() == "http" && loopback))
            && url.host().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
    )
}
