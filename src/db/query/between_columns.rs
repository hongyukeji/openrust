//! 列边界谓词及保留 SQL 布尔优先级的条件链。

use super::{
    Alias, Condition, Expr, ExprTrait, QueryBuilder, RawSql, Result, WhereGroup,
    validate_identifier,
};
use super::{DatabaseTransaction, Db, SelectStatement};
use sea_orm::sea_query::IntoCondition;
use std::sync::Arc;

/// 列间区间的操作数：安全列名、显式受信 SQL 或标量子查询。
///
/// 边界采用固定两个元素的数组；调用方须为子查询选择单一列。
/// 列名不会增加值绑定；子查询绑定按左操作数、下界、上界顺序保留。
/// 子查询必须使用同一 Db 句柄及同一事务；跨句柄或事务上下文明确返回错误。
/// 混合列名与表达式边界时，先逐项转换为本类型。
///
/// ```no_run
/// use openrust::db::{Db, query::{ColumnOperand, RawSql}};
/// # fn example(db: &Db) -> openrust::Result<()> {
/// let query = db.table("products")?.where_between_columns(
///     "weight",
///     [ColumnOperand::from("minimum_weight"), ColumnOperand::from(RawSql::new("100"))],
/// )?;
/// # let _ = query;
/// # Ok(())
/// # }
/// ```
pub struct ColumnOperand<'a>(Operand<'a>);

enum Operand<'a> {
    Column(String),
    Expression(Expr),
    Subquery {
        select: Box<SelectStatement>,
        db: Db,
        transaction: Option<&'a DatabaseTransaction>,
        invalid_context: bool,
    },
}

impl From<&str> for ColumnOperand<'_> {
    fn from(value: &str) -> Self {
        Self(Operand::Column(value.to_owned()))
    }
}
impl From<String> for ColumnOperand<'_> {
    fn from(value: String) -> Self {
        Self(Operand::Column(value))
    }
}
impl From<RawSql> for ColumnOperand<'_> {
    fn from(value: RawSql) -> Self {
        Self(Operand::Expression(Expr::cust(value.0)))
    }
}
impl<'a> From<&QueryBuilder<'a>> for ColumnOperand<'a> {
    fn from(value: &QueryBuilder<'a>) -> Self {
        Self(Operand::Subquery {
            select: Box::new(value.scoped_select()),
            db: value.db.clone(),
            transaction: value.transaction,
            invalid_context: value.predicates.invalid_context,
        })
    }
}
impl<'a> From<QueryBuilder<'a>> for ColumnOperand<'a> {
    fn from(value: QueryBuilder<'a>) -> Self {
        Self::from(&value)
    }
}

#[derive(Clone)]
pub(super) struct QueryContext<'a> {
    pub(super) db: Db,
    pub(super) table: String,
    pub(super) transaction: Option<&'a DatabaseTransaction>,
}
impl QueryContext<'_> {
    pub(super) fn validate_same(&self, other: &Self) -> Result<()> {
        let same_transaction = match (self.transaction, other.transaction) {
            (None, None) => true,
            (Some(left), Some(right)) => std::ptr::eq(left, right),
            _ => false,
        };
        if !Arc::ptr_eq(&self.db.connections, &other.db.connections)
            || !same_transaction
            || self.table != other.table
        {
            Err(super::Error::custom(
                "query_nested_context_mismatch",
                "nested condition must retain its database, table and transaction context",
            ))
        } else {
            Ok(())
        }
    }
    fn qualifier(&self, table: &str) -> Result<String> {
        if table == self.table {
            Ok(table.to_owned())
        } else {
            self.db.table_name(table)
        }
    }
}
impl ColumnOperand<'_> {
    fn expression(self, context: &QueryContext<'_>) -> Result<(Expr, bool)> {
        match self.0 {
            Operand::Expression(expression) => Ok((expression, false)),
            Operand::Subquery {
                select,
                db,
                transaction,
                invalid_context,
            } => {
                let same_transaction = match (transaction, context.transaction) {
                    (None, None) => true,
                    (Some(left), Some(right)) => std::ptr::eq(left, right),
                    _ => false,
                };
                if invalid_context
                    || !Arc::ptr_eq(&db.connections, &context.db.connections)
                    || !same_transaction
                {
                    return Err(super::Error::custom(
                        "query_subquery_context_mismatch",
                        "列子查询必须使用同一数据库句柄和事务",
                    ));
                }
                Ok(((*select).into(), true))
            }
            Operand::Column(column) => {
                let parts = column.split('.').collect::<Vec<_>>();
                for part in &parts {
                    validate_identifier(part)?;
                }
                match parts.as_slice() {
                    [column] => Ok((Expr::col(Alias::new(*column)), false)),
                    [table, column] => Ok((
                        Expr::col((Alias::new(context.qualifier(table)?), Alias::new(*column))),
                        false,
                    )),
                    [schema, table, column] => Ok((
                        Expr::col((
                            Alias::new(context.qualifier(schema)?),
                            Alias::new(*table),
                            Alias::new(*column),
                        )),
                        false,
                    )),
                    _ => Err(super::invalid_identifier()),
                }
            }
        }
    }
}

