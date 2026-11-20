use crate::{PasskeyResult, StoredCredential};
use openrust::database::{
    Value,
    eloquent::{Model, ModelMetadata, ModelState, casts::AttributeCast},
};

#[derive(Clone, Default)]
pub struct Passkey(ModelState);
impl Model for Passkey {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata =
            ModelMetadata::new("passkeys").fillable(&["name", "credential_id", "credential"]);
        &META
    }
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            ("credential", AttributeCast::Json),
            ("last_used_at", AttributeCast::DateTime),
        ]
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
}
impl Passkey {
    pub fn credential(&self) -> PasskeyResult<StoredCredential> {
        match self
            .getAttributeValue("credential")
            .map_err(|_| crate::PasskeyError::Format)?
        {
            Some(Value::Json(value)) => Ok(serde_json::from_value(value)?),
            Some(Value::Text(value)) => Ok(serde_json::from_str(&value)?),
            _ => Err(crate::PasskeyError::Format),
        }
    }
    #[allow(non_snake_case, reason = "Eloquent relation adapter")]
    pub fn userFor<T: crate::contracts::PasskeyUser>(
        &self,
    ) -> openrust::database::eloquent::relations::BelongsTo<Self, T> {
        openrust::database::eloquent::relations::BelongsTo::new(
            "user_id",
            T::from_state(ModelState::default()).getKeyName(),
        )
    }
}
