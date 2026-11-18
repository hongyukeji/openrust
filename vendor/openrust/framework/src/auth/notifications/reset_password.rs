use crate::{
    auth::{AuthError, AuthResult},
    routing::RouteParameter,
    support::facades::{Config, URL},
};

use super::LinkMail;

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

    pub async fn send(&self, email: &str) -> AuthResult<()> {
        let url = self.url(email)?;
        let broker = Config::get("auth.defaults.passwords")
            .map_err(AuthError::backend)?
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "users".into());
        let expires_minutes = Config::get(&format!("auth.passwords.{broker}.expire"))
            .map_err(AuthError::backend)?
            .and_then(|value| value.as_u64())
            .unwrap_or(60);
        LinkMail {
            kind: "ResetPassword",
            subject: "Reset your password",
            introduction: "You are receiving this email because we received a password reset request for your account.",
            action: "Reset Password",
            closing: "If you did not request a password reset, no further action is required.",
            url: &url,
            expires_minutes: Some(expires_minutes),
        }.send(email).await
    }
}
