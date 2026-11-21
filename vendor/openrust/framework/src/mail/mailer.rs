use std::sync::Arc;

use crate::{
    contracts::{
        events::Dispatcher as _,
        mail::{MailQueue, Mailer as Contract},
    },
    database::Transaction,
    events::{Dispatcher, EventContext, ListenerResponse},
    queue::{Delay, DispatchOptions},
    support::facades::Lang,
    view,
};

use super::{
    MailError, MailFuture, MailResult, Mailable, Message, MessageDraft, SendQueuedMailable,
    SentMessage,
    mailables::Address,
    message_customization::MessageCustomizers,
    pending_mail::Recipients,
    transport::{ArrayTransport, Transport},
};

/// Builds messages and dispatches them through the selected transport.
pub struct Mailer {
    pub(crate) name: String,
    pub(super) from: Option<Address>,
    pub(super) views: Arc<view::Factory>,
    pub(super) markdown: super::Markdown,
    pub(super) events: Arc<Dispatcher>,
    pub(super) transport: Arc<dyn Transport>,
    pub(super) captured: Option<Arc<ArrayTransport>>,
    pub(super) fake: super::mail_fake::FakeState,
    pub(super) customizers: Arc<MessageCustomizers>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Mailer API")]
impl Mailer {
    pub fn sent(&self) -> Option<Vec<SentMessage>> {
        self.captured.as_ref().map(|transport| transport.messages())
    }

    pub async fn send(&self, mailable: &dyn Mailable) -> MailResult<Option<SentMessage>> {
        self.deliver(mailable, Recipients::default()).await
    }

    pub async fn sendNow(&self, mailable: &dyn Mailable) -> MailResult<SentMessage> {
        self.send_now_to(mailable, Recipients::default()).await
    }

