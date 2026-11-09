//! 列间区间谓词与保留生命期的动态布尔树。

use super::{
    Alias, Condition, DatabaseTransaction, Db, DbBackend, Error, Expr, ExprTrait, QueryBuilder,
    QueryState, RawSql, Result, SelectStatement, WhereGroup, subqueries::DeferredChild,
    validate_identifier,
};
use std::sync::Arc;

/// 列间区间操作数；支持受信列名、原样 SQL 与立即准备的标量子查询。
///
/// 两个边界使用固定数组；混合输入先逐项转换为本类型。子查询接入时消费其回调。
/// ```no_run
/// use openrust::database::{Db, query::{ColumnOperand, RawSql}};
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
    Scalar(Result<Box<FrozenScalar<'a>>>),
}
struct FrozenScalar<'a> {
    sql: SelectStatement,
    bindings: SelectStatement,
    context: QueryContext<'a>,
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
impl<'tx: 'capture, 'capture> From<&QueryBuilder<'tx, 'capture>> for ColumnOperand<'tx> {
    fn from(query: &QueryBuilder<'tx, 'capture>) -> Self {
        Self(Operand::Scalar((|| {
            let (sql, bindings) = query.prepare_scalar()?;
            Ok(Box::new(FrozenScalar {
                sql,
                bindings,
                context: query.query_context()?,
            }))
        })()))
    }
}
impl<'tx: 'capture, 'capture> From<QueryBuilder<'tx, 'capture>> for ColumnOperand<'tx> {
    fn from(query: QueryBuilder<'tx, 'capture>) -> Self {
        Self::from(&query)
    }
}

