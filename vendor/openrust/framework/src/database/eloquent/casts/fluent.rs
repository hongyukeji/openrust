use crate::{
    database::{DatabaseError, DatabaseResult, Value, eloquent::Attributes},
    support::Fluent,
};

use super::{AttributeCast, CastWrite, CastableObject, Json, ValueObjectCast, json_value};

/// Converts a JSON attribute into a fluent attribute bag.
pub struct AsFluent;

impl AsFluent {
    pub const fn cast() -> AttributeCast {
        ValueObjectCast::with_assignment::<Fluent>(|value, connection| {
            if matches!(value, Value::Null) {
                return Ok(Value::Null);
            }
            Json::for_model(connection)?
                .encode(&json_value(value)?)
                .map(Value::Text)
        })
    }
}

impl CastableObject for Fluent {
    const USES_JSON_CODEC: bool = true;

    fn from_attributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        Self::from_attributes_with_json(name, raw, &Json::DEFAULT)
    }

    fn from_attributes_with_json(
        name: &str,
        raw: &Attributes,
        codec: &Json,
    ) -> DatabaseResult<Option<Self>> {
        let decoded = match raw.get(name) {
            None | Some(Value::Null) => return Ok(None),
            Some(value) => codec.decode_value(value)?.ok_or_else(invalid)?,
        };
        Self::make(decoded).map(Some).map_err(|_| invalid())
    }

    fn to_attributes(
        value: Option<&Self>,
        name: &str,
        raw: &Attributes,
    ) -> DatabaseResult<CastWrite> {
        Self::to_attributes_with_json(value, name, raw, &Json::DEFAULT)
    }

    fn to_attributes_with_json(
        value: Option<&Self>,
        _: &str,
        _: &Attributes,
        codec: &Json,
    ) -> DatabaseResult<CastWrite> {
        value
            .map_or(Ok(Value::Null), |value| {
                codec.encode(value.all()).map(Value::Text)
            })
            .map(CastWrite::Value)
    }

    fn serialize(&self) -> DatabaseResult<Value> {
        Ok(Value::Json(self.to_array()))
    }
}

fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("fluent attribute")
}
