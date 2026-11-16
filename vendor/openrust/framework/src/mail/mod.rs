//! Mail composition, mailer selection, and delivery.

pub mod events;
mod facade;
mod mail_fake;
mod mail_manager;
mod mail_service_provider;
mod mailable;
pub mod mailables;
mod mailer;
mod markdown;
mod message;
mod pending_mail;
mod send_queued_mailable;
pub mod transport;

use std::{error::Error, future::Future, pin::Pin};

pub use facade::Mail;
pub use mail_fake::{MailFake, RecordedMail};
pub use mail_manager::MailManager;
pub use mail_service_provider::MailServiceProvider;
pub use mailable::Mailable;
pub use mailer::Mailer;
pub use markdown::Markdown;
pub use message::{Message, SentMessage};
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
    Cancelled,
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

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Mail delivery was cancelled by a listener."),
            Self::UnknownMailer { name } => write!(f, "Mailer [{name}] is not defined."),
            Self::Composition { source } => write!(f, "Mail composition failed: {source}"),
            Self::Transport { source } => write!(f, "Mail transport failed: {source}"),
            Self::Queue(source) => write!(f, "Mail queue failed: {source}"),
        }
    }
}

impl Error for MailError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cancelled | Self::UnknownMailer { .. } => None,
            Self::Composition { source } | Self::Transport { source } => Some(source.as_ref()),
            Self::Queue(source) => Some(source),
        }
    }
}

fn composition(source: impl Into<Box<dyn Error + Send + Sync>>) -> MailError {
    MailError::Composition {
        source: source.into(),
    }
}

fn transport(source: impl Into<Box<dyn Error + Send + Sync>>) -> MailError {
    MailError::Transport {
        source: source.into(),
    }
}

pub(crate) use pending_mail::Recipients;
pub(crate) use send_queued_mailable::PreparedMail;
