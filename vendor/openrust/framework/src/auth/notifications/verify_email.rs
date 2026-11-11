use std::{
    fmt::Write as _,
    time::{Duration, SystemTime},
};

use aws_lc_rs::digest;

use crate::{
    auth::{AuthError, AuthResult},
    contracts::{auth::MustVerifyEmail, config::Repository, container::Container as _},
    routing::{RouteParameter, SignatureMode},
    support::facades::{URL, current_container},
};

use super::{LinkMail, route_id};

/// Sends a temporary signed link to the user's current email address.
pub struct VerifyEmail;

impl VerifyEmail {
    pub fn url(user: &dyn MustVerifyEmail) -> AuthResult<String> {
        let container = current_container().ok_or(AuthError::Configuration("application scope"))?;
        let config = container
            .get::<dyn Repository>("config")
            .map_err(AuthError::backend)?;
        let minutes = config
            .get("auth.verification.expire")
            .and_then(|value| value.as_u64())
            .unwrap_or(60);
        let expiry = minutes
            .checked_mul(60)
            .and_then(|seconds| SystemTime::now().checked_add(Duration::from_secs(seconds)))
            .ok_or(AuthError::Configuration("auth.verification.expire"))?;
        URL::temporarySignedRoute(
            "verification.verify",
            expiry,
            &[
                RouteParameter::named("id", route_id(&user.get_auth_identifier())),
                RouteParameter::named("hash", email_hash(user.getEmailForVerification())),
            ],
            SignatureMode::Absolute,
        )
        .map_err(AuthError::backend)
    }

    pub async fn send(user: &dyn MustVerifyEmail) -> AuthResult<()> {
        let url = Self::url(user)?;
        LinkMail {
            kind: "VerifyEmail",
            subject: "Verify your email address",
            introduction: "Please click the link below to verify your email address.",
            closing: "If you did not create an account, no further action is required.",
            url: &url,
        }
        .send(user.getEmailForVerification())
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
