use crate::{Fortify, resolver::Resolver};
use openrust::{
    foundation::http::FormRequest,
    support::facades::Facade as _,
    validation::{RuleSet, ValidationError, ValidationResult},
};
#[derive(Default)]
pub struct LoginRequest;
impl FormRequest for LoginRequest {
    fn rules(&self) -> ValidationResult<RuleSet> {
        let fortify =
            Resolver::<Fortify>::getFacadeRoot().map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?;
        RuleSet::parse([
            (fortify.username().as_str(), "required|string"),
            ("password", "required|string"),
            ("remember", "sometimes"),
        ])
    }
}

#[derive(Default)]
pub struct TwoFactorLoginRequest;
impl FormRequest for TwoFactorLoginRequest {
    fn rules(&self) -> ValidationResult<RuleSet> {
        RuleSet::parse([
            ("code", "nullable|string"),
            ("recovery_code", "nullable|string"),
        ])
    }
}

#[derive(Default)]
pub struct VerifyEmailRequest;
impl FormRequest for VerifyEmailRequest {
    fn authorize<'a>(
        &'a self,
        request: &'a mut openrust::http::Request,
        _: &'a openrust::validation::Data,
    ) -> openrust::validation::ValidationFuture<
        'a,
        Result<(), openrust::auth::access::AuthorizationError>,
    > {
        Box::pin(async move {
            use openrust::auth::{
                Auth, AuthError,
                access::{AuthorizationError, Response},
            };
            use subtle::ConstantTimeEq as _;
            let user = Auth::user(request)
                .await
                .map_err(|error| {
                    AuthorizationError::Evaluation(AuthError::Backend(Box::new(error)))
                })?
                .ok_or_else(|| {
                    AuthorizationError::Denied(Response::deny("This action is unauthorized."))
                })?;
            let account =
                crate::email_verification_model::account(user.as_ref()).map_err(|error| {
                    AuthorizationError::Evaluation(AuthError::Backend(Box::new(error)))
                })?;
            let key = account.key().map_err(|error| {
                AuthorizationError::Evaluation(AuthError::Backend(Box::new(error)))
            })?;
            let hash = crate::email_verification_model::email_hash(account.email());
            if !bool::from(
                key.as_bytes()
                    .ct_eq(request.route("id").unwrap_or("").as_bytes()),
            ) || !bool::from(
                hash.as_bytes()
                    .ct_eq(request.route("hash").unwrap_or("").as_bytes()),
            ) {
                return Err(AuthorizationError::Denied(Response::deny(
                    "This action is unauthorized.",
                )));
            }
            Ok(())
        })
    }
    fn rules(&self) -> ValidationResult<RuleSet> {
        Ok(RuleSet::default())
    }
}

#[derive(Default)]
pub struct SendPasswordResetLinkRequest;
impl FormRequest for SendPasswordResetLinkRequest {
    fn rules(&self) -> ValidationResult<RuleSet> {
        let fortify =
            Resolver::<Fortify>::getFacadeRoot().map_err(|source| ValidationError::Execution {
                source: Box::new(source),
            })?;
        RuleSet::parse([(fortify.email().as_str(), "required|email")])
    }
}
