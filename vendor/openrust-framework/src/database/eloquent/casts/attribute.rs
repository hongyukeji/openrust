use crate::database::{DatabaseResult, Value, eloquent::Attributes};

pub type GetAttribute = fn(Option<&Value>, &Attributes) -> DatabaseResult<Value>;
pub type SetAttribute = fn(Value, &Attributes) -> DatabaseResult<Attributes>;

/// A model attribute with an accessor, mutator, or both.
pub struct Attribute {
    pub(crate) get: Option<GetAttribute>,
    pub(crate) set: Option<SetAttribute>,
}

impl Attribute {
    pub const fn make(get: Option<GetAttribute>, set: Option<SetAttribute>) -> Self {
        Self { get, set }
    }

    pub const fn get(accessor: GetAttribute) -> Self {
        Self::make(Some(accessor), None)
    }

    pub const fn set(mutator: SetAttribute) -> Self {
        Self::make(None, Some(mutator))
    }
}
