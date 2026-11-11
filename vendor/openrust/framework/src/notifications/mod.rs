//! Notification channel selection and delivery.

mod channel_manager;
pub mod channels;
pub(crate) mod console;
mod database_notification;
mod facade;
pub mod messages;
mod notifiable;
mod notification;
mod notification_sender;
mod notification_service_provider;
mod send_queued_notifications;

use std::{error::Error, future::Future, pin::Pin};

pub use channel_manager::ChannelManager;
pub use database_notification::{DatabaseKey, DatabaseNotification, DatabaseRoute};
pub use facade::NotificationFacade;
pub use notifiable::Notifiable;
pub use notification::Notification;
pub use notification_sender::{NotificationEvent, NotificationSender};
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
pub enum NotificationRoute {
    Mail(Vec<crate::mail::mailables::Address>),
    Database(DatabaseRoute),
}

impl std::fmt::Display for NotificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownChannel { name } => {
                write!(f, "Notification channel [{name}] is not defined.")
            }
            Self::Delivery { source } => write!(f, "Notification delivery failed: {source}"),
            Self::Mail(source) => write!(f, "{source}"),
            Self::Queue(source) => write!(f, "{source}"),
            Self::Database(source) => write!(f, "{source}"),
        }
    }
}

impl Error for NotificationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::UnknownChannel { .. } => None,
            Self::Delivery { source } => Some(source.as_ref()),
            Self::Mail(source) => Some(source),
            Self::Queue(source) => Some(source),
            Self::Database(source) => Some(source),
        }
    }
}

fn delivery(source: impl Into<Box<dyn Error + Send + Sync>>) -> NotificationError {
    NotificationError::Delivery {
        source: source.into(),
    }
}

fn new_id() -> NotificationResult<String> {
    let mut random = [0; 16];
    aws_lc_rs::rand::fill(&mut random)
        .map_err(|_| delivery("Could not generate a notification identifier."))?;
    Ok(uuid::Builder::from_random_bytes(random)
        .into_uuid()
        .to_string())
}
