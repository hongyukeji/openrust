use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{database::Transaction, queue::DispatchOptions};

use super::{
    MailManager, MailResult, Mailable, SentMessage,
    mailables::{Address, Envelope},
};

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Recipients {
    to: Vec<Address>,
    cc: Vec<Address>,
    bcc: Vec<Address>,
    pub(super) locale: Option<String>,
}

impl Recipients {
    pub(crate) fn to(to: Vec<Address>) -> Self {
        Self {
            to,
            ..Self::default()
        }
    }

    pub(super) fn fill(&self, envelope: &mut Envelope) {
        envelope.to.extend(self.to.iter().cloned());
        envelope.cc.extend(self.cc.iter().cloned());
        envelope.bcc.extend(self.bcc.iter().cloned());
    }
}

/// Holds recipients and delivery options before a message is sent.
pub struct PendingMail {
    manager: Arc<MailManager>,
    recipients: Recipients,
}

#[allow(non_snake_case, reason = "Matches Laravel's PendingMail API")]
impl PendingMail {
    pub(super) fn new(manager: Arc<MailManager>, address: Address) -> Self {
        Self {
            manager,
            recipients: Recipients {
                to: vec![address],
                ..Default::default()
            },
        }
    }

    pub fn to(mut self, address: impl Into<Address>) -> Self {
        self.recipients.to.push(address.into());
        self
    }

    pub fn cc(mut self, address: impl Into<Address>) -> Self {
        self.recipients.cc.push(address.into());
        self
    }

    pub fn bcc(mut self, address: impl Into<Address>) -> Self {
        self.recipients.bcc.push(address.into());
        self
    }

    pub fn locale(mut self, locale: impl Into<String>) -> Self {
        self.recipients.locale = Some(locale.into());
        self
    }

    pub async fn send(self, mailable: &dyn Mailable) -> MailResult<Option<SentMessage>> {
        self.manager
            .mailer(mailable.mailer())?
            .deliver(mailable, self.recipients)
            .await
    }

    pub async fn sendNow(self, mailable: &dyn Mailable) -> MailResult<SentMessage> {
        self.manager
            .mailer(mailable.mailer())?
            .send_now_to(mailable, self.recipients)
            .await
    }

    pub async fn queue(
        self,
        mailable: &dyn Mailable,
        options: DispatchOptions,
        transaction: Option<&mut Transaction>,
    ) -> MailResult<Option<String>> {
        self.manager
            .mailer(mailable.mailer())?
            .enqueue(mailable, self.recipients, options, transaction)
            .await
    }
}
