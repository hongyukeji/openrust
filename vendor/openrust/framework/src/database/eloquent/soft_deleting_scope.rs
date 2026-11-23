use crate::database::DatabaseResult;

use super::{Builder, Model, Scope};

/// 排除已软删除模型的默认全局作用域。
pub struct SoftDeletingScope;

impl<M: Model> Scope<M> for SoftDeletingScope {
    fn apply(&self, builder: &mut Builder<'_, M>) -> DatabaseResult<()> {
        if let Some(column) = M::metadata().deleted_at {
            builder.where_null(&builder.qualify_column(column));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
pub(super) enum Trashed {
    #[default]
    Exclude,
    Without,
    Include,
    Only,
}
