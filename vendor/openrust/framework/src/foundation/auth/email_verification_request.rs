use subtle::ConstantTimeEq;

use crate::{
    auth::{
        Auth, AuthError,
        events::Verified,
        notifications::{email_hash, route_id},
    },
    contracts::auth::MustVerifyEmail,
    http::{HttpError, Request},
    support::facades::Event,
};

/// Authorizes a signed verification request for the current authenticated user.
pub struct EmailVerificationRequest;

impl EmailVerificationRequest {
    pub async fn authorize(
        request: &mut Request,
        user: &dyn MustVerifyEmail,
    ) -> Result<(), HttpError> {
        let Some(authenticated) = Auth::user(request).await? else {
            return Err(HttpError::Unauthenticated);
        };
        let id = user.get_auth_identifier();
        let valid = authenticated.get_auth_identifier() == id
            && request.route("id").is_some_and(|parameter| {
                bool::from(parameter.as_bytes().ct_eq(route_id(&id).as_bytes()))
            })
            && request.route("hash").is_some_and(|parameter| {
                bool::from(
                    parameter
                        .as_bytes()
                        .ct_eq(email_hash(user.getEmailForVerification()).as_bytes()),
                )
            });
        if valid {
            Ok(())
        } else {
            Err(HttpError::AuthorizationDenied(
                "This verification link does not match the authenticated user.".into(),
            ))
        }
    }

    /// Marks the authorized user as verified and dispatches the event on the first change.
    pub async fn fulfill(
        request: &mut Request,
        user: &mut dyn MustVerifyEmail,
    ) -> Result<(), HttpError> {
        Self::authorize(request, user).await?;
        if !user.hasVerifiedEmail() && user.markEmailAsVerified().await? {
            Event::dispatch(Verified {
                user: user.get_auth_identifier(),
            })
            .await
            .map_err(AuthError::backend)?;
        }
        Ok(())
    }
}
