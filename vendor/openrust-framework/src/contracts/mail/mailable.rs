use crate::mail::{
    Mail, MailFuture, MailResult,
    mailables::{Attachment, AttachmentSource, Content, Envelope, Headers, InlineAttachment},
};

use super::assertions::{Rendered, Rule, address_assertions, assert_body};

/// Describes an application email for a selected mailer.
pub trait Mailable: Send + Sync {
    /// 返回邮件的具体类型，用于队列观测。
    fn queue_subject_type(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    fn kind(&self) -> &str {
        std::any::type_name::<Self>()
    }

    fn envelope(&self) -> MailResult<Envelope>;
    fn content(&self) -> MailResult<Content>;

    address_assertions!(has_from, assert_from, from);
    address_assertions!(has_to, assert_to, to);
    address_assertions!(has_cc, assert_has_cc, cc);
    address_assertions!(has_bcc, assert_has_bcc, bcc);
    address_assertions!(has_reply_to, assert_has_reply_to, reply_to);

    #[track_caller]
    fn assert_has_to(&self, address: &str, name: Option<&str>) -> &Self
    where
        Self: Sized,
    {
        self.assert_to(address, name)
    }

    fn has_subject(&self, subject: &str) -> MailResult<bool> {
        Ok(self.envelope()?.subject == subject)
    }

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

    fn assert_see_in_text<'a>(&'a self, value: &'a str) -> MailFuture<'a, &'a Self>
    where
        Self: Sized,
    {
        Box::pin(assert_body(self, Rendered::Text, Rule::Contains(value)))
    }

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

    fn assert_see_in_order_in_text<'a>(&'a self, values: &'a [&'a str]) -> MailFuture<'a, &'a Self>
    where
        Self: Sized,
    {
        Box::pin(assert_body(self, Rendered::Text, Rule::InOrder(values)))
    }

    fn has_tag(&self, tag: &str) -> MailResult<bool> {
        Ok(self.envelope()?.tags.iter().any(|value| value == tag))
    }

    fn has_metadata(&self, name: &str, value: &str) -> MailResult<bool> {
        Ok(self
            .envelope()?
            .metadata
            .get(name)
            .is_some_and(|metadata| metadata == value))
    }

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

    #[track_caller]
    fn assert_has_attached_data(&self, data: &[u8], name: &str, mime: Option<&str>) -> &Self
    where
        Self: Sized,
    {
        self.assert_has_attachment(&Attachment::from_data(data, name).with_metadata(None, mime))
    }

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
