use crate::{PasskeyResult, StoredCredential};
use openrust::database::{
    Value,
    eloquent::{
        Attributes, Model, ModelMetadata, ModelState,
        casts::{Attribute, AttributeCast},
    },
};

#[derive(Clone, Default)]
pub struct Passkey(ModelState);
impl Model for Passkey {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("passkeys")
            .fillable(&["name", "credential_id", "credential"])
            .appends(&["authenticator"]);
        &META
    }
    fn attribute(name: &str) -> Option<Attribute> {
        match name {
            "authenticator" => Some(Attribute::get(|_, values| Ok(display_value(values, None)))),
            "authenticator_icon_light" => Some(Attribute::get(|_, values| {
                Ok(display_value(values, Some("light")))
            })),
            "authenticator_icon_dark" => Some(Attribute::get(|_, values| {
                Ok(display_value(values, Some("dark")))
            })),
            _ => None,
        }
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
        <Self as PasskeyModel>::credential(self)
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

fn display_value(values: &Attributes, theme: Option<&str>) -> Value {
    let credential = match values.get("credential") {
        Some(Value::Json(value)) => value.clone(),
        Some(Value::Text(value)) => serde_json::from_str(value).unwrap_or_default(),
        _ => return Value::Null,
    };
    let Some(id) = credential
        .get("aaguid")
        .and_then(serde_json::Value::as_str)
        .filter(|id| *id != crate::Aaguids::unknown())
    else {
        return Value::Null;
    };
    match theme {
        Some(theme) => crate::Aaguids::iconFor(id, theme).ok().flatten(),
        None => crate::Aaguids::labelFor(id),
    }
    .map_or(Value::Null, Value::Text)
}

/// Rust counterpart of extending the native Passkey Eloquent model.
#[allow(non_snake_case, reason = "Matches Passkeys Eloquent model operations")]
pub trait PasskeyModel: Model + Clone {
    fn credential(&self) -> PasskeyResult<StoredCredential> {
        credential_value(
            self.getAttributeValue("credential")
                .map_err(|_| crate::PasskeyError::Format)?,
        )
    }
    fn userFor<T: crate::contracts::PasskeyUser>(
        &self,
    ) -> openrust::database::eloquent::relations::BelongsTo<Self, T> {
        openrust::database::eloquent::relations::BelongsTo::new(
            "user_id",
            T::from_state(ModelState::default()).getKeyName(),
        )
    }
}
impl PasskeyModel for Passkey {}
fn credential_value(value: Option<Value>) -> PasskeyResult<StoredCredential> {
    match value {
        Some(Value::Json(value)) => Ok(serde_json::from_value(value)?),
        Some(Value::Text(value)) => Ok(serde_json::from_str(&value)?),
        _ => Err(crate::PasskeyError::Format),
    }
}
