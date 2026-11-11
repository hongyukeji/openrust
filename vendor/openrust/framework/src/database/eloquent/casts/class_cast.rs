use std::any::Any;

use crate::database::{DatabaseError, DatabaseResult, Value, eloquent::Attributes};

use super::AttributeCast;

/// Converts an input into a single stored value or a patch for several columns.
pub enum CastWrite {
    Value(Value),
    Patch(Attributes),
}

impl CastWrite {
    pub(in crate::database::eloquent) fn apply(self, attributes: &mut Attributes, name: String) {
        match self {
            Self::Value(value) => {
                attributes.insert(name, value);
            }
            Self::Patch(values) => attributes.extend(values),
        }
    }
}

/// Converts an attribute on assignment.
pub trait CastsInboundAttributes: Send + Sync {
    fn set(&self, name: &str, value: Value, attributes: &Attributes) -> DatabaseResult<CastWrite>;

    /// Optionally defines equivalence for the current and original stored values.
    fn compare(
        &self,
        _name: &str,
        _original: &Value,
        _current: &Value,
    ) -> DatabaseResult<Option<bool>> {
        Ok(None)
    }
}

/// Converts an attribute on read and assignment.
pub trait CastsAttributes: CastsInboundAttributes {
    fn get(
        &self,
        name: &str,
        value: Option<&Value>,
        attributes: &Attributes,
    ) -> DatabaseResult<Value>;

    /// Projects the converted value into the model's array representation.
    fn serialize(
        &self,
        _name: &str,
        value: Value,
        _attributes: &Attributes,
    ) -> DatabaseResult<Value> {
        Ok(value)
    }
}

/// Constructs a caster from an attribute's declared arguments.
#[allow(non_snake_case, reason = "Matches Laravel's Castable contract")]
pub trait Castable {
    type Arguments: Send + Sync + 'static;
    type Caster: CastsInboundAttributes + 'static;

    fn castUsing(arguments: &Self::Arguments) -> DatabaseResult<Self::Caster>;
}

type Arguments = dyn Any + Send + Sync;
type Resolve = fn(&Arguments) -> DatabaseResult<ResolvedClassCast>;

/// A model class cast with explicitly typed arguments.
#[derive(Clone, Copy)]
pub struct ClassCast {
    arguments: &'static Arguments,
    resolve: Resolve,
}

impl ClassCast {
    pub const fn of<C: Castable>(arguments: &'static C::Arguments) -> AttributeCast
    where
        C::Caster: CastsAttributes,
    {
        AttributeCast::Class(Self {
            arguments,
            resolve: |arguments| {
                Ok(ResolvedClassCast::TwoWay(Box::new(resolve::<C>(
                    arguments,
                )?)))
            },
        })
    }

    pub const fn inbound<C: Castable>(arguments: &'static C::Arguments) -> AttributeCast {
        AttributeCast::Class(Self {
            arguments,
            resolve: |arguments| {
                Ok(ResolvedClassCast::Inbound(Box::new(resolve::<C>(
                    arguments,
                )?)))
            },
        })
    }

    pub(super) fn resolve(self) -> DatabaseResult<ResolvedClassCast> {
        (self.resolve)(self.arguments)
    }
}

fn resolve<C: Castable>(arguments: &Arguments) -> DatabaseResult<C::Caster> {
    let arguments = arguments
        .downcast_ref::<C::Arguments>()
        .ok_or(DatabaseError::InvalidQuery("class cast arguments"))?;
    C::castUsing(arguments)
}

pub(super) enum ResolvedClassCast {
    TwoWay(Box<dyn CastsAttributes>),
    Inbound(Box<dyn CastsInboundAttributes>),
}

impl ResolvedClassCast {
    pub(super) fn assignment(&self) -> &dyn CastsInboundAttributes {
        match self {
            Self::TwoWay(caster) => caster.as_ref(),
            Self::Inbound(caster) => caster.as_ref(),
        }
    }

    pub(super) fn read(
        &self,
        name: &str,
        value: Option<&Value>,
        attributes: &Attributes,
    ) -> DatabaseResult<Option<Value>> {
        match self {
            Self::TwoWay(caster) => caster.get(name, value, attributes).map(Some),
            Self::Inbound(_) => Ok(value.cloned()),
        }
    }

    pub(super) fn serialize(
        &self,
        name: &str,
        value: Value,
        attributes: &Attributes,
    ) -> DatabaseResult<Value> {
        match self {
            Self::TwoWay(caster) => caster.serialize(name, value, attributes),
            Self::Inbound(_) => Ok(value),
        }
    }
}
