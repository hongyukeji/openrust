use std::sync::Arc;

use crate::{
    contracts::{
        auth::{Authenticatable, UserProvider},
        hashing::Hasher,
    },
    database::{Connection, Row, Value, query::Builder},
};

use super::{AuthError, AuthFuture, AuthIdentifier, AuthResult, Credentials, providers};

/// Loads authenticatable records through the Query Builder.
pub struct DatabaseUserProvider {
    pub(super) connection: Connection,
    pub(super) hasher: Arc<dyn Hasher>,
    pub(super) table: String,
    pub(super) identifier: String,
    password: String,
    remember: String,
}

impl DatabaseUserProvider {
    pub fn new(connection: Connection, hasher: Arc<dyn Hasher>, table: impl Into<String>) -> Self {
        Self {
            connection,
            hasher,
            table: table.into(),
            identifier: "id".into(),
            password: "password".into(),
            remember: "remember_token".into(),
        }
    }

    #[must_use]
    pub fn columns(mut self, identifier: &str, password: &str, remember: &str) -> Self {
        self.identifier = identifier.into();
        self.password = password.into();
        self.remember = remember.into();
        self
    }

    fn record(&self, row: Row) -> AuthResult<Box<dyn Authenticatable>> {
        let id = row
            .get(&self.identifier)
            .ok_or(AuthError::InvalidIdentity)?
            .try_into()?;
        Ok(Box::new(DatabaseUser {
            id,
            row,
            identifier: self.identifier.clone(),
            password: self.password.clone(),
            remember: self.remember.clone(),
            updated_token: None,
        }))
    }

    async fn first(&self, query: Builder<'static>) -> AuthResult<Option<Box<dyn Authenticatable>>> {
        query
            .first()
            .await
            .map_err(AuthError::backend)?
            .map(|row| self.record(row))
            .transpose()
    }

    pub(super) async fn retrieve_by_field(
        &self,
        field: &str,
        value: Value,
    ) -> AuthResult<Option<Box<dyn Authenticatable>>> {
        self.first(
            self.connection
                .table(&self.table)
                .r#where(field, "=", value),
        )
        .await
    }
}

impl UserProvider for DatabaseUserProvider {
    fn retrieve_by_id<'a>(
        &'a self,
        id: &'a AuthIdentifier,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async move {
            let user = self
                .retrieve_by_field(&self.identifier, id.clone().into())
                .await?;
            Ok(user.filter(|user| providers::same_identity(id, &user.get_auth_identifier())))
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
            let row = self
                .connection
                .table(&self.table)
                .r#where(column, "=", token)
                .first()
                .await
                .map_err(AuthError::backend)?;
            row.filter(|row| providers::matches_api_token(row.get(column), token))
                .map(|row| self.record(row))
                .transpose()
        })
    }

    fn update_remember_token<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        token: &'a str,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(providers::update_token(
            &self.connection,
            &self.table,
            user,
            token,
        ))
    }

    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> AuthFuture<'a, AuthResult<Option<Box<dyn Authenticatable>>>> {
        Box::pin(async move {
            let mut query = self.connection.table(&self.table);
            let mut constrained = false;
            for (field, value) in credentials.fields() {
                if field != &self.password && field != &self.remember {
                    query = query.r#where(field, "=", value.clone());
                    constrained = true;
                }
            }
            if !constrained {
                return Ok(None);
            }
            self.first(query).await
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
        Box::pin(providers::rehash(
            self.hasher.as_ref(),
            &self.connection,
            &self.table,
            user,
            credentials,
            force,
        ))
    }
}

/// A database user exposing application columns without changing their types.
pub struct DatabaseUser {
    id: AuthIdentifier,
    row: Row,
    identifier: String,
    password: String,
    remember: String,
    updated_token: Option<Option<String>>,
}

impl DatabaseUser {
    pub fn get(&self, column: &str) -> Option<&Value> {
        self.row.get(column)
    }
}

fn text(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::Text(value)) => Some(value),
        _ => None,
    }
}

impl Authenticatable for DatabaseUser {
    fn get_auth_identifier_name(&self) -> &str {
        &self.identifier
    }
    fn get_auth_identifier(&self) -> AuthIdentifier {
        self.id.clone()
    }
    fn get_auth_password_name(&self) -> &str {
        &self.password
    }
    fn get_auth_password(&self) -> Option<&str> {
        text(self.get(&self.password))
    }
    fn get_remember_token(&self) -> Option<&str> {
        self.updated_token
            .as_ref()
            .map_or_else(|| text(self.get(&self.remember)), Option::as_deref)
    }
    fn set_remember_token(&mut self, token: Option<String>) {
        self.updated_token = Some(token);
    }
    fn get_remember_token_name(&self) -> &str {
        &self.remember
    }
}
