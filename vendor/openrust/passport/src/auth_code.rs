//! Passport 13.10.0 AuthCode backed by the ordinary Eloquent lifecycle.
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, ModelMetadata, ModelState, casts::AttributeCast},
};

#[derive(Default)]
pub struct AuthCode(ModelState);
impl Model for AuthCode {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("oauth_auth_codes")
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
impl AuthCode {
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
}
