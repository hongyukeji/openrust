use crate::{
    auth::{AuthError, AuthResult},
    routing::RouteParameter,
    support::facades::URL,
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
        LinkMail {
            kind: "ResetPassword",
            subject: "Reset Password Notification",
            introduction: "You are receiving this email because we received a password reset request for your account.",
            closing: "If you did not request a password reset, no further action is required.",
            url: &url,
        }.send(email).await
    }
}
