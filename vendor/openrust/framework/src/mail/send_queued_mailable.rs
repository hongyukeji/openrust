use serde::{Deserialize, Serialize};

use crate::{
    contracts::queue::JobHandler,
    database::Transaction,
    queue::{
        DispatchOptions, DispatchResult, JobContext, Payload, QueueError, QueueFuture, QueueResult,
    },
    support::facades::{Lang, Queue},
};

use super::{
    Mail, MailError, MailResult, Mailable, SentMessage,
    mailables::{Attachment, Content, Envelope, Headers},
    pending_mail::Recipients,
};

/// The queue handler for a composed mailable definition.
pub struct SendQueuedMailable;

#[derive(Deserialize, Serialize)]
pub(crate) struct PreparedMail {
    mailer: String,
    envelope: Envelope,
    content: Content,
    attachments: Vec<Attachment>,
    headers: Headers,
    locale: Option<String>,
}

impl PreparedMail {
    pub(crate) async fn prepare(
        mailer: &str,
        mailable: &dyn Mailable,
        recipients: Recipients,
    ) -> MailResult<Self> {
        let locale = recipients.locale.as_deref().or_else(|| mailable.locale());
        let prepare = || {
            let mut envelope = mailable.envelope()?;
            recipients.fill(&mut envelope);
            Ok(Self {
                mailer: mailer.into(),
                envelope,
                content: mailable.content()?,
                attachments: mailable.attachments()?,
                headers: mailable.headers()?,
                locale: locale.map(str::to_owned).or_else(|| Lang::getLocale().ok()),
            })
        };
        match locale {
            Some(locale) => Lang::withLocale(locale, async { prepare() })
                .await
                .map_err(super::composition)?,
            None => prepare(),
        }
    }

    pub(crate) async fn send(&self) -> MailResult<SentMessage> {
        Mail::mailer(Some(&self.mailer))?.sendNow(self).await
    }
}

impl Mailable for PreparedMail {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(self.envelope.clone())
    }
    fn content(&self) -> MailResult<Content> {
        Ok(self.content.clone())
    }
    fn attachments(&self) -> MailResult<Vec<Attachment>> {
        Ok(self.attachments.clone())
    }
    fn headers(&self) -> MailResult<Headers> {
        Ok(self.headers.clone())
    }
    fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }
}

impl SendQueuedMailable {
    pub(crate) const NAME: &str = "mail.send_queued_mailable";

    pub(super) async fn dispatch(
        mailer: &str,
        mailable: &dyn Mailable,
        recipients: Recipients,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> MailResult<Option<String>> {
        let message = PreparedMail::prepare(mailer, mailable, recipients).await?;
        let payload = Payload::new(Self::NAME, message).map_err(MailError::Queue)?;
        match Queue::dispatch(payload, options, transaction)
            .await
            .map_err(MailError::Queue)?
        {
            DispatchResult::Queued(id) => Ok(Some(id)),
            DispatchResult::Deferred | DispatchResult::Completed | DispatchResult::Duplicate => {
                Ok(None)
            }
        }
    }
}

impl JobHandler for SendQueuedMailable {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let message: PreparedMail = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            message
                .send()
                .await
                .map_err(|source| QueueError::Execution {
                    source: Box::new(source),
                })?;
            Ok(())
        })
    }
}
