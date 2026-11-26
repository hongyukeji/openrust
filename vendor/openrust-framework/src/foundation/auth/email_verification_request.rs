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
        let key = user.get_email_verification_identifier()?;
        let valid = (authenticated.as_ref() as &dyn std::any::Any).type_id()
            == (user as &dyn std::any::Any).type_id()
            && authenticated.get_auth_identifier() == id
            && request.route("id").is_some_and(|parameter| {
                bool::from(parameter.as_bytes().ct_eq(route_id(&key).as_bytes()))
            })
            && request.route("hash").is_some_and(|parameter| {
                bool::from(
                    parameter
                        .as_bytes()
                        .ct_eq(email_hash(user.get_email_for_verification()).as_bytes()),
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
        if !user.has_verified_email() && user.mark_email_as_verified().await? {
            Event::dispatch(Verified {
                user: user.get_auth_identifier(),
            })
            .await
            .map_err(AuthError::backend)?;
        }
        Ok(())
    }
}
