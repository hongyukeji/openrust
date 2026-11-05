//! 共享子查询节点与接入时冻结的绑定计划。

use super::{Error, QueryBuilder, QueryContext, Result, SelectStatement};
use std::sync::Arc;

pub(super) trait ChildQuery: Send + Sync {
    fn prepare(&self, apply_callbacks: bool) -> Result<SelectStatement>;
    fn validate_source(&self) -> Result<()>;
    fn reaches(
        &self,
        target: usize,
        visited: &mut std::collections::HashSet<usize>,
    ) -> Result<bool>;
}

struct SharedChild<'src: 'capture, 'capture> {
    query: QueryBuilder<'src, 'capture>,
    original: QueryContext<'src>,
}

impl ChildQuery for SharedChild<'_, '_> {
    fn validate_source(&self) -> Result<()> {
        self.original.validate_same(&self.query.query_context()?)?;
        self.query.validate_current()
    }
    fn reaches(
        &self,
        target: usize,
        visited: &mut std::collections::HashSet<usize>,
    ) -> Result<bool> {
        self.query.reaches(target, visited)
    }
    fn prepare(&self, apply_callbacks: bool) -> Result<SelectStatement> {
        self.original.validate_same(&self.query.query_context()?)?;
        let select = self.query.prepare_child(apply_callbacks)?;
        self.original.validate_same(&self.query.query_context()?)?;
        Ok(select)
    }
}

#[derive(Clone)]
pub(super) struct DeferredChild<'a> {
    node: Arc<dyn ChildQuery + 'a>,
    captured: SelectStatement,
    original: QueryContext<'a>,
}

impl<'a> DeferredChild<'a> {
    pub(super) fn capture<'src, 'capture>(
        query: &QueryBuilder<'src, 'capture>,
        apply_callbacks: bool,
    ) -> Result<Self>
    where
        'src: 'capture + 'a,
        'capture: 'a,
    {
        if apply_callbacks {
            query.apply_before_query_callbacks()?;
        }
        let original = query.query_context()?;
        let captured = query.bindings_select()?;
        Ok(Self {
            node: Arc::new(SharedChild {
                query: query.alias(),
                original: original.clone(),
            }),
            captured,
            original,
        })
    }

    pub(super) fn reaches(
        &self,
        target: usize,
        visited: &mut std::collections::HashSet<usize>,
    ) -> Result<bool> {
        self.node.reaches(target, visited)
    }

    pub(super) fn validate(&self, outer: &QueryContext<'_>) -> Result<()> {
        self.original.validate_connection(outer)?;
        self.node.validate_source()
    }

    pub(super) fn prepare(&self, bindings: bool, apply_callbacks: bool) -> Result<SelectStatement> {
        self.node.validate_source()?;
        if bindings {
            Ok(self.captured.clone())
        } else {
            self.node.prepare(apply_callbacks)
        }
    }
}

#[derive(Clone)]
pub(super) struct DeferredUnion<'a> {
    pub(super) child: DeferredChild<'a>,
    pub(super) all: bool,
}

pub(super) fn busy_error() -> Error {
    Error::custom(
        "query_subquery_busy",
        "查询对象正在准备，不能重入或并发编译",
    )
}
