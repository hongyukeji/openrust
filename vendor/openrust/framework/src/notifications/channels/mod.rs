//! Notification delivery channels.

mod database_channel;
mod mail_channel;

use super::{Notifiable, Notification, NotificationFuture, NotificationResult};

pub use database_channel::DatabaseChannel;
pub use mail_channel::MailChannel;

/// The result of a channel delivery, available to notification events.
pub struct ChannelResponse {
    _private: (),
}

/// Delivers a notification through one channel.
pub trait Channel: Send + Sync {
    fn send<'a>(
        &'a self,
        recipient: &'a dyn Notifiable,
        notification: &'a dyn Notification,
    ) -> NotificationFuture<'a, NotificationResult<Option<ChannelResponse>>>;
}
