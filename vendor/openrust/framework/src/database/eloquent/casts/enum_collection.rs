use serde_json::Value as Json;

use crate::{
    database::{DatabaseError, DatabaseResult, Value},
    support::Collection,
};

use super::collection_cast::decode_collection;
use super::{AttributeCast, CastableEnum, EnumCast};

/// Casts a JSON list or keyed object to a collection of enum cases.
pub struct AsEnumCollection;

impl AsEnumCollection {
    pub const fn of<E: CastableEnum>() -> AttributeCast {
        AttributeCast::EnumCollection(EnumCollectionCast {
            cases: EnumCast::of::<E>(),
            reindex: false,
        })
    }
}

/// Casts a JSON array object to enum cases and reindexes keys on assignment.
pub struct AsEnumArrayObject;

impl AsEnumArrayObject {
    pub const fn of<E: CastableEnum>() -> AttributeCast {
        AttributeCast::EnumCollection(EnumCollectionCast {
            cases: EnumCast::of::<E>(),
            reindex: true,
        })
    }
}

#[derive(Clone, Copy)]
pub struct EnumCollectionCast {
    cases: EnumCast,
    reindex: bool,
}

impl EnumCollectionCast {
    pub(super) fn read(self, value: &Value) -> DatabaseResult<Value> {
        let Some(values) = decode_collection(value) else {
            return Ok(Value::Null);
        };
        let canonical = values.tryMap(|value, _| {
            let case = self.cases.normalize(&backing(value)?)?;
            storage(case)
        })?;
        canonical.toArray().map(Value::Json).map_err(|_| invalid())
    }

    pub(super) fn read_typed<E: CastableEnum>(
        self,
        value: &Value,
    ) -> DatabaseResult<Option<Collection<E>>> {
        self.cases.check_type::<E>()?;
        decode_collection(value)
            .map(|values| values.tryMap(|value, _| E::fromValue(&backing(value)?)))
            .transpose()
    }

    pub(super) fn typed_storage<E: CastableEnum>(
        self,
        values: Collection<E>,
    ) -> DatabaseResult<Collection<Json>> {
        self.cases.check_type::<E>()?;
        values
            .into_iter()
            .map(|(key, value)| Ok((key, storage(value.intoValue())?)))
            .collect::<DatabaseResult<Vec<_>>>()
            .map(Collection::fromEntries)
    }

    pub(super) fn write(self, value: Value) -> DatabaseResult<Value> {
        if matches!(value, Value::Null) {
            return Ok(Value::Null);
        }
        let json = super::json_value(value)?;
        if self.reindex && !json.is_array() && !json.is_object() {
            return Err(invalid());
        }
        self.encode(Collection::fromValue(json))
    }

    pub(super) fn encode(self, values: Collection<Json>) -> DatabaseResult<Value> {
        let values = if self.reindex {
            values.values()
        } else {
            values
        };
        values.toJson().map(Value::Text).map_err(|_| invalid())
    }

    pub(super) fn equivalent(current: &Value, original: &Value) -> bool {
        super::JsonCollectionCast::equivalent(current, original)
    }
}

fn backing(value: &Json) -> DatabaseResult<Value> {
    match value {
        Json::String(text) => Ok(text.as_str().into()),
        Json::Number(number) => number
            .as_i64()
            .map(Value::Integer)
            .or_else(|| number.as_u64().map(Value::Unsigned))
            .ok_or_else(invalid),
        _ => Err(invalid()),
    }
}

fn storage(value: Value) -> DatabaseResult<Json> {
    match value {
        Value::Text(_) | Value::Integer(_) | Value::Unsigned(_) => super::json_value(value),
        _ => Err(invalid()),
    }
}

fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("enum collection attribute")
}
