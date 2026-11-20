//! Data handed to built-in notification channels.

mod mail_message;
mod slack_attachment;
mod slack_message;

pub use mail_message::MailMessage;
pub use slack_attachment::{SlackAttachment, SlackAttachmentField};
pub use slack_message::SlackMessage;

use crate::mail::Mailable;
use chrono::{DateTime, Utc};

/// The message or custom mailable produced for the mail channel.
pub enum MailDelivery {
    Message(Box<MailMessage>),
    Mailable(Box<dyn Mailable>),
}

pub(crate) use mail_message::{EMAIL_VIEW, VIEWS};

/// The structured payload of a database notification.
pub struct DatabaseMessage {
    pub data: serde_json::Value,
    pub kind: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
}

impl DatabaseMessage {
    pub fn new(data: serde_json::Value) -> Self {
        Self {
            data,
            kind: None,
            read_at: None,
        }
    }
}

/// Payload and queue selection for a broadcast notification.
pub struct BroadcastMessage {
    pub data: crate::broadcasting::BroadcastPayload,
    pub connection: Option<String>,
    pub queue: Option<String>,
}

impl BroadcastMessage {
    pub fn new(data: crate::broadcasting::BroadcastPayload) -> Self {
        Self {
            data,
            connection: None,
            queue: None,
        }
    }
}
