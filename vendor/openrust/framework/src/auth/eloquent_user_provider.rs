use std::{marker::PhantomData, sync::Arc};

use crate::{
    contracts::{
        auth::{Authenticatable, UserProvider},
        hashing::Hasher,
    },
    database::{
        Connection, Value,
        eloquent::{Model, ModelState},
    },
};

use super::{AuthError, AuthFuture, AuthIdentifier, AuthResult, Credentials, providers};

/// Loads a compiled Eloquent model with its query scopes.
pub struct EloquentUserProvider<M> {
    connection: Connection,
    hasher: Arc<dyn Hasher>,
    identifier: String,
    password: String,
    remember: String,
    model: PhantomData<fn() -> M>,
}

impl<M: Model + Authenticatable> EloquentUserProvider<M> {
    pub fn new(connection: Connection, hasher: Arc<dyn Hasher>) -> Self {
        let prototype = M::from_state(ModelState::default());
        Self {
            connection,
            hasher,
            identifier: prototype.get_auth_identifier_name().into(),
            password: prototype.get_auth_password_name().into(),
            remember: prototype.get_remember_token_name().into(),
            model: PhantomData,
        }
    }
}

impl<M: Model + Authenticatable> UserProvider for EloquentUserProvider<M> {
    fn retrieve_by_id<'a>(
        &'a self,
        id: &'a AuthIdentifier,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async move {
            let mut query = M::queryOn(&self.connection);
            query.r#where(&self.identifier, "=", Value::from(id.clone()));
            let user = query.first().await.map_err(AuthError::backend)?;
            Ok(user
                .filter(|user| providers::same_identity(id, &user.get_auth_identifier()))
                .map(erase))
        })
    }

    fn retrieve_by_token<'a>(
        &'a self,
        id: &'a AuthIdentifier,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async move {
            Ok(self
                .retrieve_by_id(id)
                .await?
                .filter(|user| providers::matches_token(user.as_ref(), token)))
        })
    }

    fn retrieve_by_api_token<'a>(
        &'a self,
        column: &'a str,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async move {
            let mut query = M::queryOn(&self.connection);
            query.r#where(column, "=", token);
            Ok(query
                .first()
                .await
                .map_err(AuthError::backend)?
                .filter(|user| providers::matches_api_token(user.getAttribute(column), token))
                .map(erase))
        })
    }

    fn update_remember_token<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            let model = M::from_state(ModelState::default());
            providers::update_token(&self.connection, model.getTable(), user, token).await
        })
    }

    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async move {
            let mut query = M::queryOn(&self.connection);
            let mut constrained = false;
            for (field, value) in credentials.fields() {
                if field != &self.password && field != &self.remember {
                    query.r#where(field, "=", value.clone());
                    constrained = true;
                }
            }
            if !constrained {
                return Ok(None);
            }
            Ok(query.first().await.map_err(AuthError::backend)?.map(erase))
        })
    }

    fn validate_credentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(providers::validate(self.hasher.as_ref(), user, credentials))
    }

    fn rehash_password_if_required<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        credentials: &'a Credentials,
        force: bool,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            let model = M::from_state(ModelState::default());
            providers::rehash(
                self.hasher.as_ref(),
                &self.connection,
                model.getTable(),
                user,
                credentials,
                force,
            )
            .await
        })
    }
}

fn erase(user: impl Authenticatable) -> Box<dyn Authenticatable> {
    Box::new(user)
}
