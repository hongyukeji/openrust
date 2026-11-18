//! Structured definitions used when composing a mailable.

use serde::{Deserialize, Serialize};

mod attachment;
pub use attachment::AttachmentSource;

/// A mailbox and its optional display name.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub metadata: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub using: Vec<crate::mail::MessageCustomization>,
}

/// The text, HTML, or named views for a mailable.
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Content {
    pub text: Option<String>,
    pub html: Option<String>,
    pub text_view: Option<String>,
    pub view: Option<String>,
    #[serde(default)]
    pub markdown: Option<String>,
    /// A theme override for this Markdown message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub with: crate::view::ViewData,
}

/// An attachment with an explicit file name, MIME type, and contents.
#[derive(Clone, Deserialize, Serialize)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// An attachment addressed by a Content-ID in the HTML body.
#[derive(Clone, Deserialize, Serialize)]
pub struct InlineAttachment {
    pub content_id: String,
    pub attachment: Attachment,
}

impl InlineAttachment {
    pub fn new(content_id: impl Into<String>, attachment: Attachment) -> Self {
        Self {
            content_id: content_id.into(),
            attachment,
        }
    }

    pub fn cid(&self) -> String {
        format!("cid:{}", self.content_id)
    }
}

/// The message identifier, references, and custom headers of a mailable.
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Headers {
    pub message_id: Option<String>,
    pub references: Vec<String>,
    pub text: std::collections::BTreeMap<String, String>,
}