fn predicate<'a, C: Into<ColumnOperand<'a>>, B: Into<ColumnOperand<'a>>>(
    context: &QueryContext<'_>,
    column: C,
    bounds: [B; 2],
    not: bool,
) -> Result<(Expr, bool)> {
    let [lower, upper] = bounds;
    let (column, column_subquery) = column.into().expression(context)?;
    let (lower, lower_subquery) = lower.into().expression(context)?;
    let (upper, upper_subquery) = upper.into().expression(context)?;
    Ok((
        if not {
            column.not_between(lower, upper)
        } else {
            column.between(lower, upper)
        },
        column_subquery || lower_subquery || upper_subquery,
    ))
}

/// 保留 SQL 的 AND 优先级；只有显式 `where_nested` 才固定整个括号组。
#[derive(Clone)]
pub(super) struct PredicateChain {
    branches: Vec<Condition>,
    has_subquery: bool,
    invalid_context: bool,
}
impl PredicateChain {
    pub(super) fn all() -> Self {
        Self {
            branches: Vec::new(),
            has_subquery: false,
            invalid_context: false,
        }
    }
    pub(super) fn inherit(&mut self, other: &Self) {
        self.has_subquery |= other.has_subquery;
        self.invalid_context |= other.invalid_context;
    }
    pub(super) fn rebind(
        &mut self,
        previous: Option<&DatabaseTransaction>,
        next: Option<&DatabaseTransaction>,
    ) {
        let same = match (previous, next) {
            (None, None) => true,
            (Some(left), Some(right)) => std::ptr::eq(left, right),
            _ => false,
        };
        self.invalid_context |= self.has_subquery && !same;
    }
    pub(super) fn validate_embedding(
        &self,
        source: &QueryBuilder<'_>,
        outer: &QueryBuilder<'_>,
    ) -> Result<()> {
        self.validate_context()?;
        if self.has_subquery {
            let same_transaction = match (source.transaction, outer.transaction) {
                (None, None) => true,
                (Some(left), Some(right)) => std::ptr::eq(left, right),
                _ => false,
            };
            if !Arc::ptr_eq(&source.db.connections, &outer.db.connections) || !same_transaction {
                return Err(super::Error::custom(
                    "query_subquery_context_mismatch",
                    "嵌套列子查询必须保留原数据库与事务",
                ));
            }
        }
        Ok(())
    }
    pub(super) fn validate_context(&self) -> Result<()> {
        if self.invalid_context {
            Err(super::Error::custom(
                "query_subquery_context_mismatch",
                "列子查询附着后不能改变数据库与事务上下文",
            ))
        } else {
            Ok(())
        }
    }
    pub(super) fn add(mut self, condition: impl IntoCondition) -> Self {
        let condition = condition.into_condition();
        if condition.is_empty() {
            return self;
        }
        let last = self.branches.pop().unwrap_or_else(Condition::all);
        self.branches.push(last.add(condition));
        self
    }
    pub(super) fn or(mut self, condition: impl IntoCondition) -> Self {
        let condition = condition.into_condition();
        if !condition.is_empty() {
            self.branches.push(Condition::all().add(condition));
        }
        self
    }
}
impl From<PredicateChain> for Condition {
    fn from(chain: PredicateChain) -> Self {
        if chain.branches.is_empty() {
            Condition::all()
        } else {
            chain
                .branches
                .into_iter()
                .fold(Condition::any(), Condition::add)
        }
    }
}

