use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value as Json;

use crate::{
    database::{DatabaseError, DatabaseResult, Value},
    support::{ArrayKey, Collection},
};

use super::AttributeCast;

/// Casts a JSON array or keyed object to an editable collection.
pub struct AsArrayObject;

impl AsArrayObject {
    pub const fn cast() -> AttributeCast {
        JsonCollectionCast::declaration(false, None)
    }

    pub const fn nullable() -> AttributeCast {
        JsonCollectionCast::declaration(true, None)
    }
}

/// Casts a JSON array or keyed object to a collection, optionally mapping its items.
pub struct AsCollection;

impl AsCollection {
    pub const fn cast() -> AttributeCast {
        JsonCollectionCast::declaration(false, None)
    }

    pub const fn nullable() -> AttributeCast {
        JsonCollectionCast::declaration(true, None)
    }

    pub const fn of<T: DeserializeOwned + Serialize>() -> AttributeCast {
        Self::map(|value, _| {
            let value = T::deserialize(value).map_err(|_| invalid())?;
            serde_json::to_value(value).map_err(|_| invalid())
        })
    }

    pub const fn map(mapper: MapJsonItem) -> AttributeCast {
        JsonCollectionCast::declaration(false, Some(mapper))
    }

    /// Builds the collection before applying its optional item mapper.
    pub const fn using(factory: BuildJsonCollection, mapper: Option<MapJsonItem>) -> AttributeCast {
        AttributeCast::JsonCollection(JsonCollectionCast {
            nullable: false,
            factory: Some(factory),
            mapper,
        })
    }
}

/// A fallible mapper for a decoded JSON collection item.
pub type MapJsonItem = fn(&Json, &ArrayKey) -> DatabaseResult<Json>;
pub type BuildJsonCollection = fn(Collection) -> DatabaseResult<Collection>;

#[derive(Clone, Copy)]
pub struct JsonCollectionCast {
    nullable: bool,
    factory: Option<BuildJsonCollection>,
    mapper: Option<MapJsonItem>,
}

impl JsonCollectionCast {
    const fn declaration(nullable: bool, mapper: Option<MapJsonItem>) -> AttributeCast {
        AttributeCast::JsonCollection(Self {
            nullable,
            factory: None,
            mapper,
        })
    }

    pub(super) fn collection(self, value: &Value) -> DatabaseResult<Option<Collection>> {
        let Some(mut values) = decode_collection(value) else {
            return Ok(None);
        };
        if let Some(factory) = self.factory {
            values = factory(values)?;
        }
        if let Some(mapper) = self.mapper {
            values = values.tryMap(mapper)?;
        }
        Ok(Some(values))
    }

    pub(super) fn read(self, value: &Value) -> DatabaseResult<Value> {
        self.collection(value)?.map_or(Ok(Value::Null), |values| {
            values.toArray().map(Value::Json).map_err(|_| invalid())
        })
    }

    pub(super) fn write(self, value: Value) -> DatabaseResult<Value> {
        if self.nullable && matches!(value, Value::Null) {
            return Ok(Value::Null);
        }
        serde_json::to_string(&super::json_value(value)?)
            .map(Value::Text)
            .map_err(|_| invalid())
    }

    pub(super) fn equivalent(current: &Value, original: &Value) -> bool {
        match (decode_collection(current), decode_collection(original)) {
            (Some(current), Some(original)) => current == original,
            (None, None) => decode_scalar(current) == decode_scalar(original),
            _ => false,
        }
    }
}

pub(super) fn decode_collection(value: &Value) -> Option<Collection<Json>> {
    match value {
        Value::Text(text) => serde_json::from_str(text).ok(),
        Value::Json(json) => Collection::deserialize(json).ok(),
        _ => None,
    }
}

fn decode_scalar(value: &Value) -> Option<Json> {
    match value {
        Value::Text(text) => serde_json::from_str(text).ok(),
        Value::Json(json) => Some(json.clone()),
        _ => None,
    }
}

pub(super) fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("JSON collection attribute")
}
