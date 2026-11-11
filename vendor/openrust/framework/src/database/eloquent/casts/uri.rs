use crate::{
    database::{DatabaseError, DatabaseResult, Value, eloquent::Attributes},
    support::Uri,
};

use super::{AttributeCast, CastWrite, CastableObject, ValueObjectCast, primitive::scalar_string};

/// Converts a text attribute to a URI reference.
pub struct AsUri;

impl AsUri {
    pub const fn cast() -> AttributeCast {
        ValueObjectCast::with_assignment::<Uri>(|value| match value {
            Value::Null | Value::Text(_) => Ok(value),
            other => scalar_string(&other).map(Value::Text),
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent naming")]
impl CastableObject for Uri {
    fn fromAttributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        match raw.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => Self::of(&scalar_string(value)?)
                .map(Some)
                .map_err(|_| DatabaseError::InvalidQuery("model URI attribute")),
        }
    }

    fn toAttributes(value: Option<&Self>, _: &str, _: &Attributes) -> DatabaseResult<CastWrite> {
        Ok(CastWrite::Value(value.map_or(Value::Null, |text| {
            Value::Text(text.value().to_owned())
        })))
    }

    fn serialize(&self) -> DatabaseResult<Value> {
        Ok(Value::Text(self.value().to_owned()))
    }
}
