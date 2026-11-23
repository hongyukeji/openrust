use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
};

use crate::database::{Connection, DatabaseError, DatabaseResult, Value, eloquent::Attributes};

use super::{AttributeCast, CastWrite, Json};

/// Maps a Rust value object to model columns and a serializable value.
///
/// Cloning must give edits independent mutable state.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub trait CastableObject: Clone + Send + Sync + 'static {
    const WITHOUT_OBJECT_CACHING: bool = false;
    /// Resolves the application's codec for the JSON-specific read and write methods.
    const USES_JSON_CODEC: bool = false;

    fn fromAttributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>>;
    fn to_attributes(
        value: Option<&Self>,
        name: &str,
        raw: &Attributes,
    ) -> DatabaseResult<CastWrite>;
    fn serialize(&self) -> DatabaseResult<Value>;

    fn fromAttributesWithJson(
        name: &str,
        raw: &Attributes,
        _codec: &Json,
    ) -> DatabaseResult<Option<Self>> {
        Self::fromAttributes(name, raw)
    }

    fn toAttributesWithJson(
        value: Option<&Self>,
        name: &str,
        raw: &Attributes,
        _codec: &Json,
    ) -> DatabaseResult<CastWrite> {
        Self::to_attributes(value, name, raw)
    }
}

type Object = dyn Any + Send + Sync;
type Read = fn(&str, &Attributes, Option<&Connection>) -> DatabaseResult<Value>;
type Assign = fn(Value, Option<&Connection>) -> DatabaseResult<Value>;

/// Registers a typed value object in the model cast declarations.
#[derive(Clone, Copy)]
pub struct ValueObjectCast {
    object_type: fn() -> TypeId,
    read: Read,
    project: fn(&Object) -> DatabaseResult<Value>,
    assign: Option<Assign>,
}

impl ValueObjectCast {
    pub const fn of<T: CastableObject>() -> AttributeCast {
        Self::build::<T>(None)
    }

    pub(super) const fn with_assignment<T: CastableObject>(assign: Assign) -> AttributeCast {
        Self::build::<T>(Some(assign))
    }

    const fn build<T: CastableObject>(assign: Option<Assign>) -> AttributeCast {
        AttributeCast::Object(Self {
            object_type: TypeId::of::<T>,
            read: |name, raw, connection| {
                read_object::<T>(name, raw, connection)?
                    .map_or(Ok(Value::Null), |value| value.serialize())
            },
            project: |value| value.downcast_ref::<T>().ok_or_else(invalid)?.serialize(),
            assign,
        })
    }

    pub(super) fn assign(
        self,
        value: Value,
        connection: Option<&Connection>,
    ) -> DatabaseResult<Value> {
        self.assign.ok_or_else(invalid)?(value, connection)
    }

    pub(super) fn check<T: CastableObject>(self) -> DatabaseResult<()> {
        if (self.object_type)() == TypeId::of::<T>() {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    pub(super) fn read(
        self,
        name: &str,
        attributes: &Attributes,
        cache: &ValueObjects,
        connection: Option<&Connection>,
    ) -> DatabaseResult<Value> {
        match cache.entries.get(name) {
            Some(value) => (self.project)(value.value.as_ref()),
            None => (self.read)(name, attributes, connection),
        }
    }
}

pub(in crate::database::eloquent) fn read_object<T: CastableObject>(
    name: &str,
    raw: &Attributes,
    connection: Option<&Connection>,
) -> DatabaseResult<Option<T>> {
    if T::USES_JSON_CODEC {
        T::fromAttributesWithJson(name, raw, &Json::for_model(connection)?)
    } else {
        T::fromAttributes(name, raw)
    }
}

pub(in crate::database::eloquent) fn write_object<T: CastableObject>(
    value: Option<&T>,
    name: &str,
    raw: &Attributes,
    connection: Option<&Connection>,
) -> DatabaseResult<CastWrite> {
    if T::USES_JSON_CODEC {
        T::toAttributesWithJson(value, name, raw, &Json::for_model(connection)?)
    } else {
        T::to_attributes(value, name, raw)
    }
}

struct CachedObject {
    value: Box<Object>,
    clone_value: fn(&Object) -> Box<Object>,
}

impl Clone for CachedObject {
    fn clone(&self) -> Self {
        Self {
            value: (self.clone_value)(self.value.as_ref()),
            clone_value: self.clone_value,
        }
    }
}

#[derive(Clone, Default)]
pub(in crate::database::eloquent) struct ValueObjects {
    entries: BTreeMap<String, CachedObject>,
}

impl ValueObjects {
    pub(in crate::database::eloquent) const EMPTY: Self = Self {
        entries: BTreeMap::new(),
    };

    pub(in crate::database::eloquent) fn get<T: CastableObject>(&self, name: &str) -> Option<&T> {
        self.entries.get(name)?.value.downcast_ref()
    }

    pub(in crate::database::eloquent) fn insert<T: CastableObject>(
        &mut self,
        name: &str,
        value: T,
    ) {
        self.entries.insert(
            name.into(),
            CachedObject {
                value: Box::new(value),
                clone_value: |value| {
                    Box::new(
                        value
                            .downcast_ref::<T>()
                            .expect("cached object type")
                            .clone(),
                    )
                },
            },
        );
    }
}

pub(super) fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("value object attribute")
}
