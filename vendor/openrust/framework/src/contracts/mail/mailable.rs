use crate::mail::{
    Mail, MailFuture, MailResult,
    mailables::{Attachment, Content, Envelope, Headers},
};

/// Describes an application email for a selected mailer.
pub trait Mailable: Send + Sync {
    fn kind(&self) -> &str {
        std::any::type_name::<Self>()
    }

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

    /// Renders the message body in the current application without sending it.
    fn render(&self) -> MailFuture<'_, MailResult<String>>
    where
        Self: Sized,
    {
        Box::pin(Mail::render(self))
    }

    /// Renders an HTTP response for a preview route.
    #[allow(non_snake_case, reason = "Matches Laravel's response API")]
    fn toResponse(
        &self,
    ) -> crate::http::HttpFuture<'_, Result<crate::http::Response, crate::http::HttpError>>
    where
        Self: Sized,
    {
        Box::pin(Mail::preview(self))
    }

    /// Returns the queue options when sending this mailable should enqueue it.
    fn queue_options(&self) -> Option<crate::queue::DispatchOptions> {
        None
    }
}
