//! Passport 13.10.0 Token backed by the ordinary Eloquent lifecycle.
use openrust::database::{
    DatabaseResult, Value,
    eloquent::{Attributes, Model, ModelMetadata, ModelState, casts::AttributeCast},
};

#[derive(Default)]
pub struct Token(ModelState);
impl Model for Token {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("oauth_access_tokens")
            .incrementing(false)
            .guarded(&[]);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            ("scopes", AttributeCast::Json),
            ("revoked", AttributeCast::Boolean),
            ("expires_at", AttributeCast::DateTime),
        ]
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn get_key_type(&self) -> openrust::database::eloquent::ModelKeyType {
        openrust::database::eloquent::ModelKeyType::String
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
    fn resolve_connection_name(&self) -> DatabaseResult<Option<std::borrow::Cow<'_, str>>> {
        if let Some(name) = self.get_connection_name() {
            return Ok(Some(name.into()));
        }
        crate::client::configured_connection(
            openrust::support::facades::Config::get("passport.connection")
                .map_err(openrust::database::DatabaseError::Unavailable)?,
        )
        .map(|name| name.map(std::borrow::Cow::Owned))
    }
}
pub trait TokenModel: Model {
    fn revoke(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        async {
            self.force_fill_async(Attributes::from([("revoked".into(), true.into())]))
                .await?;
            self.save().await
        }
    }
}
impl TokenModel for Token {}
impl Token {
    pub fn client(
        &self,
    ) -> openrust::database::eloquent::relations::BelongsTo<Self, crate::Client> {
        self.client_for::<crate::Client>()
    }
    pub fn client_for<C: crate::ClientModel>(
        &self,
    ) -> openrust::database::eloquent::relations::BelongsTo<Self, C> {
        openrust::database::eloquent::relations::BelongsTo::new(
            "client_id",
            C::from_state(ModelState::default()).get_key_name(),
        )
    }
    pub fn refresh_token(
        &self,
    ) -> openrust::database::eloquent::relations::HasOne<Self, crate::RefreshToken> {
        self.refresh_token_for::<crate::RefreshToken>()
    }
    pub fn refresh_token_for<R: crate::RefreshTokenModel>(
        &self,
    ) -> openrust::database::eloquent::relations::HasOne<Self, R> {
        openrust::database::eloquent::relations::HasOne::new(self.get_key_name(), "access_token_id")
    }
    /// The compiled provider model and its authentication key replace PHP's class lookup.
    pub fn user<U: Model + openrust::contracts::auth::Authenticatable>(
        &self,
        auth_identifier_name: &'static str,
    ) -> DatabaseResult<openrust::database::eloquent::relations::BelongsTo<Self, U>> {
        let user = U::from_state(ModelState::default());
        if user.get_auth_identifier_name() != auth_identifier_name {
            return Err(openrust::database::DatabaseError::InvalidQuery(
                "token user owner key must match the provider authentication identifier",
            ));
        }
        Ok(openrust::database::eloquent::relations::BelongsTo::new(
            "user_id",
            auth_identifier_name,
        ))
    }
    pub fn can(&self, scope: &str) -> DatabaseResult<bool> {
        let Some(value) = self.get_attribute_value("scopes")? else {
            return Ok(false);
        };
        if !crate::client::truthy(Some(&value)) {
            return Ok(false);
        }
        let Value::Json(scopes) = value else {
            return Ok(false);
        };
        let scopes = crate::PhpArray::try_from(scopes)?;
        if scopes.is_empty() {
            return Ok(false);
        }
        Ok(scopes.contains_loose("*") || crate::client::scope_exists_in(scope, &scopes)?)
    }
    pub fn cant(&self, scope: &str) -> DatabaseResult<bool> {
        self.can(scope).map(|can| !can)
    }
}
