use std::{future::Future, sync::Arc};

use chrono::{DateTime, Utc};

use crate::{
    auth::{AuthError, AuthIdentifier, AuthManager, AuthResult},
    contracts::auth::Authenticatable,
    database::{
        DatabaseManager,
        eloquent::{
            Builder, Model,
            relations::{MorphMany, MorphMap},
        },
    },
    http::Request,
};

use super::{AccessToken, NewAccessToken, PersonalAccessToken, PersonalAccessTokens, Sanctum};

/// The token relationship of a compiled Eloquent owner.
#[allow(non_snake_case, reason = "Matches Sanctum's owner API")]
pub trait HasApiTokens: Model + Authenticatable {
    fn tokens(&self, sanctum: &Sanctum, database: &DatabaseManager) -> AuthResult<TokenRelation> {
        let owner_id = match self.getKey() {
            Some(crate::database::Value::Integer(id)) if *id >= 0 => (*id as u64).into(),
            Some(key) => AuthIdentifier::try_from(key)?,
            None => return Err(AuthError::InvalidIdentity),
        };
        Ok(TokenRelation {
            owner_type: self.getMorphClass().map_err(AuthError::backend)?,
            owner_id,
            store: sanctum.personalAccessTokens(database, "")?,
        })
    }

    /// Typed relation for Eloquent eager loading and relationship existence queries.
    fn tokensRelation<T: super::PersonalAccessTokenModel>(
        &self,
        sanctum: &Sanctum,
    ) -> AuthResult<MorphMany<Self, T>> {
        sanctum.require_token_model::<T>()?;
        let alias = self.getMorphClass().map_err(AuthError::backend)?;
        Ok(
            MorphMany::new(self.getKeyName(), "tokenable_id", "tokenable_type", alias)
                .with_exact_morph_alias(alias),
        )
    }

    /// Starts a typed Eloquent query constrained to this owner.
    fn tokensQuery<T: super::PersonalAccessTokenModel>(
        &self,
        sanctum: &Sanctum,
        database: &DatabaseManager,
    ) -> AuthResult<Builder<'static, T>> {
        sanctum.require_token_model::<T>()?;
        let owner = self.tokens(sanctum, database)?;
        Ok(
            super::token_model::EloquentTokenStore::<T>::new(owner.store.connection().clone())
                .owner_query(owner.owner_type, owner.owner_id),
        )
    }

    fn createToken<'a>(
        &'a self,
        sanctum: &'a Sanctum,
        database: &'a DatabaseManager,
        name: &'a str,
        abilities: &'a [&'a str],
        expires_at: Option<DateTime<Utc>>,
    ) -> impl Future<Output = AuthResult<NewAccessToken>> + Send + 'a {
        async move {
            let relation = self.tokens(sanctum, database)?;
            relation
                .store
                .create_with_secret(
                    relation.owner_type,
                    relation.owner_id,
                    name,
                    abilities,
                    expires_at,
                    self.generateTokenString(sanctum)?,
                )
                .await
        }
    }

    fn generateTokenString(&self, sanctum: &Sanctum) -> AuthResult<zeroize::Zeroizing<String>> {
        super::personal_access_token::generate_secret(&sanctum.token_prefix()?)
    }

    fn currentAccessToken<'a>(&self, request: &'a Request) -> Option<&'a AccessToken> {
        self.access_token_guard(request)?;
        AccessToken::currentAccessToken(request)
    }

    fn withAccessToken<'a>(
        &'a self,
        request: &mut Request,
        token: AccessToken,
    ) -> AuthResult<&'a Self> {
        let guard = self
            .access_token_guard(request)
            .ok_or(AuthError::InvalidIdentity)?;
        super::RequestTokens::insert(request, &guard, token);
        Ok(self)
    }

    #[doc(hidden)]
    fn access_token_guard(&self, request: &Request) -> Option<String> {
        let guard = AuthManager::selected_guard(request)
            .map(str::to_owned)
            .or_else(|| {
                crate::auth::Auth::manager()
                    .ok()
                    .map(|auth| auth.getDefaultDriver())
            })?;
        let user = AuthManager::resolved_user(request, &guard)?;
        (user.get_auth_identifier() == self.get_auth_identifier()
            && user.as_ref().type_id() == std::any::TypeId::of::<Self>())
        .then_some(guard)
    }

    fn tokenCan(&self, request: &Request, ability: &str) -> bool {
        self.currentAccessToken(request)
            .is_some_and(|token| token.can(ability))
    }

    fn tokenCant(&self, request: &Request, ability: &str) -> bool {
        !self.tokenCan(request, ability)
    }
}

/// A polymorphic, owner-scoped relationship to the configured token model.
pub struct TokenRelation {
    owner_type: &'static str,
    owner_id: AuthIdentifier,
    store: PersonalAccessTokens,
}

#[allow(non_snake_case, reason = "Matches Sanctum's owner API")]
impl TokenRelation {
    pub async fn get(&self) -> AuthResult<Vec<PersonalAccessToken>> {
        self.store
            .related(self.owner_type, self.owner_id.clone())
            .await
    }

    pub async fn createToken(
        &self,
        name: &str,
        abilities: &[&str],
        expires_at: Option<DateTime<Utc>>,
    ) -> AuthResult<NewAccessToken> {
        self.store
            .createToken(
                self.owner_type,
                self.owner_id.clone(),
                name,
                abilities,
                expires_at,
            )
            .await
    }

    pub async fn delete(&self) -> AuthResult<u64> {
        self.store
            .delete_related(self.owner_type, self.owner_id.clone(), None)
            .await
    }

    pub async fn deleteToken(&self, id: impl Into<AuthIdentifier>) -> AuthResult<u64> {
        self.store
            .delete_related(self.owner_type, self.owner_id.clone(), Some(id.into()))
            .await
    }
}

impl PersonalAccessToken {
    pub async fn tokenable(
        &self,
        auth: &AuthManager,
        morphs: &MorphMap,
    ) -> AuthResult<Option<Arc<dyn Authenticatable>>> {
        let model = morphs
            .getMorphedModel(&self.tokenable_type)
            .unwrap_or(&self.tokenable_type);
        Ok(auth
            .retrieve_sanctum_owner(model, &self.tokenable_id)
            .await?
            .map(Arc::from))
    }
}
