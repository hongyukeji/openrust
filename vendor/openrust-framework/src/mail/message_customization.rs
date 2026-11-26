use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    MailResult, composition,
    mailables::{Attachment, AttachmentSource, Envelope, Headers, InlineAttachment},
};

/// A named, serializable request to customize a message before sending.
#[derive(Clone, Deserialize, Serialize)]
pub struct MessageCustomization {
    pub name: String,
    pub data: Value,
}

impl MessageCustomization {
    pub fn new(name: impl Into<String>, data: Value) -> Self {
        Self {
            name: name.into(),
            data,
        }
    }
}

/// The shared message data used to build both MIME and provider requests.
pub struct MessageDraft {
    pub envelope: Envelope,
    pub headers: Headers,
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<Attachment>,
    pub attachment_sources: Vec<AttachmentSource>,
    pub inline_attachments: Vec<InlineAttachment>,
}

type Customizer = dyn Fn(&mut MessageDraft, &Value) -> MailResult<()> + Send + Sync;

#[derive(Default)]
pub(super) struct MessageCustomizers(Mutex<HashMap<String, Arc<Customizer>>>);

impl MessageCustomizers {
    pub(super) fn register(&self, name: String, callback: Arc<Customizer>) -> MailResult<()> {
        use std::collections::hash_map::Entry;
        match self
            .0
            .lock()
            .expect("mail customizer lock poisoned")
            .entry(name)
        {
            Entry::Occupied(entry) => Err(composition(format!(
                "Mail message customizer [{}] is already registered.",
                entry.key()
            ))),
            Entry::Vacant(entry) => {
                entry.insert(callback);
                Ok(())
            }
        }
    }

    pub(super) fn apply(&self, draft: &mut MessageDraft) -> MailResult<()> {
        for customization in draft.envelope.using.clone() {
            let callback = self
                .0
                .lock()
                .expect("mail customizer lock poisoned")
                .get(&customization.name)
                .cloned()
                .ok_or_else(|| {
                    composition(format!(
                        "Mail message customizer [{}] is not registered.",
                        customization.name
                    ))
                })?;
            callback(draft, &customization.data)?;
        }
        Ok(())
    }
}
