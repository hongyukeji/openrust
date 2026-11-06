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
            let current = self.node.prepare(apply_callbacks)?;
            let backend = self.original.db.backend()?;
            if binding_count(&current, backend)? != binding_count(&self.captured, backend)? {
                return Err(Error::custom(
                    "query_subquery_bindings_mismatch",
                    "实时子查询SQL与接入时冻结绑定数量不一致",
                ));
            }
            Ok(current)
        }
    }
}

// 每个节点单独核验，不能让两个兄弟的+1/-1变化在父总数中相互抵消。
fn binding_count(select: &SelectStatement, backend: super::DbBackend) -> Result<usize> {
    let values = match backend {
        super::DbBackend::MySql => select.build(super::MysqlQueryBuilder).1,
        super::DbBackend::Postgres => select.build(super::PostgresQueryBuilder).1,
        super::DbBackend::Sqlite => select.build(super::SqliteQueryBuilder).1,
        _ => return Err(super::unsupported_backend()),
    };
    Ok(values.0.len())
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

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "subqueries_contract_tests.rs"]
mod contract_tests;
