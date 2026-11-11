use std::sync::Arc;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{
    MailManager, MailResult, Mailable, Mailer, PendingMail, SentMessage, mailables::Address,
};

/// Resolves the mail manager for the current application.
pub struct Mail;

impl Facade for Mail {
    type Service = MailManager;

    fn getFacadeAccessor() -> ServiceKey {
        "mail.manager".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Mail facade")]
impl Mail {
    pub fn mailer(name: Option<&str>) -> MailResult<Arc<Mailer>> {
        Self::getFacadeRoot()
            .map_err(super::transport)?
            .mailer(name)
    }

    pub fn to(address: impl Into<Address>) -> MailResult<PendingMail> {
        Ok(Self::getFacadeRoot().map_err(super::transport)?.to(address))
    }

    pub async fn send(mailable: &dyn Mailable) -> MailResult<Option<SentMessage>> {
        Self::mailer(mailable.mailer())?.send(mailable).await
    }

    pub async fn sendNow(mailable: &dyn Mailable) -> MailResult<SentMessage> {
        Self::mailer(mailable.mailer())?.sendNow(mailable).await
    }
}
