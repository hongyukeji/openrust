use crate::{
    log::{LogContext, Logger},
    mail::{MailFuture, MailResult, Message, SentMessage, transport},
};
use std::sync::Arc;

use super::Transport;

/// Writes composed messages to an application log channel.
pub struct LogTransport(pub(crate) Arc<Logger>);

impl Transport for LogTransport {
    fn send<'a>(&'a self, message: &'a Message) -> MailFuture<'a, MailResult<SentMessage>> {
        Box::pin(async move {
            self.0
                .debug(
                    &String::from_utf8_lossy(&message.formatted()),
                    &LogContext::new(),
                )
                .map_err(transport)?;
            Ok(SentMessage {
                message: message.clone(),
            })
        })
    }
}
