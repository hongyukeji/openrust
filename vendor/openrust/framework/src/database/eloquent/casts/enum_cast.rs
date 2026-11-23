use std::any::TypeId;

use crate::database::{DatabaseError, DatabaseResult, Value};

/// Maps an application enum to its database backing value or case name.
pub trait CastableEnum: Sized + 'static {
    fn from_value(value: &Value) -> DatabaseResult<Self>;

    fn into_value(self) -> Value;
}

/// Registers the enum type and its conversion functions with a model.
#[derive(Clone, Copy)]
pub struct EnumCast {
    type_id: fn() -> TypeId,
    normalize: fn(&Value) -> DatabaseResult<Value>,
}

impl EnumCast {
    pub const fn of<E: CastableEnum>() -> Self {
        Self {
            type_id: TypeId::of::<E>,
            normalize: |value| E::from_value(value).map(E::into_value),
        }
    }

    pub(super) fn normalize(self, value: &Value) -> DatabaseResult<Value> {
        if matches!(value, Value::Null) {
            return Ok(Value::Null);
        }
        (self.normalize)(value)
    }

    pub(super) fn check_type<E: CastableEnum>(self) -> DatabaseResult<()> {
        if (self.type_id)() == TypeId::of::<E>() {
            Ok(())
        } else {
            Err(DatabaseError::InvalidQuery("enum attribute type"))
        }
    }
}
