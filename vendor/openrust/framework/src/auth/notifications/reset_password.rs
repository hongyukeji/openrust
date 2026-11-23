use std::sync::Arc;

use crate::{
    auth::{AuthError, AuthResult},
    contracts::auth::CanResetPassword,
    notifications::messages::MailDelivery,
    routing::RouteParameter,
    support::facades::{Config, URL},
};

use super::{LinkMail, MailCallbacks, send_mail};

/// Sends the named password reset link to the user's email address.
pub struct ResetPassword<'a> {
    token: &'a str,
}

impl<'a> ResetPassword<'a> {
    pub fn new(token: &'a str) -> Self {
        Self { token }
    }

    pub fn url(&self, email: &str) -> AuthResult<String> {
        URL::route(
            "password.reset",
            &[
                RouteParameter::named("token", self.token),
                RouteParameter::named("email", email),
            ],
            true,
        )
        .map_err(AuthError::backend)
    }

    /// Configure reset link creation for this application.
    pub fn create_url_using(
        callback: impl Fn(&dyn CanResetPassword, &str) -> AuthResult<String> + Send + Sync + 'static,
    ) -> AuthResult<()> {
        MailCallbacks::get()?
            .write()
            .expect("auth mail callbacks lock")
            .reset_url = Some(Arc::new(callback));
        Ok(())
    }

    /// Replace the reset notification email, receiving the user and reset token.
    pub fn to_mail_using(
        callback: impl Fn(&dyn CanResetPassword, &str) -> AuthResult<MailDelivery>
        + Send
        + Sync
        + 'static,
    ) -> AuthResult<()> {
        MailCallbacks::get()?
            .write()
            .expect("auth mail callbacks lock")
            .reset_mail = Some(Arc::new(callback));
        Ok(())
    }

    /// Send to the user, applying this application's notification callbacks.
    pub async fn send_to(&self, user: &dyn CanResetPassword) -> AuthResult<()> {
        let email = user.get_email_for_password_reset();
        send_mail("ResetPassword", email, || {
            let (url, mail) = {
                let callbacks = MailCallbacks::get()?;
                let callbacks = callbacks.read().expect("auth mail callbacks lock");
                (callbacks.reset_url.clone(), callbacks.reset_mail.clone())
            };
            if let Some(mail) = mail {
                return mail(user, self.token);
            }
            let url = match url {
                Some(create_url) => create_url(user, self.token)?,
                None => self.url(email)?,
            };
            self.default_mail(&url)
        })
        .await
    }

    /// Send the default notification using an email address alone.
    pub async fn send(&self, email: &str) -> AuthResult<()> {
        send_mail("ResetPassword", email, || {
            self.default_mail(&self.url(email)?)
        })
        .await
    }

    fn default_mail(&self, url: &str) -> AuthResult<MailDelivery> {
        let broker = Config::get("auth.defaults.passwords")?
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "users".into());
        let expires_minutes = Config::get(&format!("auth.passwords.{broker}.expire"))?
            .and_then(|value| value.as_u64())
            .unwrap_or(60);
        LinkMail {
            subject: "Reset your password",
            introduction: "You are receiving this email because we received a password reset request for your account.",
            action: "Reset Password",
            closing: "If you did not request a password reset, no further action is required.",
            url,
            expires_minutes: Some(expires_minutes),
        }.render()
    }
}
