//! Passport 13.10.0 RefreshToken backed by the ordinary Eloquent lifecycle.
use openrust::database::{
    DatabaseResult,
    eloquent::{Attributes, Model, ModelMetadata, ModelState, casts::AttributeCast},
};

#[derive(Default)]
pub struct RefreshToken(ModelState);
impl Model for RefreshToken {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("oauth_refresh_tokens")
            .incrementing(false)
            .guarded(&[])
            .timestamps(None, None);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
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
pub trait RefreshTokenModel: Model {
    fn revoke(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        async {
            self.force_fill_async(Attributes::from([("revoked".into(), true.into())]))
                .await?;
            self.save().await
        }
    }
}
impl RefreshTokenModel for RefreshToken {}
impl RefreshToken {
    pub fn access_token(
        &self,
    ) -> openrust::database::eloquent::relations::BelongsTo<Self, crate::Token> {
        self.access_token_for::<crate::Token>()
    }
    pub fn access_token_for<T: crate::TokenModel>(
        &self,
    ) -> openrust::database::eloquent::relations::BelongsTo<Self, T> {
        openrust::database::eloquent::relations::BelongsTo::new(
            "access_token_id",
            T::from_state(ModelState::default()).get_key_name(),
        )
    }
}
