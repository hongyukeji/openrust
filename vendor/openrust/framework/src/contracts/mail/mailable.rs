use crate::mail::{
    Mail, MailFuture, MailResult,
    mailables::{Attachment, AttachmentSource, Content, Envelope, Headers, InlineAttachment},
};

use super::assertions::{Rendered, Rule, address_assertions, assert_body};

/// Describes an application email for a selected mailer.
pub trait Mailable: Send + Sync {
    fn kind(&self) -> &str {
        std::any::type_name::<Self>()
    }

    fn envelope(&self) -> MailResult<Envelope>;
    fn content(&self) -> MailResult<Content>;

    address_assertions!(hasFrom, assertFrom, from);
    address_assertions!(hasTo, assertTo, to);
    address_assertions!(hasCc, assertHasCc, cc);
    address_assertions!(hasBcc, assertHasBcc, bcc);
    address_assertions!(hasReplyTo, assertHasReplyTo, reply_to);

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_to(&self, address: &str, name: Option<&str>) -> &Self
    where
        Self: Sized,
    {
        self.assertTo(address, name)
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn has_subject(&self, subject: &str) -> MailResult<bool> {
        Ok(self.envelope()?.subject == subject)
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_subject(&self, subject: &str) -> &Self
    where
        Self: Sized,
    {
        assert!(
            self.has_subject(subject)
                .expect("could not inspect mailable subject"),
            "Did not find the expected mail subject [{subject}]."
        );
        self
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn assert_see_in_html<'a>(&'a self, value: &'a str, escape: bool) -> MailFuture<'a, &'a Self>
    where
        Self: Sized,
    {
        Box::pin(assert_body(
            self,
            Rendered::Html { escape },
            Rule::Contains(value),
        ))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn assert_dont_see_in_html<'a>(
        &'a self,
        value: &'a str,
        escape: bool,
    ) -> MailFuture<'a, &'a Self>
    where
        Self: Sized,
    {
        Box::pin(assert_body(
            self,
            Rendered::Html { escape },
            Rule::DoesNotContain(value),
        ))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn assert_see_in_order_in_html<'a>(
        &'a self,
        values: &'a [&'a str],
        escape: bool,
    ) -> MailFuture<'a, &'a Self>
    where
        Self: Sized,
    {
        Box::pin(assert_body(
            self,
            Rendered::Html { escape },
            Rule::InOrder(values),
        ))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn assert_see_in_text<'a>(&'a self, value: &'a str) -> MailFuture<'a, &'a Self>
    where
        Self: Sized,
    {
        Box::pin(assert_body(self, Rendered::Text, Rule::Contains(value)))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn assert_dont_see_in_text<'a>(&'a self, value: &'a str) -> MailFuture<'a, &'a Self>
    where
        Self: Sized,
    {
        Box::pin(assert_body(
            self,
            Rendered::Text,
            Rule::DoesNotContain(value),
        ))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn assert_see_in_order_in_text<'a>(&'a self, values: &'a [&'a str]) -> MailFuture<'a, &'a Self>
    where
        Self: Sized,
    {
        Box::pin(assert_body(self, Rendered::Text, Rule::InOrder(values)))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn has_tag(&self, tag: &str) -> MailResult<bool> {
        Ok(self.envelope()?.tags.iter().any(|value| value == tag))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn has_metadata(&self, name: &str, value: &str) -> MailResult<bool> {
        Ok(self
            .envelope()?
            .metadata
            .get(name)
            .is_some_and(|metadata| metadata == value))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_tag(&self, tag: &str) -> &Self
    where
        Self: Sized,
    {
        assert!(
            self.has_tag(tag).expect("could not inspect mailable tags"),
            "Did not find the expected mail tag [{tag}]."
        );
        self
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_metadata(&self, name: &str, value: &str) -> &Self
    where
        Self: Sized,
    {
        assert!(
            self.has_metadata(name, value)
                .expect("could not inspect mailable metadata"),
            "Did not find the expected mail metadata [{name}]."
        );
        self
    }

    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        Ok(Vec::new())
    }
    /// Describes attachments whose files or storage objects are resolved at delivery.
    fn attachment_sources(&self) -> MailResult<Vec<AttachmentSource>> {
        Ok(Vec::new())
    }

    /// Checks the declared source and options without reading attachment files.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn has_attachment(&self, attachment: &AttachmentSource) -> MailResult<bool> {
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

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_attachment(&self, attachment: &AttachmentSource) -> &Self
    where
        Self: Sized,
    {
        assert!(
            self.has_attachment(attachment)
                .expect("could not inspect mailable attachments"),
            "Did not find the expected attachment."
        );
        self
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_no_attachments(&self) -> &Self
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

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_attached_data(&self, data: &[u8], name: &str, mime: Option<&str>) -> &Self
    where
        Self: Sized,
    {
        self.assert_has_attachment(&Attachment::from_data(data, name).with_metadata(None, mime))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_attachment_from_storage(
        &self,
        path: &str,
        name: Option<&str>,
        mime: Option<&str>,
    ) -> &Self
    where
        Self: Sized,
    {
        self.assert_has_attachment(&Attachment::from_storage(path).with_metadata(name, mime))
    }

    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    #[track_caller]
    fn assert_has_attachment_from_storage_disk(
        &self,
        disk: &str,
        path: &str,
        name: Option<&str>,
        mime: Option<&str>,
    ) -> &Self
    where
        Self: Sized,
    {
        self.assert_has_attachment(
            &Attachment::from_storage_disk(disk, path).with_metadata(name, mime),
        )
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
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn to_response(
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
