use crate::database::{
    DatabaseResult, Value,
    eloquent::{Attributes, Model},
};

/// Transforms model attributes in both directions.
pub trait CastsAttributes<M: Model>: Send + Sync {
    type Value;

    fn get(
        &self,
        model: &M,
        key: &str,
        value: &Value,
        attributes: &Attributes,
    ) -> DatabaseResult<Self::Value>;

    fn set(
        &self,
        model: &M,
        key: &str,
        value: &Self::Value,
        attributes: &Attributes,
    ) -> DatabaseResult<Attributes>;
}
