//! Passport 13.10.0 AuthCode backed by the ordinary Eloquent lifecycle.
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, ModelMetadata, ModelState, casts::AttributeCast},
};

#[derive(Default)]
pub struct AuthCode(ModelState);
#[allow(non_snake_case)]
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
    fn getKeyType(&self) -> openrust::database::eloquent::ModelKeyType {
        openrust::database::eloquent::ModelKeyType::String
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
    fn resolveConnectionName(&self) -> DatabaseResult<Option<std::borrow::Cow<'_, str>>> {
        if let Some(name) = self.getConnectionName() {
            return Ok(Some(name.into()));
        }
        crate::client::configured_connection(
            openrust::support::facades::Config::get("passport.connection")
                .map_err(openrust::database::DatabaseError::Unavailable)?,
        )
        .map(|name| name.map(std::borrow::Cow::Owned))
    }
}
#[allow(non_snake_case)]
impl AuthCode {
    pub fn client(
        &self,
    ) -> openrust::database::eloquent::relations::BelongsTo<Self, crate::Client> {
        self.clientFor::<crate::Client>()
    }
    pub fn clientFor<C: crate::ClientModel>(
        &self,
    ) -> openrust::database::eloquent::relations::BelongsTo<Self, C> {
        openrust::database::eloquent::relations::BelongsTo::new(
            "client_id",
            C::from_state(ModelState::default()).getKeyName(),
        )
    }
}
