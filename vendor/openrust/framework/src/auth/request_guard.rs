use std::sync::Arc;

use crate::{
    contracts::auth::{Authenticatable, Guard},
    http::Request,
};

use super::{AuthFuture, AuthIdentifier, AuthResult, Credentials, session_guard::AuthContext};

pub(super) type Resolver = dyn for<'a> Fn(&'a mut Request) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>>
    + Send
    + Sync;

/// Resolves a stateless identity once per request.
pub struct RequestGuard<'a> {
    request: &'a mut Request,
    resolve: Arc<Resolver>,
    name: String,
}

impl<'a> RequestGuard<'a> {
    pub(super) fn new(name: String, request: &'a mut Request, resolve: Arc<Resolver>) -> Self {
        Self {
            request,
            resolve,
            name,
        }
    }
}

impl Guard for RequestGuard<'_> {
    fn user(&mut self) -> AuthFuture<'_, AuthResult<Option<&dyn Authenticatable>>> {
        Box::pin(async move {
            if !AuthContext::state(self.request, &self.name).is_some_and(|state| state.resolved) {
                let user = (self.resolve)(self.request).await?;
                AuthContext::set(self.request, &self.name, user, false);
            }
            Ok(
                AuthContext::state(self.request, &self.name)
                    .and_then(|state| state.user.as_deref()),
            )
        })
    }

    fn id(&mut self) -> AuthFuture<'_, AuthResult<Option<AuthIdentifier>>> {
        Box::pin(async move { Ok(self.user().await?.map(Authenticatable::get_auth_identifier)) })
    }

    fn check(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async move { Ok(self.user().await?.is_some()) })
    }

    fn guest(&mut self) -> AuthFuture<'_, AuthResult<bool>> {
        Box::pin(async move { Ok(!self.check().await?) })
    }

    fn has_user(&self) -> bool {
        AuthContext::state(self.request, &self.name).is_some_and(|state| state.user.is_some())
    }

    fn validate<'a>(&'a mut self, _: &'a Credentials) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(async { Ok(false) })
    }

    fn set_user(&mut self, user: Box<dyn Authenticatable>) {
        AuthContext::set(self.request, &self.name, Some(user), false);
    }
}
