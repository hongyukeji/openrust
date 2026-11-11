use std::sync::Arc;

use crate::{
    contracts::mail::{MailQueue, Mailer as Contract},
    database::Transaction,
    queue::{Delay, DispatchOptions},
    support::facades::Lang,
    view,
};

use super::{
    MailFuture, MailResult, Mailable, Message, SendQueuedMailable, SentMessage,
    mailables::Address,
    pending_mail::Recipients,
    transport::{ArrayTransport, Transport},
};

/// Builds messages and dispatches them through the selected transport.
pub struct Mailer {
    pub(crate) name: String,
    pub(super) from: Option<Address>,
    pub(super) views: Arc<view::Factory>,
    pub(super) transport: Arc<dyn Transport>,
    pub(super) captured: Option<Arc<ArrayTransport>>,
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
        self.send_now_to(mailable, recipients).await.map(Some)
    }

    pub(super) async fn send_now_to(
        &self,
        mailable: &dyn Mailable,
        recipients: Recipients,
    ) -> MailResult<SentMessage> {
        let locale = recipients.locale.as_deref().or_else(|| mailable.locale());
        let message = match locale {
            Some(locale) => Lang::withLocale(locale, async { self.compose(mailable, &recipients) })
                .await
                .map_err(super::composition)??,
            None => self.compose(mailable, &recipients)?,
        };
        self.transport.send(&message).await
    }

    fn compose(&self, mailable: &dyn Mailable, recipients: &Recipients) -> MailResult<Message> {
        let mut envelope = mailable.envelope()?;
        recipients.fill(&mut envelope);
        if envelope.from.is_none() {
            envelope.from.clone_from(&self.from);
        }
        Message::compose(
            envelope,
            mailable.content()?,
            mailable.attachments()?,
            mailable.headers()?,
            &self.views,
        )
    }

    pub(super) async fn enqueue(
        &self,
        mailable: &dyn Mailable,
        recipients: Recipients,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> MailResult<Option<String>> {
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
        Box::pin(async move { self.sendNow(mailable).await.map(Some) })
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
