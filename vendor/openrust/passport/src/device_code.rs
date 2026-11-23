//! Passport's published device-code record, using the ordinary Eloquent lifecycle.
use openrust::database::{
    DatabaseResult,
    eloquent::{Model, ModelMetadata, ModelState, casts::AttributeCast},
};
#[derive(Default)]
pub struct DeviceCode(ModelState);
impl Model for DeviceCode {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("oauth_device_codes")
            .incrementing(false)
            .guarded(&[])
            .timestamps(None, None);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            ("scopes", AttributeCast::Json),
            ("revoked", AttributeCast::Boolean),
            ("user_approved_at", AttributeCast::DateTime),
            ("last_polled_at", AttributeCast::DateTime),
            ("expires_at", AttributeCast::DateTime),
        ]
    }
    fn get_key_type(&self) -> openrust::database::eloquent::ModelKeyType {
        openrust::database::eloquent::ModelKeyType::String
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
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
