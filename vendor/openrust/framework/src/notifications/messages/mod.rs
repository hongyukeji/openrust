//! Data handed to built-in notification channels.

use crate::contracts::mail::Mailable;

/// The message or custom mailable produced for the mail channel.
pub enum MailDelivery {
    Message(MailMessage),
    Mailable(Box<dyn Mailable>),
}

/// The content and addressing options of a mail notification.
pub struct MailMessage {
    _private: (),
}

/// The structured payload of a database notification.
pub struct DatabaseMessage {
    _private: (),
}
