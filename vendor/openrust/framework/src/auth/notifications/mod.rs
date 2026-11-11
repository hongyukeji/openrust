//! Notifications issued by authentication flows.

mod reset_password;
mod verify_email;

pub use reset_password::ResetPassword;
pub use verify_email::VerifyEmail;
pub(crate) use verify_email::email_hash;

use crate::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    mail::mailables::{Content, Envelope},
    notifications::{
        Notifiable, Notification, NotificationResult, NotificationRoute,
        messages::{MailDelivery, MailMessage},
    },
    support::facades::{Lang, Notification as Notifications},
};

struct EmailRecipient<'a>(&'a str);

impl Notifiable for EmailRecipient<'_> {
    fn route_notification_for(
        &self,
        channel: &str,
        _: &dyn Notification,
    ) -> NotificationResult<Option<NotificationRoute>> {
        Ok((channel == "mail").then(|| NotificationRoute::Mail(vec![self.0.into()])))
    }
}

struct LinkMail<'a> {
    kind: &'static str,
    subject: &'static str,
    introduction: &'static str,
    closing: &'static str,
    url: &'a str,
}

impl Notification for LinkMail<'_> {
    fn kind(&self) -> &str {
        self.kind
    }

    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["mail".into()])
    }

    fn to_mail(&self, _: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        let text = |message| {
            Lang::get(message, &Default::default(), None).map_err(|source| {
                crate::notifications::NotificationError::Delivery {
                    source: Box::new(source),
                }
            })
        };
        Ok(Some(MailDelivery::Message(Box::new(MailMessage {
            envelope: Envelope {
                subject: text(self.subject)?,
                ..Envelope::default()
            },
            content: Content {
                text: Some(format!(
                    "{}\n\n{}\n\n{}",
                    text(self.introduction)?,
                    self.url,
                    text(self.closing)?
                )),
                ..Content::default()
            },
            ..MailMessage::default()
        }))))
    }
}

impl LinkMail<'_> {
    async fn send(&self, email: &str) -> AuthResult<()> {
        Notifications::send(&[&EmailRecipient(email)], self)
            .await
            .map_err(AuthError::backend)
    }
}

pub(crate) fn route_id(id: &AuthIdentifier) -> String {
    match id {
        AuthIdentifier::Integer(id) => id.to_string(),
        AuthIdentifier::UnsignedInteger(id) => id.to_string(),
        AuthIdentifier::String(id) => id.clone(),
    }
}
