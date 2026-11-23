use std::{
    fmt::Write as _,
    sync::Arc,
    time::{Duration, SystemTime},
};

use aws_lc_rs::digest;

use crate::{
    auth::{AuthError, AuthResult},
    contracts::{auth::MustVerifyEmail, config::Repository, container::Container as _},
    notifications::messages::MailDelivery,
    routing::{RouteParameter, SignatureMode},
    support::facades::{Url, current_container},
};

use super::{LinkMail, MailCallbacks, route_id, send_mail};

/// Sends a temporary signed link to the user's current email address.
pub struct VerifyEmail;

impl VerifyEmail {
    /// Configure verification link creation for this application.
    pub fn create_url_using(
        callback: impl Fn(&dyn MustVerifyEmail) -> AuthResult<String> + Send + Sync + 'static,
    ) -> AuthResult<()> {
        MailCallbacks::get()?
            .write()
            .expect("auth mail callbacks lock")
            .verify_url = Some(Arc::new(callback));
        Ok(())
    }

    /// Replace the verification notification email, receiving the user and its link.
    pub fn to_mail_using(
        callback: impl Fn(&dyn MustVerifyEmail, &str) -> AuthResult<MailDelivery>
        + Send
        + Sync
        + 'static,
    ) -> AuthResult<()> {
        MailCallbacks::get()?
            .write()
            .expect("auth mail callbacks lock")
            .verify_mail = Some(Arc::new(callback));
        Ok(())
    }

    pub fn url(user: &dyn MustVerifyEmail) -> AuthResult<String> {
        let callback = MailCallbacks::get()?
            .read()
            .expect("auth mail callbacks lock")
            .verify_url
            .clone();
        match callback {
            Some(create_url) => create_url(user),
            None => Self::default_url(user),
        }
    }

    fn default_url(user: &dyn MustVerifyEmail) -> AuthResult<String> {
        let container = current_container().ok_or(AuthError::Configuration("application scope"))?;
        let config = container.get::<dyn Repository>("config")?;
        let minutes = config
            .get("auth.verification.expire")
            .and_then(|value| value.as_u64())
            .unwrap_or(60);
        let expiry = minutes
            .checked_mul(60)
            .and_then(|seconds| SystemTime::now().checked_add(Duration::from_secs(seconds)))
            .ok_or(AuthError::Configuration("auth.verification.expire"))?;
        Url::temporary_signed_route(
            "verification.verify",
            expiry,
            &[
                RouteParameter::named("id", route_id(&user.get_auth_identifier())),
                RouteParameter::named("hash", email_hash(user.get_email_for_verification())),
            ],
            SignatureMode::Absolute,
        )
        .map_err(AuthError::backend)
    }

    pub async fn send(user: &dyn MustVerifyEmail) -> AuthResult<()> {
        send_mail("VerifyEmail", user.get_email_for_verification(), || {
            let url = Self::url(user)?;
            let mail = MailCallbacks::get()?
                .read()
                .expect("auth mail callbacks lock")
                .verify_mail
                .clone();
            if let Some(mail) = mail {
                return mail(user, &url);
            }
            LinkMail {
                subject: "Verify your email address",
                introduction: "Please click the button below to verify your email address.",
                action: "Verify Email Address",
                closing: "If you did not create an account, no further action is required.",
                url: &url,
                expires_minutes: None,
            }
            .render()
        })
        .await
    }
}

pub(crate) fn email_hash(email: &str) -> String {
    let digest = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, email.as_bytes());
    let mut hash = String::with_capacity(40);
    for byte in digest.as_ref() {
        write!(hash, "{byte:02x}").expect("writing to a String");
    }
    hash
}