impl QueryBuilder<'_> {
    pub(super) fn query_context(&self) -> QueryContext<'_> {
        QueryContext {
            db: self.db.clone(),
            table: self.table.clone(),
            transaction: self.transaction,
        }
    }

    /// 以 AND 追加列值位于同一行两列闭区间的条件；NULL 遵循 SQL 三值逻辑。
    ///
    /// 两个边界固定为数组；列名逐段引用，RawSql 必须来自可信代码；动态值通过子查询原有绑定保留。
    pub fn where_between_columns<'a, C: Into<ColumnOperand<'a>>, B: Into<ColumnOperand<'a>>>(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self> {
        let (predicate, has_subquery) = predicate(&self.query_context(), column, bounds, false)?;
        self.predicates.has_subquery |= has_subquery;
        self.predicates = self.predicates.add(predicate);
        Ok(self)
    }
    /// 以 AND 追加列值不在同一行两列闭区间的条件；NULL 遵循 SQL 三值逻辑。
    ///
    /// 两个边界固定为数组；列名逐段引用，RawSql 必须来自可信代码；动态值通过子查询原有绑定保留。
    pub fn where_not_between_columns<'a, C: Into<ColumnOperand<'a>>, B: Into<ColumnOperand<'a>>>(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self> {
        let (predicate, has_subquery) = predicate(&self.query_context(), column, bounds, true)?;
        self.predicates.has_subquery |= has_subquery;
        self.predicates = self.predicates.add(predicate);
        Ok(self)
    }
    /// 以 OR 追加列值位于同一行两列闭区间的条件；NULL 遵循 SQL 三值逻辑。
    ///
    /// 两个边界固定为数组；列名逐段引用，RawSql 必须来自可信代码；动态值通过子查询原有绑定保留。
    pub fn or_where_between_columns<'a, C: Into<ColumnOperand<'a>>, B: Into<ColumnOperand<'a>>>(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self> {
        let (predicate, has_subquery) = predicate(&self.query_context(), column, bounds, false)?;
        self.predicates.has_subquery |= has_subquery;
        self.predicates = self.predicates.or(predicate);
        Ok(self)
    }
    /// 以 OR 追加列值不在同一行两列闭区间的条件；NULL 遵循 SQL 三值逻辑。
    ///
    /// 两个边界固定为数组；列名逐段引用，RawSql 必须来自可信代码；动态值通过子查询原有绑定保留。
    pub fn or_where_not_between_columns<
        'a,
        C: Into<ColumnOperand<'a>>,
        B: Into<ColumnOperand<'a>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self> {
        let (predicate, has_subquery) = predicate(&self.query_context(), column, bounds, true)?;
        self.predicates.has_subquery |= has_subquery;
        self.predicates = self.predicates.or(predicate);
        Ok(self)
    }
}

impl WhereGroup<'_> {
    /// 以 AND 追加列值位于同一行两列闭区间的条件；NULL 遵循 SQL 三值逻辑。
    ///
    /// 两个边界固定为数组；列名逐段引用，RawSql 必须来自可信代码；动态值通过子查询原有绑定保留。
    pub fn where_between_columns<'a, C: Into<ColumnOperand<'a>>, B: Into<ColumnOperand<'a>>>(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self> {
        let (predicate, has_subquery) = predicate(&self.context, column, bounds, false)?;
        self.condition.has_subquery |= has_subquery;
        self.condition = self.condition.add(predicate);
        Ok(self)
    }
    /// 以 AND 追加列值不在同一行两列闭区间的条件；NULL 遵循 SQL 三值逻辑。
    ///
    /// 两个边界固定为数组；列名逐段引用，RawSql 必须来自可信代码；动态值通过子查询原有绑定保留。
    pub fn where_not_between_columns<'a, C: Into<ColumnOperand<'a>>, B: Into<ColumnOperand<'a>>>(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self> {
        let (predicate, has_subquery) = predicate(&self.context, column, bounds, true)?;
        self.condition.has_subquery |= has_subquery;
        self.condition = self.condition.add(predicate);
        Ok(self)
    }
    /// 以 OR 追加列值位于同一行两列闭区间的条件；NULL 遵循 SQL 三值逻辑。
    ///
    /// 两个边界固定为数组；列名逐段引用，RawSql 必须来自可信代码；动态值通过子查询原有绑定保留。
    pub fn or_where_between_columns<'a, C: Into<ColumnOperand<'a>>, B: Into<ColumnOperand<'a>>>(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self> {
        let (predicate, has_subquery) = predicate(&self.context, column, bounds, false)?;
        self.condition.has_subquery |= has_subquery;
        self.condition = self.condition.or(predicate);
        Ok(self)
    }
    /// 以 OR 追加列值不在同一行两列闭区间的条件；NULL 遵循 SQL 三值逻辑。
    ///
    /// 两个边界固定为数组；列名逐段引用，RawSql 必须来自可信代码；动态值通过子查询原有绑定保留。
    pub fn or_where_not_between_columns<
        'a,
        C: Into<ColumnOperand<'a>>,
        B: Into<ColumnOperand<'a>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self> {
        let (predicate, has_subquery) = predicate(&self.context, column, bounds, true)?;
        self.condition.has_subquery |= has_subquery;
        self.condition = self.condition.or(predicate);
        Ok(self)
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "between_columns_tests.rs"]
mod tests;
