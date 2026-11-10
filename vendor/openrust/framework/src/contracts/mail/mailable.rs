use crate::mail::{
    MailResult,
    mailables::{Attachment, Content, Envelope, Headers},
};

/// Describes an application email for a selected mailer.
pub trait Mailable: Send + Sync {
    fn envelope(&self) -> MailResult<Envelope>;
    fn content(&self) -> MailResult<Content>;
    fn attachments(&self) -> MailResult<Vec<Attachment>>;
    fn headers(&self) -> MailResult<Headers>;
    fn locale(&self) -> Option<&str>;
    fn mailer(&self) -> Option<&str>;
}
