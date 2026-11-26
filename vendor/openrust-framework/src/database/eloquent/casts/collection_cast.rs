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
        Self::map(project_item::<T>)
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

    pub(super) fn collection(
        self,
        value: &Value,
        codec: &super::Json,
    ) -> DatabaseResult<Option<Collection>> {
        let Some(values) = decode_collection(value, codec)? else {
            return Ok(None);
        };
        project_collection(values, self.factory, self.mapper).map(Some)
    }

    pub(super) fn read(self, value: &Value, codec: &super::Json) -> DatabaseResult<Value> {
        self.collection(value, codec)?
            .map_or(Ok(Value::Null), |values| {
                values.to_array().map(Value::Json).map_err(|_| invalid())
            })
    }

    pub(super) fn write(self, value: Value, codec: &super::Json) -> DatabaseResult<Value> {
        if self.nullable && matches!(value, Value::Null) {
            return Ok(Value::Null);
        }
        codec.encode(&super::json_value(value)?).map(Value::Text)
    }

    pub(super) fn equivalent(
        current: &Value,
        original: &Value,
        codec: &super::Json,
    ) -> DatabaseResult<bool> {
        let current = codec.decode_value(current)?;
        let original = codec.decode_value(original)?;
        Ok(
            match (
                as_collection(current.as_ref()),
                as_collection(original.as_ref()),
            ) {
                (Some(current), Some(original)) => current == original,
                (None, None) => current == original,
                _ => false,
            },
        )
    }
}

pub(super) fn project_collection(
    mut values: Collection,
    factory: Option<BuildJsonCollection>,
    mapper: Option<MapJsonItem>,
) -> DatabaseResult<Collection> {
    if let Some(factory) = factory {
        values = factory(values)?;
    }
    if let Some(mapper) = mapper {
        values = values.try_map(mapper)?;
    }
    Ok(values)
}

pub(super) fn project_item<T: DeserializeOwned + Serialize>(
    value: &Json,
    _: &ArrayKey,
) -> DatabaseResult<Json> {
    let value = T::deserialize(value).map_err(|_| invalid())?;
    serde_json::to_value(value).map_err(|_| invalid())
}

pub(super) fn decode_collection(
    value: &Value,
    codec: &super::Json,
) -> DatabaseResult<Option<Collection<Json>>> {
    Ok(codec.decode_value(value)?.and_then(|value| {
        (value.is_array() || value.is_object()).then(|| Collection::from_value(value))
    }))
}

fn as_collection(value: Option<&Json>) -> Option<Collection<Json>> {
    value.and_then(|document| Collection::deserialize(document).ok())
}

pub(super) fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("JSON collection attribute")
}
