use crate::contracts::events::Event;

use super::{Message, SentMessage};

/// Emitted before a composed message is handed to a mail transport.
pub struct MessageSending {
    pub mailer: String,
    pub message: Message,
}

impl Event for MessageSending {
    fn name(&self) -> &str {
        "MessageSending"
    }
}

/// Emitted when the selected transport accepts the message.
pub struct MessageSent {
    pub mailer: String,
    pub sent: SentMessage,
}

impl Event for MessageSent {
    fn name(&self) -> &str {
        "MessageSent"
    }
}
