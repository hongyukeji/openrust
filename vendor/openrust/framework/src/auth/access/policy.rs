use std::any::TypeId;

use crate::{
    auth::{AuthFuture, AuthResult},
    contracts::auth::Authenticatable,
};

use super::{Argument, Response};

/// Selects a policy for an ability without a model instance, such as create.
#[derive(Clone, Copy)]
pub struct PolicyTarget(pub(super) TypeId);

impl PolicyTarget {
    pub fn of<T: Send + Sync + 'static>() -> Self {
        Self(TypeId::of::<T>())
    }
}

/// Defines abilities associated with an application resource type.
pub trait Policy<T: 'static>: Send + Sync {
    fn has_ability(&self, ability: &str) -> bool;

    fn allows_guests(&self, _ability: &str) -> bool {
        false
    }

    fn before<'a>(
        &'a self,
        _user: Option<&'a dyn Authenticatable>,
        _ability: &'a str,
        _arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Option<Response>>> {
        Box::pin(async { Ok(None) })
    }

    fn evaluate<'a>(
        &'a self,
        ability: &'a str,
        user: Option<&'a dyn Authenticatable>,
        resource: Option<&'a T>,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Option<Response>>>;
}
