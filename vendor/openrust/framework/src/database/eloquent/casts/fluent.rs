use crate::{
    database::{DatabaseError, DatabaseResult, Value, eloquent::Attributes},
    support::Fluent,
};

use super::{AttributeCast, CastWrite, CastableObject, ValueObjectCast, json_value};

/// Converts a JSON attribute into a fluent attribute bag.
pub struct AsFluent;

impl AsFluent {
    pub const fn cast() -> AttributeCast {
        ValueObjectCast::with_assignment::<Fluent>(|value| {
            if matches!(value, Value::Null) {
                return Ok(Value::Null);
            }
            encode(&json_value(value)?)
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent naming")]
impl CastableObject for Fluent {
    fn fromAttributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        let decoded = match raw.get(name) {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::Json(value)) => value.clone(),
            Some(Value::Text(value)) => serde_json::from_str(value).map_err(|_| invalid())?,
            _ => return Err(invalid()),
        };
        Self::make(decoded).map(Some).map_err(|_| invalid())
    }

    fn toAttributes(value: Option<&Self>, _: &str, _: &Attributes) -> DatabaseResult<CastWrite> {
        value
            .map_or(Ok(Value::Null), |value| encode(value.all()))
            .map(CastWrite::Value)
    }

    fn serialize(&self) -> DatabaseResult<Value> {
        Ok(Value::Json(self.toArray()))
    }
}

fn encode(value: &serde_json::Value) -> DatabaseResult<Value> {
    serde_json::to_string(value)
        .map(Value::Text)
        .map_err(|_| invalid())
}

fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("fluent attribute")
}
