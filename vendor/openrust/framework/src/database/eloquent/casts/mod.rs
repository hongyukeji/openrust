//! Model accessors, mutators, and attribute casts.

mod attribute;
mod binary;
mod class_cast;
mod collection_cast;
pub(super) mod date;
mod decimal;
mod encrypted;
mod enum_cast;
mod enum_collection;
mod fluent;
mod hashed;
mod json_path;
mod primitive;
mod stringable;
mod uri;
mod value_object;
mod vector;

pub use attribute::{Attribute, GetAttribute, SetAttribute};
pub use binary::{AsBinary, BinaryCodec, BinaryFormat};
pub use class_cast::{CastWrite, Castable, CastsAttributes, CastsInboundAttributes, ClassCast};
pub use collection_cast::{
    AsArrayObject, AsCollection, BuildJsonCollection, JsonCollectionCast, MapJsonItem,
};
pub use encrypted::{AsEncryptedArrayObject, AsEncryptedCollection, EncryptedCast};
pub use enum_cast::{CastableEnum, EnumCast};
pub use enum_collection::{AsEnumArrayObject, AsEnumCollection, EnumCollectionCast};
pub use fluent::AsFluent;
pub use primitive::AttributeCast;
pub use stringable::AsStringable;
pub use uri::AsUri;
pub(super) use value_object::ValueObjects;
pub use value_object::{CastableObject, ValueObjectCast};
pub use vector::AsVector;

use std::{collections::BTreeMap, sync::Arc};

use crate::database::{Connection, DatabaseError, DatabaseResult, Value};

use super::{Attributes, Model, ModelState};
use crate::support::Collection;

/// Shares the query's overrides across rows until a model changes its own casts.
#[derive(Clone, Default)]
pub(super) struct CastOverrides(Option<Arc<BTreeMap<String, AttributeCast>>>);

