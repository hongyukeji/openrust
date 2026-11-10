use crate::database::{DatabaseResult, eloquent::Model};

/// Selects a caster for a model attribute type.
pub trait Castable<M: Model>: Sized {
    type Caster: super::CastsAttributes<M, Value = Self>;

    fn cast_using(arguments: &[String]) -> DatabaseResult<Self::Caster>;
}
