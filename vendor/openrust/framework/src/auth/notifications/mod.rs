//! Notifications issued by authentication flows.

mod reset_password;
mod verify_email;

pub use reset_password::ResetPassword;
pub use verify_email::VerifyEmail;
pub(crate) use verify_email::email_hash;

use std::collections::BTreeMap;

use crate::{
    auth::{AuthError, AuthIdentifier, AuthResult},
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
    action: &'static str,
    closing: &'static str,
    url: &'a str,
    expires_minutes: Option<u64>,
}

impl Notification for LinkMail<'_> {
    fn kind(&self) -> &str {
        self.kind
    }

    fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
        Ok(vec!["mail".into()])
    }

    fn to_mail(&self, _: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
        let translate = |message: &str, replacements: &BTreeMap<String, String>| {
            Lang::get(message, replacements, None).map_err(|source| {
                crate::notifications::NotificationError::Delivery {
                    source: Box::new(source),
                }
            })
        };
        let empty = Default::default();
        let mut mail = MailMessage::new()
            .subject(translate(self.subject, &empty)?)
            .line(translate(self.introduction, &empty)?)
            .action(translate(self.action, &empty)?, self.url);
        if let Some(minutes) = self.expires_minutes {
            mail = mail.line(translate(
                "This password reset link will expire in :count minutes.",
                &[("count".into(), minutes.to_string())].into(),
            )?);
        }
        mail = mail.line(translate(self.closing, &empty)?);
        Ok(Some(MailDelivery::Message(Box::new(mail))))
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
