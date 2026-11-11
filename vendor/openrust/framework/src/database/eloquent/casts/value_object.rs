use std::{
    any::{Any, TypeId},
    collections::BTreeMap,
};

use crate::database::{DatabaseError, DatabaseResult, Value, eloquent::Attributes};

use super::{AttributeCast, CastWrite};

/// Maps a Rust value object to model columns and a serializable value.
///
/// Cloning must give edits independent mutable state.
#[allow(non_snake_case, reason = "Matches Laravel's Eloquent naming")]
pub trait CastableObject: Clone + Send + Sync + 'static {
    const WITHOUT_OBJECT_CACHING: bool = false;

    fn fromAttributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>>;
    fn toAttributes(
        value: Option<&Self>,
        name: &str,
        raw: &Attributes,
    ) -> DatabaseResult<CastWrite>;
    fn serialize(&self) -> DatabaseResult<Value>;
}

type Object = dyn Any + Send + Sync;
type Read = fn(&str, &Attributes) -> DatabaseResult<Value>;
type Assign = fn(Value) -> DatabaseResult<Value>;

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
            read: |name, raw| {
                T::fromAttributes(name, raw)?.map_or(Ok(Value::Null), |value| value.serialize())
            },
            project: |value| value.downcast_ref::<T>().ok_or_else(invalid)?.serialize(),
            assign,
        })
    }

    pub(super) fn assign(self, value: Value) -> DatabaseResult<Value> {
        self.assign.ok_or_else(invalid)?(value)
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
    ) -> DatabaseResult<Value> {
        match cache.entries.get(name) {
            Some(value) => (self.project)(value.as_ref()),
            None => (self.read)(name, attributes),
        }
    }
}

#[derive(Default)]
pub(in crate::database::eloquent) struct ValueObjects {
    entries: BTreeMap<String, Box<Object>>,
}

impl ValueObjects {
    pub(in crate::database::eloquent) const EMPTY: Self = Self {
        entries: BTreeMap::new(),
    };

    pub(in crate::database::eloquent) fn get<T: CastableObject>(&self, name: &str) -> Option<&T> {
        self.entries.get(name)?.downcast_ref()
    }

    pub(in crate::database::eloquent) fn insert<T: CastableObject>(
        &mut self,
        name: &str,
        value: T,
    ) {
        self.entries.insert(name.into(), Box::new(value));
    }
}

pub(super) fn invalid() -> DatabaseError {
    DatabaseError::InvalidQuery("value object attribute")
}
