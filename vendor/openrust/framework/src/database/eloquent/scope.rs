use crate::database::DatabaseResult;

use super::{Builder, Model};

/// Applies a named constraint to a model query.
pub trait Scope<M: Model>: Send + Sync {
    fn apply(&self, builder: &mut Builder<'_, M>) -> DatabaseResult<()>;
}
