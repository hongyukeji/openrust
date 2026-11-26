use crate::database::{
    DatabaseResult,
    eloquent::{Attributes, Model},
};

/// Transforms an attribute only when assigning it to a model.
pub trait CastsInboundAttributes<M: Model>: Send + Sync {
    type Value;

    fn set(
        &self,
        model: &M,
        key: &str,
        value: &Self::Value,
        attributes: &Attributes,
    ) -> DatabaseResult<Attributes>;
}
