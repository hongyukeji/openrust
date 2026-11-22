use crate::database::DatabaseResult;

use super::{Builder, Model};

/// Applies a named constraint to a model query.
pub trait Scope<M: Model>: Send + Sync {
    fn apply(&self, builder: &mut Builder<'_, M>) -> DatabaseResult<()>;

    /// 注册作用域时扩展查询构造器；默认无需额外装配。
    fn extend(&self, _builder: &mut Builder<'_, M>) -> DatabaseResult<()> {
        Ok(())
    }
}

impl<M, F> Scope<M> for F
where
    M: Model,
    F: Fn(&mut Builder<'_, M>) -> DatabaseResult<()> + Send + Sync,
{
    fn apply(&self, builder: &mut Builder<'_, M>) -> DatabaseResult<()> {
        self(builder)
    }
}

pub(super) enum ScopeEntry<M: Model> {
    Static(&'static dyn Scope<M>),
    Owned(std::sync::Arc<dyn Scope<M>>),
}

impl<M: Model> Clone for ScopeEntry<M> {
    fn clone(&self) -> Self {
        match self {
            Self::Static(scope) => Self::Static(*scope),
            Self::Owned(scope) => Self::Owned(scope.clone()),
        }
    }
}

impl<M: Model> ScopeEntry<M> {
    pub(super) fn get(&self) -> &dyn Scope<M> {
        match self {
            Self::Static(scope) => *scope,
            Self::Owned(scope) => scope.as_ref(),
        }
    }
}
