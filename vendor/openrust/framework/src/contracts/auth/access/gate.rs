use crate::auth::{
    AuthFuture, AuthResult,
    access::{Argument, AuthorizationError, Response},
};

/// Evaluates registered abilities for a scoped user.
pub trait Gate: Send + Sync {
    fn has(&self, ability: &str) -> bool;

    fn inspect<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Response>>;

    fn allows<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<bool>>;

    fn authorize<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, Result<Response, AuthorizationError>>;
}
