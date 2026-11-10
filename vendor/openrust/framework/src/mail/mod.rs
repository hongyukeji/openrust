//! Mail composition, mailer selection, and delivery.

mod mail_manager;
mod mail_service_provider;
mod mailable;
pub mod mailables;
mod mailer;
mod pending_mail;
mod send_queued_mailable;
pub mod transport;

use std::{error::Error, future::Future, pin::Pin};

pub use mail_manager::MailManager;
pub use mail_service_provider::MailServiceProvider;
pub use mailable::Mailable;
pub use mailer::Mailer;
pub use pending_mail::PendingMail;
pub use send_queued_mailable::SendQueuedMailable;

/// An asynchronous mail operation.
pub type MailFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a mail operation.
pub type MailResult<T> = Result<T, MailError>;

/// A configuration, composition, queue, or transport failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum MailError {
    UnknownMailer {
        name: String,
    },
    Composition {
        source: Box<dyn Error + Send + Sync>,
    },
    Transport {
        source: Box<dyn Error + Send + Sync>,
    },
    Queue(crate::queue::QueueError),
}

/// A composed email ready for transport.
pub struct Message {
    _private: (),
}

/// The result of a message accepted by the selected transport.
pub struct SentMessage {
    _private: (),
}
