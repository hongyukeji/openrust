//! Notification channel selection and delivery.

mod channel_manager;
pub mod channels;
mod database_notification;
pub mod messages;
mod notifiable;
mod notification;
mod notification_sender;
mod notification_service_provider;
mod send_queued_notifications;

use std::{error::Error, future::Future, pin::Pin};

pub use channel_manager::ChannelManager;
pub use database_notification::DatabaseNotification;
pub use notifiable::Notifiable;
pub use notification::Notification;
pub use notification_sender::NotificationSender;
pub use notification_service_provider::NotificationServiceProvider;
pub use send_queued_notifications::SendQueuedNotifications;

/// An asynchronous notification operation.
pub type NotificationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a notification operation.
pub type NotificationResult<T> = Result<T, NotificationError>;

/// A channel, payload, persistence, or transport failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum NotificationError {
    UnknownChannel {
        name: String,
    },
    Delivery {
        source: Box<dyn Error + Send + Sync>,
    },
    Mail(crate::mail::MailError),
    Queue(crate::queue::QueueError),
    Database(crate::database::DatabaseError),
}

/// The address or model relation for a notification channel.
pub struct NotificationRoute {
    _private: (),
}
