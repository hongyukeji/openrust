use crate::mail::{MailResult, mailables::AttachmentSource};

/// An object that can describe its own mail attachment.
pub trait Attachable {
    #[allow(non_snake_case, reason = "Matches Laravel's Attachable contract")]
    fn toMailAttachment(&self) -> MailResult<AttachmentSource>;
}