    pub async fn queue(
        &self,
        mailable: &dyn Mailable,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> MailResult<Option<String>> {
        self.enqueue(mailable, Recipients::default(), options, transaction)
            .await
    }

    pub(super) async fn deliver(
        &self,
        mailable: &dyn Mailable,
        recipients: Recipients,
    ) -> MailResult<Option<SentMessage>> {
        if let Some(options) = mailable.queue_options() {
            self.enqueue(mailable, recipients, options, None).await?;
            return Ok(None);
        }
        self.send_immediately(mailable, recipients).await
    }

    pub(super) async fn send_now_to(
        &self,
        mailable: &dyn Mailable,
        recipients: Recipients,
    ) -> MailResult<SentMessage> {
        self.send_immediately(mailable, recipients)
            .await?
            .ok_or(MailError::Cancelled)
    }

    pub(super) async fn send_immediately(
        &self,
        mailable: &dyn Mailable,
        recipients: Recipients,
    ) -> MailResult<Option<SentMessage>> {
        self.send_with_context(mailable, recipients, mailable.kind(), false)
            .await
    }

    pub(super) async fn send_with_context(
        &self,
        mailable: &dyn Mailable,
        recipients: Recipients,
        kind: &str,
        queued: bool,
    ) -> MailResult<Option<SentMessage>> {
        let locale = recipients.locale.as_deref().or_else(|| mailable.locale());
        let message = match locale {
            Some(locale) => Lang::withLocale(locale, self.compose(mailable, &recipients))
                .await
                .map_err(super::composition)??,
            None => self.compose(mailable, &recipients).await?,
        };
        let fake = self.fake.read().expect("mail fake lock poisoned").clone();
        if let Some(fake) = fake {
            let sent = SentMessage { message };
            fake.record(super::RecordedMail {
                kind: mailable.kind().into(),
                mailer: self.name.clone(),
                envelope: sent.message.1.envelope.clone(),
                locale: locale.map(str::to_owned).or_else(|| Lang::getLocale().ok()),
                message: Some(sent.clone()),
                queue: None,
            });
            return Ok(Some(sent));
        }
        let before = self
            .events
            .until(
                Arc::new(super::events::MessageSending {
                    mailer: self.name.clone(),
                    message: message.clone(),
                }),
                &mut EventContext::new(),
            )
            .await
            .map_err(super::composition)?;
        if matches!(before, Some(ListenerResponse::Stop))
            || matches!(before, Some(ListenerResponse::Value(value)) if value.downcast_ref::<bool>() == Some(&false))
        {
            return Ok(None);
        }
        let sent = self.transport.send(&message).await?;
        self.events
            .dispatch(
                Arc::new(super::events::MessageSent {
                    mailer: self.name.clone(),
                    sent: sent.clone(),
                    mailable: kind.into(),
                    queued,
                }),
                &mut EventContext::new(),
            )
            .await
            .map_err(super::composition)?;
        Ok(Some(sent))
    }

    async fn compose(
        &self,
        mailable: &dyn Mailable,
        recipients: &Recipients,
    ) -> MailResult<Message> {
        let mut envelope = mailable.envelope()?;
        recipients.fill(&mut envelope);
        if envelope.from.is_none() {
            envelope.from.clone_from(&self.from);
        }
        let mut draft = MessageDraft::render(
            envelope,
            mailable.headers()?,
            mailable.content()?,
            &self.views,
            &self.markdown,
        )?;
        self.customizers.apply(&mut draft)?;
        // Storage and URL resolution can hold large transport futures.
        for source in std::mem::take(&mut draft.attachment_sources) {
            draft.attachments.push(Box::pin(source.resolve()).await?);
        }
        draft.attachments.extend(mailable.attachments()?);
        for source in mailable.attachment_sources()? {
            draft.attachments.push(Box::pin(source.resolve()).await?);
        }
        draft
            .inline_attachments
            .extend(mailable.inline_attachments()?);
        Message::compose(draft)
    }

    pub(super) async fn enqueue(
        &self,
        mailable: &dyn Mailable,
        recipients: Recipients,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> MailResult<Option<String>> {
        let fake = self.fake.read().expect("mail fake lock poisoned").clone();
        if let Some(fake) = fake {
            let prepared = super::PreparedMail::prepare(&self.name, mailable, recipients).await?;
            fake.record(super::RecordedMail {
                kind: mailable.kind().into(),
                mailer: self.name.clone(),
                envelope: prepared.envelope,
                locale: prepared.locale,
                message: None,
                queue: Some(options),
            });
            return Ok(None);
        }
        SendQueuedMailable::dispatch(&self.name, mailable, recipients, options, transaction).await
    }
}

impl Contract for Mailer {
    fn send<'a>(
        &'a self,
        mailable: &'a dyn Mailable,
    ) -> MailFuture<'a, MailResult<Option<SentMessage>>> {
        Box::pin(self.send(mailable))
    }

    fn send_now<'a>(
        &'a self,
        mailable: &'a dyn Mailable,
    ) -> MailFuture<'a, MailResult<Option<SentMessage>>> {
        Box::pin(self.send_immediately(mailable, Recipients::default()))
    }
}

impl MailQueue for Mailer {
    fn queue<'a>(
        &'a self,
        mailable: &'a dyn Mailable,
        queue: Option<&'a str>,
    ) -> MailFuture<'a, MailResult<Option<String>>> {
        Box::pin(self.enqueue(
            mailable,
            Recipients::default(),
            queue_options(mailable, queue, None),
            None,
        ))
    }

    fn later<'a>(
        &'a self,
        delay: Delay,
        mailable: &'a dyn Mailable,
        queue: Option<&'a str>,
    ) -> MailFuture<'a, MailResult<Option<String>>> {
        Box::pin(self.enqueue(
            mailable,
            Recipients::default(),
            queue_options(mailable, queue, Some(delay)),
            None,
        ))
    }
}

fn queue_options(
    mailable: &dyn Mailable,
    queue: Option<&str>,
    delay: Option<Delay>,
) -> DispatchOptions {
    let mut options = mailable.queue_options().unwrap_or_default();
    if let Some(queue) = queue {
        options.queue = Some(queue.into());
    }
    if delay.is_some() {
        options.delay = delay;
    }
    options
}