impl CastOverrides {
    pub(super) fn merge(
        &mut self,
        casts: impl IntoIterator<Item = (impl Into<String>, AttributeCast)>,
    ) {
        for (name, cast) in casts {
            Arc::make_mut(self.0.get_or_insert_with(Arc::default)).insert(name.into(), cast);
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    pub(super) fn merge_from(&mut self, overrides: &Self) {
        if self.is_empty() {
            self.clone_from(overrides);
        } else if let Some(casts) = &overrides.0 {
            self.merge(casts.iter().map(|(name, cast)| (name, *cast)));
        }
    }

    fn get(&self, name: &str) -> Option<AttributeCast> {
        self.0.as_ref()?.get(name).copied()
    }
}

fn cast_for<M: Model>(name: &str, overrides: &CastOverrides) -> Option<AttributeCast> {
    let metadata = M::metadata();
    overrides
        .get(name)
        .or_else(|| {
            M::casts()
                .iter()
                .find_map(|(column, cast)| (*column == name).then_some(*cast))
        })
        .or_else(|| {
            [
                metadata.created_at,
                metadata.updated_at,
                metadata.deleted_at,
            ]
            .contains(&Some(name))
            .then_some(AttributeCast::DateTime)
        })
}

pub(super) use primitive::json_value;

pub(super) fn serialize_attribute<M: Model>(
    model: &M,
    name: &str,
    value: Value,
) -> DatabaseResult<Value> {
    if matches!(value, Value::Null)
        || M::attribute(name)
            .and_then(|attribute| attribute.get)
            .is_some()
    {
        return Ok(value);
    }
    let state = model.state();
    match (cast_for::<M>(name, &state.casts), value) {
        (Some(AttributeCast::Custom(caster)), value) => {
            caster.serialize(name, value, &state.attributes)
        }
        (Some(AttributeCast::Class(cast)), value) => {
            cast.resolve()?.serialize(name, value, &state.attributes)
        }
        (Some(AttributeCast::Date | AttributeCast::DateTime), Value::Text(text)) => {
            let date = chrono::DateTime::parse_from_rfc3339(&text)
                .map_err(|_| DatabaseError::InvalidQuery("serialized model date"))?;
            model.serializeDate(date.to_utc()).map(Value::Text)
        }
        (
            Some(AttributeCast::DateWithFormat(format) | AttributeCast::DateTimeWithFormat(format)),
            Value::Text(text),
        ) => {
            let date = chrono::DateTime::parse_from_rfc3339(&text)
                .map_err(|_| DatabaseError::InvalidQuery("serialized model date"))?;
            date::format_for_storage(
                date.with_timezone(&timezone(state.connection.as_ref())),
                format,
            )
        }
        (_, value) => Ok(value),
    }
}

pub(super) fn check_enum<M: Model, E: CastableEnum>(
    name: &str,
    overrides: &CastOverrides,
) -> DatabaseResult<()> {
    match cast_for::<M>(name, overrides) {
        Some(AttributeCast::Enum(cast)) => cast.check_type::<E>(),
        _ => Err(DatabaseError::InvalidQuery("enum attribute cast")),
    }
}

pub(super) fn read_enum<M: Model, E: CastableEnum>(
    attributes: &Attributes,
    name: &str,
    overrides: &CastOverrides,
) -> DatabaseResult<Option<E>> {
    check_enum::<M, E>(name, overrides)?;
    let parse = |value: &Value| match value {
        Value::Null => Ok(None),
        value => E::fromValue(value).map(Some),
    };
    let raw = attributes.get(name);
    if let Some(accessor) = M::attribute(name).and_then(|attribute| attribute.get) {
        return parse(&accessor(raw, attributes)?);
    }
    raw.map_or(Ok(None), parse)
}

fn enum_collection_for<M: Model>(
    name: &str,
    overrides: &CastOverrides,
) -> DatabaseResult<EnumCollectionCast> {
    match cast_for::<M>(name, overrides) {
        Some(AttributeCast::EnumCollection(cast)) => Ok(cast),
        _ => Err(DatabaseError::InvalidQuery("enum collection cast")),
    }
}

pub(super) fn read_enum_collection<M: Model, E: CastableEnum>(
    model: &M,
    name: &str,
) -> DatabaseResult<Option<Collection<E>>> {
    let state = model.state();
    let cast = enum_collection_for::<M>(name, &state.casts)?;
    let raw = state.attributes.get(name);
    if let Some(getter) = M::attribute(name).and_then(|attribute| attribute.get) {
        return cast.read_typed(&getter(raw, &state.attributes)?);
    }
    cast.read_typed(raw.unwrap_or(&Value::Null))
}

pub(super) fn write_enum_collection<M: Model, E: CastableEnum>(
    model: &mut M,
    name: &str,
    values: Collection<E>,
) -> DatabaseResult<()> {
    let cast = enum_collection_for::<M>(name, &model.state().casts)?;
    let values = cast.typed_storage(values)?;
    if M::attribute(name)
        .and_then(|attribute| attribute.set)
        .is_some()
    {
        let values = values
            .toArray()
            .map_err(|_| DatabaseError::InvalidQuery("enum collection attribute"))?;
        model.setAttributeValue(name, Value::Json(values))?;
    } else {
        model.setAttribute(name, cast.encode(values)?);
    }
    Ok(())
}

fn json_collection_for<M: Model>(
    name: &str,
    overrides: &CastOverrides,
) -> DatabaseResult<JsonCollectionCast> {
    match cast_for::<M>(name, overrides) {
        Some(AttributeCast::JsonCollection(cast)) => Ok(cast),
        _ => Err(collection_cast::invalid()),
    }
}

pub(super) fn read_json_collection<M: Model>(
    model: &M,
    name: &str,
) -> DatabaseResult<Option<Collection>> {
    let state = model.state();
    let cast = json_collection_for::<M>(name, &state.casts)?;
    let raw = state.attributes.get(name);
    if let Some(getter) = M::attribute(name).and_then(|attribute| attribute.get) {
        return Ok(collection_cast::decode_collection(&getter(
            raw,
            &state.attributes,
        )?));
    }
    cast.collection(raw.unwrap_or(&Value::Null))
}

pub(super) fn write_json_collection<M: Model, T: serde::Serialize>(
    model: &mut M,
    name: &str,
    values: Collection<T>,
) -> DatabaseResult<()> {
    json_collection_for::<M>(name, &model.state().casts)?;
    if M::attribute(name)
        .and_then(|attribute| attribute.set)
        .is_some()
    {
        let value = values.toArray().map_err(|_| collection_cast::invalid())?;
        model.setAttributeValue(name, Value::Json(value))?;
    } else {
        let value = values.toJson().map_err(|_| collection_cast::invalid())?;
        model.setAttribute(name, Value::Text(value));
    }
    Ok(())
}

pub(super) fn read<M: Model>(state: &ModelState, name: &str) -> DatabaseResult<Option<Value>> {
    let attributes = &state.attributes;
    let connection = state.connection.as_ref();
    let raw = attributes.get(name);
    if let Some(accessor) = M::attribute(name).and_then(|attribute| attribute.get) {
        return accessor(raw, attributes).map(Some);
    }
    match (raw, cast_for::<M>(name, &state.casts)) {
        (_, Some(AttributeCast::Object(cast))) => {
            cast.read(name, attributes, state.value_objects()).map(Some)
        }
        (raw, Some(AttributeCast::Custom(caster))) => caster.get(name, raw, attributes).map(Some),
        (raw, Some(AttributeCast::Class(cast))) => cast.resolve()?.read(name, raw, attributes),
        (raw, Some(AttributeCast::Inbound(_))) => Ok(raw.cloned()),
        (Some(raw), Some(AttributeCast::Encrypted(cast))) => cast.read(raw, connection).map(Some),
        (
            Some(raw),
            Some(
                cast @ (AttributeCast::Date
                | AttributeCast::DateTime
                | AttributeCast::DateWithFormat(_)
                | AttributeCast::DateTimeWithFormat(_)
                | AttributeCast::Timestamp),
            ),
        ) => date::read(cast, raw, timezone(connection), M::metadata().date_format).map(Some),
        (Some(raw), Some(cast)) => cast.read(raw).map(Some),
        _ => Ok(raw.cloned()),
    }
}

pub(super) fn check_object<M: Model, T: CastableObject>(
    state: &ModelState,
    name: &str,
) -> DatabaseResult<()> {
    match cast_for::<M>(name, &state.casts) {
        Some(AttributeCast::Object(cast)) => cast.check::<T>(),
        _ => Err(value_object::invalid()),
    }
}

pub(super) fn equivalent<M: Model>(
    name: &str,
    current: &Value,
    original: &Value,
    connection: Option<&Connection>,
    overrides: &CastOverrides,
) -> DatabaseResult<bool> {
    if current == original {
        return Ok(true);
    }
    if matches!(current, Value::Null) {
        return Ok(false);
    }
    let cast = cast_for::<M>(name, overrides);
    if let Some(AttributeCast::Class(cast)) = cast {
        return Ok(cast
            .resolve()?
            .assignment()
            .compare(name, original, current)?
            .unwrap_or_else(|| primitive::numeric_equivalent(current, original)));
    }
    if let Some(caster) = cast.and_then(AttributeCast::class_caster) {
        return Ok(caster
            .compare(name, original, current)?
            .unwrap_or_else(|| primitive::numeric_equivalent(current, original)));
    }
    if matches!(original, Value::Null) {
        return Ok(false);
    }
    let format = M::metadata().date_format;
    match cast {
        Some(AttributeCast::Encrypted(cast)) => cast.equivalent(current, original, connection),
        Some(
            AttributeCast::Date
            | AttributeCast::DateTime
            | AttributeCast::DateWithFormat(_)
            | AttributeCast::DateTimeWithFormat(_),
        ) => date::equivalent(current, original, timezone(connection), format),
        Some(cast @ AttributeCast::Timestamp) => {
            let timezone = timezone(connection);
            Ok(date::read(cast, current, timezone, format)?
                == date::read(cast, original, timezone, format)?)
        }
        Some(AttributeCast::Enum(_)) | None => Ok(primitive::numeric_equivalent(current, original)),
        Some(AttributeCast::EnumCollection(_)) => {
            Ok(EnumCollectionCast::equivalent(current, original))
        }
        Some(AttributeCast::JsonCollection(_)) => {
            Ok(JsonCollectionCast::equivalent(current, original))
        }
        Some(AttributeCast::Object(_) | AttributeCast::Binary(_) | AttributeCast::Vector) => {
            Ok(false)
        }
        Some(cast) => cast.equivalent(current, original),
    }
}

pub(super) fn write<M: Model>(
    attributes: &mut Attributes,
    name: String,
    value: Value,
    connection: Option<&Connection>,
    overrides: &CastOverrides,
) -> DatabaseResult<()> {
    if let Some(mutator) = M::attribute(&name).and_then(|attribute| attribute.set) {
        attributes.extend(mutator(value, attributes)?);
    } else {
        let cast = cast_for::<M>(&name, overrides);
        if let Some(AttributeCast::Class(cast)) = cast {
            cast.resolve()?
                .assignment()
                .set(&name, value, attributes)?
                .apply(attributes, name);
            return Ok(());
        }
        if let Some(caster) = cast.and_then(AttributeCast::class_caster) {
            caster
                .set(&name, value, attributes)?
                .apply(attributes, name);
            return Ok(());
        }
        if name.contains("->") {
            return write_json_path::<M>(attributes, &name, value, connection, overrides);
        }
        let value = match cast {
            Some(AttributeCast::Object(cast)) => cast.assign(value)?,
            Some(AttributeCast::Encrypted(cast)) => cast.write(value, connection)?,
            Some(AttributeCast::Hashed) => hashed::write(value, connection)?,
            Some(
                AttributeCast::Date
                | AttributeCast::DateTime
                | AttributeCast::DateWithFormat(_)
                | AttributeCast::DateTimeWithFormat(_),
            ) => date::store(value, timezone(connection), M::metadata().date_format)?,
            Some(cast) => cast.write(value)?,
            None => value,
        };
        attributes.insert(name, value);
    }
    Ok(())
}

pub(super) fn needs_async<M: Model>(name: &str, overrides: &CastOverrides) -> bool {
    !name.contains("->")
        && matches!(cast_for::<M>(name, overrides), Some(AttributeCast::Hashed))
        && M::attribute(name)
            .and_then(|attribute| attribute.set)
            .is_none()
}

pub(super) async fn write_async<M: Model>(
    attributes: &mut Attributes,
    name: String,
    value: Value,
    connection: Option<&Connection>,
    overrides: &CastOverrides,
) -> DatabaseResult<()> {
    if needs_async::<M>(&name, overrides) {
        let value = hashed::write_async(value, connection).await?;
        attributes.insert(name, value);
        return Ok(());
    }
    write::<M>(attributes, name, value, connection, overrides)
}

pub(super) fn write_json_path<M: Model>(
    attributes: &mut Attributes,
    name: &str,
    value: Value,
    connection: Option<&Connection>,
    overrides: &CastOverrides,
) -> DatabaseResult<()> {
    let (column, path) = name
        .split_once("->")
        .filter(|(column, _)| !column.is_empty())
        .ok_or_else(json_path::invalid)?;
    let cast = cast_for::<M>(column, overrides);
    if matches!(cast, Some(AttributeCast::Hashed)) {
        return Err(json_path::invalid());
    }
    let value = json_path::write(path, attributes.get(column), value, cast, connection)?;
    attributes.insert(column.into(), value);
    Ok(())
}

pub(super) fn check_encrypted_collection<M: Model>(
    name: &str,
    overrides: &CastOverrides,
) -> DatabaseResult<()> {
    match cast_for::<M>(name, overrides) {
        Some(AttributeCast::Encrypted(
            EncryptedCast::Collection
            | EncryptedCast::CollectionObject
            | EncryptedCast::ArrayObject,
        )) => Ok(()),
        _ => Err(DatabaseError::InvalidQuery(
            "encrypted collection attribute",
        )),
    }
}

fn timezone(connection: Option<&Connection>) -> chrono_tz::Tz {
    connection.map_or(chrono_tz::UTC, |connection| connection.model_timezone)
}
