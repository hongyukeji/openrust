//! Mail transport contracts and adapters.

mod log_transport;
mod smtp_transport;

use super::{MailFuture, MailResult, Message, SentMessage};

pub use log_transport::LogTransport;
pub use smtp_transport::SmtpTransport;

/// Submits a composed message to its destination.
pub trait Transport: Send + Sync {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>>;
}
