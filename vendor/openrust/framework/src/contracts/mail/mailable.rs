use crate::mail::{
    Mail, MailFuture, MailResult,
    mailables::{Attachment, AttachmentSource, Content, Envelope, Headers, InlineAttachment},
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
    /// Describes attachments whose files or storage objects are resolved at delivery.
    fn attachment_sources(&self) -> MailResult<Vec<AttachmentSource>> {
        Ok(Vec::new())
    }

    /// Checks the declared source and options without reading attachment files.
    #[allow(non_snake_case, reason = "Matches Laravel's mailable inspection API")]
    fn hasAttachment(&self, attachment: &AttachmentSource) -> MailResult<bool> {
        if self
            .attachment_sources()?
            .iter()
            .any(|item| attachment.matches(item))
        {
            return Ok(true);
        }
        Ok(self
            .attachments()?
            .iter()
            .any(|item| attachment.matches_materialized(item)))
    }

    #[allow(non_snake_case, reason = "Matches Laravel's mailable assertions")]
    #[track_caller]
    fn assertHasAttachment(&self, attachment: &AttachmentSource) -> &Self
    where
        Self: Sized,
    {
        assert!(
            self.hasAttachment(attachment)
                .expect("could not inspect mailable attachments"),
            "Did not find the expected attachment."
        );
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's mailable assertions")]
    #[track_caller]
    fn assertHasNoAttachments(&self) -> &Self
    where
        Self: Sized,
    {
        assert!(
            self.attachment_sources()
                .expect("could not inspect mailable attachment sources")
                .is_empty()
                && self
                    .attachments()
                    .expect("could not inspect mailable attachments")
                    .is_empty(),
            "Expected no attachments."
        );
        self
    }

    #[allow(non_snake_case, reason = "Matches Laravel's mailable assertions")]
    #[track_caller]
    fn assertHasAttachedData(&self, data: &[u8], name: &str, mime: Option<&str>) -> &Self
    where
        Self: Sized,
    {
        self.assertHasAttachment(&Attachment::fromData(data, name).with_metadata(None, mime))
    }

    #[allow(non_snake_case, reason = "Matches Laravel's mailable assertions")]
    #[track_caller]
    fn assertHasAttachmentFromStorage(
        &self,
        path: &str,
        name: Option<&str>,
        mime: Option<&str>,
    ) -> &Self
    where
        Self: Sized,
    {
        self.assertHasAttachment(&Attachment::fromStorage(path).with_metadata(name, mime))
    }

    #[allow(non_snake_case, reason = "Matches Laravel's mailable assertions")]
    #[track_caller]
    fn assertHasAttachmentFromStorageDisk(
        &self,
        disk: &str,
        path: &str,
        name: Option<&str>,
        mime: Option<&str>,
    ) -> &Self
    where
        Self: Sized,
    {
        self.assertHasAttachment(&Attachment::fromStorageDisk(disk, path).with_metadata(name, mime))
    }
    fn inline_attachments(&self) -> MailResult<Vec<InlineAttachment>> {
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