#[derive(Clone)]
pub(super) struct QueryContext<'a> {
    pub(super) db: Db,
    pub(super) table: String,
    pub(super) transaction: Option<&'a DatabaseTransaction>,
}
impl QueryContext<'_> {
    pub(super) fn validate_same(&self, other: &QueryContext<'_>) -> Result<()> {
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
    pub(super) fn validate_connection(&self, other: &QueryContext<'_>) -> Result<()> {
        let same_transaction = match (self.transaction, other.transaction) {
            (None, None) => true,
            (Some(left), Some(right)) => std::ptr::eq(left, right),
            _ => false,
        };
        if !Arc::ptr_eq(&self.db.connections, &other.db.connections) || !same_transaction {
            Err(Error::custom(
                "query_subquery_context_mismatch",
                "子查询必须使用同一数据库句柄与实际事务",
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

impl<'a> ColumnOperand<'a> {
    pub(super) fn ready(self, context: &QueryContext<'_>) -> Result<Self> {
        match self.0 {
            Operand::Scalar(scalar) => {
                let scalar = scalar?;
                scalar.context.validate_connection(context)?;
                Ok(Self(Operand::Scalar(Ok(scalar))))
            }
            Operand::Column(name) => {
                let field = name
                    .split("->")
                    .next()
                    .ok_or_else(super::invalid_identifier)?;
                ColumnOperand::from(field).expressions(context)?;
                Ok(Self(Operand::Column(name)))
            }
            other @ Operand::Expression(_) => Ok(Self(other)),
        }
    }
    pub(super) fn column_name(&self) -> Option<&str> {
        match &self.0 {
            Operand::Column(name) => Some(name),
            _ => None,
        }
    }
    pub(super) fn contains_json_arrow(&self, context: &QueryContext<'_>) -> Result<bool> {
        Ok(match &self.0 {
            Operand::Column(name) => name.contains("->"),
            Operand::Expression(Expr::Custom(sql)) => sql.contains("->"),
            Operand::Scalar(Ok(scalar)) => {
                let sql = match context.db.backend()? {
                    super::DbBackend::MySql => scalar.sql.build(super::MysqlQueryBuilder).0,
                    super::DbBackend::Postgres => scalar.sql.build(super::PostgresQueryBuilder).0,
                    super::DbBackend::Sqlite => scalar.sql.build(super::SqliteQueryBuilder).0,
                    _ => return Err(super::unsupported_backend()),
                };
                sql.contains("->")
            }
            _ => false,
        })
    }
    pub(super) fn raw_text(&self) -> Option<&str> {
        match &self.0 {
            Operand::Expression(Expr::Custom(sql)) => Some(sql),
            _ => None,
        }
    }
    pub(super) fn expressions(
        self,
        context: &QueryContext<'_>,
    ) -> Result<(Expr, Expr, Option<QueryContext<'a>>)> {
        match self.0 {
            Operand::Expression(expression) => Ok((expression.clone(), expression, None)),
            Operand::Scalar(scalar) => {
                let scalar = scalar?;
                scalar.context.validate_connection(context)?;
                Ok((
                    scalar.sql.into(),
                    scalar.bindings.into(),
                    Some(scalar.context),
                ))
            }
            Operand::Column(column) => {
                let parts = column.split('.').collect::<Vec<_>>();
                for part in &parts {
                    validate_identifier(part)?;
                }
                let expression = match parts.as_slice() {
                    [column] => Expr::col(Alias::new(*column)),
                    [table, column] => {
                        Expr::col((Alias::new(context.qualifier(table)?), Alias::new(*column)))
                    }
                    [schema, table, column] => Expr::col((
                        Alias::new(context.qualifier(schema)?),
                        Alias::new(*table),
                        Alias::new(*column),
                    )),
                    _ => return Err(super::invalid_identifier()),
                };
                Ok((expression.clone(), expression, None))
            }
        }
    }
}

#[derive(Clone)]
enum Term<'a> {
    Plain(Condition),
    Raw(Expr),
    Frozen {
        sql: Expr,
        bindings: Expr,
        origins: Vec<QueryContext<'a>>,
    },
    ComparisonSub {
        sql: Expr,
        bindings: Expr,
        operator: &'static str,
        child: Box<DeferredChild<'a>>,
        origins: Vec<QueryContext<'a>>,
    },
    Nested {
        tree: PredicateChain<'a>,
        context: QueryContext<'a>,
        not: bool,
    },
    Exists(Box<DeferredChild<'a>>),
}

impl Term<'_> {
    fn compile(&self, context: &QueryContext<'_>, bindings: bool) -> Result<Condition> {
        Ok(match self {
            Self::Plain(condition) => condition.clone(),
            Self::Raw(expression) => expression.clone().into(),
            Self::Frozen {
                sql,
                bindings: frozen,
                origins,
            } => {
                for origin in origins {
                    origin.validate_connection(context)?;
                }
                if bindings {
                    frozen.clone()
                } else {
                    sql.clone()
                }
                .into()
            }
            Self::ComparisonSub {
                sql,
                bindings: frozen,
                operator,
                child,
                origins,
            } => {
                for origin in origins {
                    origin.validate_connection(context)?;
                }
                child.validate(context)?;
                let left = if bindings {
                    frozen.clone()
                } else {
                    sql.clone()
                };
                left.binary(
                    sea_orm::sea_query::BinOper::Custom(operator),
                    Expr::from(child.prepare(bindings, false)?),
                )
                .into()
            }
            Self::Nested {
                tree,
                context: origin,
                not,
            } => {
                if tree.has_subquery {
                    origin.validate_same(context)?;
                }
                let nested = tree.compile(context, bindings)?;
                if *not { nested.not() } else { nested }
            }
            Self::Exists(child) => {
                child.validate(context)?;
                Expr::exists(child.prepare(bindings, false)?).into()
            }
        })
    }
}

#[derive(Clone)]
pub(super) struct PredicateChain<'a> {
    branches: Vec<Vec<Term<'a>>>,
    pub(super) has_subquery: bool,
    pub(super) invalid_context: bool,
}

impl<'a> PredicateChain<'a> {
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
    pub(super) fn validate_context(&self) -> Result<()> {
        if self.invalid_context {
            Err(Error::custom(
                "query_subquery_context_mismatch",
                "子查询附着后不能改变数据库或事务",
            ))
        } else {
            Ok(())
        }
    }
    fn push(mut self, term: Term<'a>, or: bool) -> Self {
        if or || self.branches.is_empty() {
            self.branches.push(Vec::new());
        }
        if let Some(branch) = self.branches.last_mut() {
            branch.push(term);
        }
        self
    }
    pub(super) fn add(self, condition: impl Into<Condition>) -> Self {
        let condition = condition.into();
        if condition.is_empty() {
            self
        } else {
            self.push(Term::Plain(condition), false)
        }
    }
    pub(super) fn add_raw(self, expression: Expr, or: bool) -> Self {
        self.push(Term::Raw(expression), or)
    }
    pub(super) fn add_nested(self, tree: Self, context: QueryContext<'a>) -> Self {
        if tree.branches.is_empty() {
            self
        } else {
            self.push(
                Term::Nested {
                    tree,
                    context,
                    not: false,
                },
                false,
            )
        }
    }
    pub(super) fn add_group(
        mut self,
        tree: Self,
        context: QueryContext<'a>,
        or: bool,
        not: bool,
    ) -> Self {
        if tree.branches.is_empty() {
            return self;
        }
        self.inherit(&tree);
        self.push(Term::Nested { tree, context, not }, or)
    }
    pub(super) fn add_exists(mut self, child: DeferredChild<'a>) -> Self {
        self.has_subquery = true;
        self.push(Term::Exists(Box::new(child)), false)
    }
    pub(super) fn validate_sources(&self, context: &QueryContext<'_>) -> Result<()> {
        self.validate_context()?;
        for branch in &self.branches {
            for term in branch {
                match term {
                    Term::Frozen { origins, .. } | Term::ComparisonSub { origins, .. } => {
                        for origin in origins {
                            origin.validate_connection(context)?;
                        }
                        if let Term::ComparisonSub { child, .. } = term {
                            child.validate(context)?;
                        }
                    }
                    Term::Nested {
                        tree,
                        context: original,
                        ..
                    } => {
                        if tree.has_subquery {
                            original.validate_same(context)?;
                        }
                        tree.validate_sources(context)?;
                    }
                    Term::Exists(child) => child.validate(context)?,
                    Term::Plain(_) | Term::Raw(_) => {}
                }
            }
        }
        Ok(())
    }
    pub(super) fn reaches(
        &self,
        target: usize,
        visited: &mut std::collections::HashSet<usize>,
    ) -> Result<bool> {
        for branch in &self.branches {
            for term in branch {
                let reaches = match term {
                    Term::Exists(child) | Term::ComparisonSub { child, .. } => {
                        child.reaches(target, visited)?
                    }
                    Term::Nested { tree, .. } => tree.reaches(target, visited)?,
                    _ => false,
                };
                if reaches {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    pub(super) fn compile(&self, context: &QueryContext<'_>, bindings: bool) -> Result<Condition> {
        self.validate_context()?;
        if self.branches.is_empty() {
            return Ok(Condition::all());
        }
        if self.contains_raw() {
            return self.compile_with_raw(context, bindings);
        }
        let mut result = Condition::any();
        for branch in &self.branches {
            let mut conditions = Condition::all();
            for term in branch {
                conditions = conditions.add(term.compile(context, bindings)?);
            }
            result = result.add(conditions);
        }
        Ok(result)
    }

    fn contains_raw(&self) -> bool {
        self.branches.iter().flatten().any(|term| match term {
            Term::Raw(_) => true,
            Term::Nested { tree, .. } => tree.contains_raw(),
            _ => false,
        })
    }

    fn compile_with_raw(&self, context: &QueryContext<'_>, bindings: bool) -> Result<Condition> {
        let postgres = context.db.backend()? == DbBackend::Postgres;
        let mut template = String::new();
        let mut expressions = Vec::new();
        for branch in &self.branches {
            for (index, term) in branch.iter().enumerate() {
                if !expressions.is_empty() {
                    template.push_str(if index == 0 { " OR " } else { " AND " });
                }
                expressions.push(Expr::from(term.compile(context, bindings)?));
                let slot = if postgres {
                    format!("${}", expressions.len())
                } else {
                    "?".to_owned()
                };
                if matches!(term, Term::Raw(_)) {
                    template.push_str(&slot);
                    // 保留原始布尔优先级，并阻止末尾行注释吞掉后续条件或组右括号。
                    template.push('\n');
                } else {
                    // 普通谓词仍是原有表达式，显式条件组保留括号边界。
                    template.push('(');
                    template.push_str(&slot);
                    template.push(')');
                }
            }
        }
        Ok(Expr::cust_with_exprs(template, expressions).into())
    }
    pub(super) fn add_comparison_sub(
        mut self,
        sql: Expr,
        bindings: Expr,
        operator: &'static str,
        child: DeferredChild<'a>,
        origins: Vec<QueryContext<'a>>,
        or: bool,
    ) -> Self {
        self.has_subquery = true;
        self.push(
            Term::ComparisonSub {
                sql,
                bindings,
                operator,
                child: Box::new(child),
                origins,
            },
            or,
        )
    }
    pub(super) fn add_comparison(
        mut self,
        sql: Expr,
        bindings: Expr,
        origins: Vec<QueryContext<'a>>,
        or: bool,
    ) -> Self {
        self.has_subquery |= !origins.is_empty();
        self.push(
            Term::Frozen {
                sql,
                bindings,
                origins,
            },
            or,
        )
    }
    fn between<'operand, C: Into<ColumnOperand<'operand>>, B: Into<ColumnOperand<'operand>>>(
        mut self,
        context: &QueryContext<'_>,
        column: C,
        bounds: [B; 2],
        not: bool,
        or: bool,
    ) -> Result<Self>
    where
        'operand: 'a,
    {
        let [lower, upper] = bounds;
        let (column_sql, column_bind, column_origin) = column.into().expressions(context)?;
        let (lower_sql, lower_bind, lower_origin) = lower.into().expressions(context)?;
        let (upper_sql, upper_bind, upper_origin) = upper.into().expressions(context)?;
        let origins = [column_origin, lower_origin, upper_origin]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        self.has_subquery |= !origins.is_empty();
        let sql = if not {
            column_sql.not_between(lower_sql, upper_sql)
        } else {
            column_sql.between(lower_sql, upper_sql)
        };
        let bindings = if not {
            column_bind.not_between(lower_bind, upper_bind)
        } else {
            column_bind.between(lower_bind, upper_bind)
        };
        Ok(self.push(
            Term::Frozen {
                sql,
                bindings,
                origins,
            },
            or,
        ))
    }
}

impl<'tx: 'cap, 'cap> QueryState<'tx, 'cap> {
    /// 以AND添加列间闭区间谓词；NULL沿用数据库三值逻辑。
    pub fn where_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        let context = self.context();
        self.predicates = self
            .predicates
            .between(&context, column, bounds, false, false)?;
        Ok(self)
    }
    /// 以AND添加列间非闭区间谓词；NULL沿用数据库三值逻辑。
    pub fn where_not_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        let context = self.context();
        self.predicates = self
            .predicates
            .between(&context, column, bounds, true, false)?;
        Ok(self)
    }
    /// 以OR添加列间闭区间谓词；NULL沿用数据库三值逻辑。
    pub fn or_where_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        let context = self.context();
        self.predicates = self
            .predicates
            .between(&context, column, bounds, false, true)?;
        Ok(self)
    }
    /// 以OR添加列间非闭区间谓词；NULL沿用数据库三值逻辑。
    pub fn or_where_not_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        let context = self.context();
        self.predicates = self
            .predicates
            .between(&context, column, bounds, true, true)?;
        Ok(self)
    }
}

impl<'tx: 'cap, 'cap> WhereGroup<'tx, 'cap> {
    /// 以AND添加列间闭区间谓词；NULL沿用数据库三值逻辑。
    pub fn where_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        let context = self.context.clone();
        self.condition = self
            .condition
            .between(&context, column, bounds, false, false)?;
        Ok(self)
    }
    /// 以AND添加列间非闭区间谓词；NULL沿用数据库三值逻辑。
    pub fn where_not_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        let context = self.context.clone();
        self.condition = self
            .condition
            .between(&context, column, bounds, true, false)?;
        Ok(self)
    }
    /// 以OR添加列间闭区间谓词；NULL沿用数据库三值逻辑。
    pub fn or_where_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        let context = self.context.clone();
        self.condition = self
            .condition
            .between(&context, column, bounds, false, true)?;
        Ok(self)
    }
    /// 以OR添加列间非闭区间谓词；NULL沿用数据库三值逻辑。
    pub fn or_where_not_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        mut self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        let context = self.context.clone();
        self.condition = self
            .condition
            .between(&context, column, bounds, true, true)?;
        Ok(self)
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "between_columns_tests.rs"]
mod tests;
