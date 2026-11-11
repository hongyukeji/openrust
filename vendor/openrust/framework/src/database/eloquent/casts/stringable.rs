use crate::{
    database::{DatabaseResult, Value, eloquent::Attributes},
    support::Stringable,
};

use super::{AttributeCast, CastWrite, CastableObject, ValueObjectCast, primitive::scalar_string};

/// Converts a text attribute to a fluent string value.
pub struct AsStringable;

impl AsStringable {
    pub const fn cast() -> AttributeCast {
        ValueObjectCast::with_assignment::<Stringable>(|value| match value {
            Value::Null | Value::Text(_) => Ok(value),
            other => scalar_string(&other).map(Value::Text),
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent naming")]
impl CastableObject for Stringable {
    fn fromAttributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        match raw.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => scalar_string(value).map(Self::from).map(Some),
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
