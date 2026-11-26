use crate::auth::{
    AuthFuture, AuthResult,
    access::{Argument, AuthorizationError, PolicyHandle, Response},
};

/// Evaluates registered abilities for a scoped user.
pub trait Gate: Send + Sync {
    fn get_policy_for(&self, target: &Argument) -> AuthResult<Option<PolicyHandle>>;

    fn has(&self, ability: &str) -> bool;

    #[track_caller]
    fn inspect<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Response>>;

    #[track_caller]
    fn allows<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<bool>>;

    #[track_caller]
    fn authorize<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, Result<Response, AuthorizationError>>;
}
