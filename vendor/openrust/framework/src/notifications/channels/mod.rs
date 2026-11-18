//! Notification delivery channels.

mod broadcast_channel;
mod database_channel;
mod mail_channel;
mod vonage_sms_channel;

use super::{Notifiable, Notification, NotificationFuture, NotificationResult};

pub use broadcast_channel::{BroadcastChannel, BroadcastNotificationCreated};
pub use database_channel::DatabaseChannel;
pub use mail_channel::MailChannel;
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
