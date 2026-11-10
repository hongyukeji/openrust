use crate::{
    auth::{AuthFuture, AuthResult},
    contracts::auth::Authenticatable,
};

use super::{Argument, Response};

/// Defines abilities associated with an application resource type.
pub trait Policy<T: 'static>: Send + Sync {
    fn has_ability(&self, ability: &str) -> bool;

    fn before<'a>(
        &'a self,
        user: Option<&'a dyn Authenticatable>,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Option<Response>>>;

    fn evaluate<'a>(
        &'a self,
        ability: &'a str,
        user: Option<&'a dyn Authenticatable>,
        resource: Option<&'a T>,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Option<Response>>>;
}
