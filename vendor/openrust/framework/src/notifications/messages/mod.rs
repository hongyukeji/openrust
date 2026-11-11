//! Data handed to built-in notification channels.

use crate::mail::{
    MailResult, Mailable,
    mailables::{Attachment, Content, Envelope, Headers},
};
use chrono::{DateTime, Utc};

/// The message or custom mailable produced for the mail channel.
pub enum MailDelivery {
    Message(Box<MailMessage>),
    Mailable(Box<dyn Mailable>),
}

/// The content and addressing options of a mail notification.
#[derive(Default)]
pub struct MailMessage {
    pub envelope: Envelope,
    pub content: Content,
    pub attachments: Vec<Attachment>,
    pub headers: Headers,
    pub mailer: Option<String>,
}

impl Mailable for MailMessage {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(self.envelope.clone())
    }
    fn content(&self) -> MailResult<Content> {
        Ok(self.content.clone())
    }
    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        Ok(self.attachments.clone())
    }
    fn headers(&self) -> MailResult<Headers> {
        Ok(self.headers.clone())
    }
    fn mailer(&self) -> Option<&str> {
        self.mailer.as_deref()
    }
}

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
