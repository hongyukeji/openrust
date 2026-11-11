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

/// Captures sent mail in memory for the current mailer.
#[derive(Default)]
pub struct ArrayTransport(std::sync::Mutex<Vec<SentMessage>>);

impl ArrayTransport {
    pub fn messages(&self) -> Vec<SentMessage> {
        self.0
            .lock()
            .expect("array mail transport lock poisoned")
            .clone()
    }
}

impl Transport for ArrayTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
            let sent = SentMessage {
                message: message.clone(),
            };
            self.0
                .lock()
                .expect("array mail transport lock poisoned")
                .push(sent.clone());
            Ok(sent)
        })
    }
}
