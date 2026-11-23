use crate::mail::{MailResult, mailables::AttachmentSource};

/// An object that can describe its own mail attachment.
pub trait Attachable {
    fn to_mail_attachment(&self) -> MailResult<AttachmentSource>;
}
