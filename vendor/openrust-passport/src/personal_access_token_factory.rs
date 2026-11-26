use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};
use tokio::sync::Mutex;
type TokenInstances = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;
pub type PersonalToken<T> = Arc<Mutex<T>>;
use crate::{AuthorizationServer, OauthError, TokenModel};
use openrust::{auth::AuthIdentifier, contracts::auth::Authenticatable, support::facades::Facade};
/// Dedicated personal grant; deliberately absent from the public token endpoint.
pub struct PersonalAccessTokenFactory;
impl PersonalAccessTokenFactory {
    pub async fn make(
        user_id: impl Into<AuthIdentifier>,
        name: &str,
        scopes: &[String],
        provider: &str,
    ) -> Result<PersonalAccessTokenResult, OauthError> {
        AuthorizationServer::get_facade_root()
            .map_err(|_| OauthError::server_error("Personal grant unavailable"))?
            .issue_personal(user_id.into(), name, scopes, provider)
            .await
    }
}
#[derive(serde::Serialize)]
pub struct PersonalAccessTokenResult {
    #[serde(rename = "accessTokenId")]
    pub access_token_id: String,
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "tokenType")]
    pub token_type: String,
    #[serde(rename = "expiresIn")]
    pub expires_in: i64,
    #[serde(skip)]
    tokens: Mutex<TokenInstances>,
}
impl std::fmt::Debug for PersonalAccessTokenResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PersonalAccessTokenResult([REDACTED])")
    }
}
impl PersonalAccessTokenResult {
    pub fn new(id: String, access: String, expires: i64) -> Self {
        Self {
            access_token_id: id,
            access_token: access,
            token_type: "Bearer".into(),
            expires_in: expires,
            tokens: Mutex::new(HashMap::new()),
        }
    }
    pub async fn get_token(
        &self,
    ) -> openrust::database::DatabaseResult<Option<PersonalToken<crate::Token>>> {
        self.get_token_for::<crate::Token>().await
    }
    pub async fn get_token_for<T: TokenModel>(
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
        let Some(model) = T::query()?.find(self.access_token_id.clone()).await? else {
            return Ok(None);
        };
        let model = Arc::new(Mutex::new(model));
        instances.insert(TypeId::of::<T>(), model.clone());
        Ok(Some(model))
    }
    pub fn to_array(&self) -> serde_json::Value {
        serde_json::json!({"accessTokenId":self.access_token_id,"accessToken":self.access_token,"tokenType":self.token_type,"expiresIn":self.expires_in})
    }
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
/// Compiled user models explicitly declare their configured provider name.
pub trait HasApiTokens: Authenticatable {
    fn get_provider_name(&self) -> &str;
    fn create_token(
        &self,
        name: &str,
        scopes: &[String],
    ) -> impl Future<Output = Result<PersonalAccessTokenResult, OauthError>> + Send {
        async move {
            PersonalAccessTokenFactory::make(
                self.get_auth_identifier(),
                name,
                scopes,
                self.get_provider_name(),
            )
            .await
        }
    }
}
