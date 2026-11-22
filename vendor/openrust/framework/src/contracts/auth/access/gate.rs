use crate::auth::{
    AuthFuture, AuthResult,
    access::{Argument, AuthorizationError, PolicyHandle, Response},
};

/// Evaluates registered abilities for a scoped user.
pub trait Gate: Send + Sync {
    #[allow(non_snake_case, reason = "保留授权策略查询原名")]
    fn getPolicyFor(&self, target: &Argument) -> AuthResult<Option<PolicyHandle>>;

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
