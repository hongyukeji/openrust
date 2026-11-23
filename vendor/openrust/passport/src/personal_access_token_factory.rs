#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};
use tokio::sync::Mutex;
type TokenInstances = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;
pub type PersonalToken<T> = Arc<Mutex<T>>;
use crate::{AuthorizationServer, OAuthError, TokenModel};
use openrust::{auth::AuthIdentifier, contracts::auth::Authenticatable, support::facades::Facade};
/// Dedicated personal grant; deliberately absent from the public token endpoint.
pub struct PersonalAccessTokenFactory;
#[allow(non_snake_case)]
impl PersonalAccessTokenFactory {
    pub async fn make(
        userId: impl Into<AuthIdentifier>,
        name: &str,
        scopes: &[String],
        provider: &str,
    ) -> Result<PersonalAccessTokenResult, OAuthError> {
        AuthorizationServer::getFacadeRoot()
            .map_err(|_| OAuthError::server_error("Personal grant unavailable"))?
            .issue_personal(userId.into(), name, scopes, provider)
            .await
    }
}
#[derive(serde::Serialize)]
#[allow(non_snake_case)]
pub struct PersonalAccessTokenResult {
    pub accessTokenId: String,
    pub accessToken: String,
    pub tokenType: String,
    pub expiresIn: i64,
    #[serde(skip)]
    tokens: Mutex<TokenInstances>,
}
impl std::fmt::Debug for PersonalAccessTokenResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PersonalAccessTokenResult([REDACTED])")
    }
}
#[allow(non_snake_case)]
impl PersonalAccessTokenResult {
    pub fn new(id: String, access: String, expires: i64) -> Self {
        Self {
            accessTokenId: id,
            accessToken: access,
            tokenType: "Bearer".into(),
            expiresIn: expires,
            tokens: Mutex::new(HashMap::new()),
        }
    }
    pub async fn getToken(
        &self,
    ) -> openrust::database::DatabaseResult<Option<PersonalToken<crate::Token>>> {
        self.getTokenFor::<crate::Token>().await
    }
    pub async fn getTokenFor<T: TokenModel>(
        &self,
    ) -> openrust::database::DatabaseResult<Option<PersonalToken<T>>> {
        let mut instances = self.tokens.lock().await;
        if let Some(instance) = instances.get(&TypeId::of::<T>()) {
            return instance
                .clone()
                .downcast::<Mutex<T>>()
                .map(Some)
                .map_err(|_| {
                    openrust::database::DatabaseError::InvalidQuery("Personal token model mismatch")
                });
        }
        let Some(model) = T::query()?.find(self.accessTokenId.clone()).await? else {
            return Ok(None);
        };
        let model = Arc::new(Mutex::new(model));
        instances.insert(TypeId::of::<T>(), model.clone());
        Ok(Some(model))
    }
    pub fn toArray(&self) -> serde_json::Value {
        serde_json::json!({"accessTokenId":self.accessTokenId,"accessToken":self.accessToken,"tokenType":self.tokenType,"expiresIn":self.expiresIn})
    }
    pub fn toJson(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
/// Compiled user models explicitly declare their configured provider name.
#[allow(non_snake_case)]
pub trait HasApiTokens: Authenticatable {
    fn getProviderName(&self) -> &str;
    fn createToken(
        &self,
        name: &str,
        scopes: &[String],
    ) -> impl Future<Output = Result<PersonalAccessTokenResult, OAuthError>> + Send {
        async move {
            PersonalAccessTokenFactory::make(
                self.getAuthIdentifier(),
                name,
                scopes,
                self.getProviderName(),
            )
            .await
        }
    }
}
