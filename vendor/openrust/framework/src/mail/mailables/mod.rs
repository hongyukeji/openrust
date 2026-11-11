//! Structured definitions used when composing a mailable.

use serde::{Deserialize, Serialize};

/// A mailbox and its optional display name.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Address {
    pub address: String,
    pub name: Option<String>,
}

impl From<&str> for Address {
    fn from(address: &str) -> Self {
        Self {
            address: address.into(),
            name: None,
        }
    }
}

/// The subject, sender, and recipient metadata of a mailable.
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Envelope {
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub subject: String,
}

/// The text, HTML, or named views for a mailable.
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Content {
    pub text: Option<String>,
    pub html: Option<String>,
    pub text_view: Option<String>,
    pub view: Option<String>,
    pub with: crate::view::ViewData,
}

/// An attachment with an explicit file name, MIME type, and contents.
#[derive(Clone, Deserialize, Serialize)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// The message identifier, references, and custom headers of a mailable.
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Headers {
    pub message_id: Option<String>,
    pub references: Vec<String>,
    pub text: std::collections::BTreeMap<String, String>,
}
