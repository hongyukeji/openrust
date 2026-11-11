use crate::mail::{
    MailResult,
    mailables::{Attachment, Content, Envelope, Headers},
};

/// Describes an application email for a selected mailer.
pub trait Mailable: Send + Sync {
    fn envelope(&self) -> MailResult<Envelope>;
    fn content(&self) -> MailResult<Content>;
    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        Ok(Vec::new())
    }
    fn headers(&self) -> MailResult<Headers> {
        Ok(Headers::default())
    }
    fn locale(&self) -> Option<&str> {
        None
    }
    fn mailer(&self) -> Option<&str> {
        None
    }

    /// Returns the queue options when sending this mailable should enqueue it.
    fn queue_options(&self) -> Option<crate::queue::DispatchOptions> {
        None
    }
}
