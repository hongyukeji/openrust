//! Structured definitions used when composing a mailable.

/// A mailbox and its optional display name.
pub struct Address {
    pub address: String,
    pub name: Option<String>,
}

/// The subject, sender, and recipient metadata of a mailable.
pub struct Envelope {
    _private: (),
}

/// The text, HTML, or view content of a mailable.
pub struct Content {
    _private: (),
}

/// An attachment supplied by a mailable.
pub struct Attachment {
    _private: (),
}

/// The additional headers of a mailable.
pub struct Headers {
    _private: (),
}
