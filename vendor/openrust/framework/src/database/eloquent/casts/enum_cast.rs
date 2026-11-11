use std::any::TypeId;

use crate::database::{DatabaseError, DatabaseResult, Value};

/// Maps an application enum to its database backing value or case name.
#[allow(non_snake_case, reason = "Matches Laravel's cast naming")]
pub trait CastableEnum: Sized + 'static {
    fn fromValue(value: &Value) -> DatabaseResult<Self>;

    fn intoValue(self) -> Value;
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
            normalize: |value| E::fromValue(value).map(E::intoValue),
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
