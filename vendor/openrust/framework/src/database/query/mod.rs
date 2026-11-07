//! 由现有 `Db` 连接、表名解析和 `SeaQuery` 方言承载的 fluent 查询入口。

use sea_orm::sea_query::{
    Alias, Asterisk, Condition, Expr, ExprTrait, Func, InsertStatement, JoinType, LockType,
    MysqlQueryBuilder, OnConflict, Order, PostgresQueryBuilder, Query, SelectStatement,
    SqliteQueryBuilder, UnionType, Value, Values,
};
use sea_orm::{
    ConnectionTrait, DatabaseTransaction, DbBackend, ExecResult, QueryResult, Statement, TryGetable,
};
use std::future::Future;
use std::sync::{Arc, Mutex};

mod between_columns;
mod builder;
mod comparison;
mod conditional;
mod existence;
mod factory;
mod insert_id;
mod json_contains;
mod multi_column;
mod sqlite_write_target;
mod subqueries;
pub use between_columns::ColumnOperand;
use between_columns::{PredicateChain, QueryContext};
pub use comparison::{WhereOperator, WhereValue};
pub use conditional::ExistsOrResult;
pub use multi_column::{MultiColumn, MultiComparison, MultiValue};

use super::Db;
use crate::core::{Error, Result};

/// 拥有查询状态的 Laravel 风格查询；普通克隆复制父状态，共享已接入的子查询对象。
pub struct QueryBuilder<'tx: 'cap, 'cap> {
    inner: Arc<QueryInner<'tx, 'cap>>,
}

struct QueryInner<'tx: 'cap, 'cap> {
    state: Mutex<QueryState<'tx, 'cap>>,
    busy: std::sync::atomic::AtomicBool,
}

type BeforeCallback<'cap> =
    Arc<dyn for<'q> Fn(&mut QueryBuilder<'q, 'cap>) -> Result<()> + Send + Sync + 'cap>;

#[derive(Clone)]
struct QueryState<'tx: 'cap, 'cap> {
    db: Db,
    table: Option<String>,
    transaction: Option<&'tx DatabaseTransaction>,
    select: SelectStatement,
    // SeaQuery不公开join集合；此标记只随唯一状态clone/事务转移。
    has_joins: bool,
    predicates: PredicateChain<'cap>,
    callbacks: Vec<BeforeCallback<'cap>>,
    unions: Vec<subqueries::DeferredUnion<'cap>>,
    projection: Option<Vec<Expr>>,
    limit: Option<u64>,
    offset: u64,
    orders: Vec<(String, SortDirection)>,
    union_orders: Vec<(String, SortDirection)>,
    union_limit: Option<u64>,
    union_offset: Option<u64>,
    grouped: bool,
    having: bool,
    failure: Option<(String, String)>,
}

/// 查询分页结果；页面默认大小为 15，空结果的末页仍为 1。
pub struct QueryPage {
    /// 当前页的数据库行。
    pub items: Vec<QueryResult>,
    /// 当前页号。
    pub current_page: u64,
    /// 每页行数。
    pub per_page: u64,
    /// 满足查询的行数。
    pub total: u64,
    /// 最后一页，空集为 1。
    pub last_page: u64,
    path: String,
    query: Vec<(String, String)>,
    page_name: String,
    fragment: Option<String>,
}

/// 不执行总数查询的分页结果。
pub struct SimplePage {
    /// 当前页数据。
    pub items: Vec<QueryResult>,
    /// 当前页号。
    pub current_page: u64,
    /// 每页行数。
    pub per_page: u64,
    /// 是否存在下一页。
    pub has_more: bool,
    path: String,
    query: Vec<(String, String)>,
    page_name: String,
    fragment: Option<String>,
}

impl SimplePage {
    /// 当前页之前的链接。
    pub fn previous_page_url(&self) -> Option<String> {
        (self.current_page > 1).then(|| {
            page_url(
                &self.path,
                &self.query,
                &self.page_name,
                self.current_page - 1,
                self.fragment.as_deref(),
            )
        })
    }

    /// 下一页链接；没有下一页则返回 `None`。
    pub fn next_page_url(&self) -> Option<String> {
        self.has_more.then(|| {
            page_url(
                &self.path,
                &self.query,
                &self.page_name,
                self.current_page.saturating_add(1),
                self.fragment.as_deref(),
            )
        })
    }

    /// 显式保留当前请求的其他查询参数。
    #[must_use]
    pub fn with_query_string(mut self) -> Self {
        self.query = CURRENT_PAGE_CONTEXT
            .try_with(Clone::clone)
            .unwrap_or_default()
            .query;
        self
    }

    /// 显式追加已审查的链接查询参数。
    #[must_use]
    pub fn appends(mut self, parameters: impl IntoIterator<Item = (String, String)>) -> Self {
        for (key, value) in parameters {
            self.query.retain(|(existing, _)| existing != &key);
            if key != self.page_name {
                self.query.push((key, value));
            }
        }
        self
    }

    /// 为分页链接添加片段。
    #[must_use]
    pub fn fragment(mut self, fragment: impl Into<String>) -> Self {
        self.fragment = Some(fragment.into().trim_start_matches('#').to_owned());
        self
    }
}

impl QueryPage {
    /// 为指定页生成仅包含当前分页参数的链接。
    pub fn url(&self, page: u64) -> String {
        page_url(
            &self.path,
            &self.query,
            &self.page_name,
            std::cmp::Ord::max(page, 1),
            self.fragment.as_deref(),
        )
    }

    /// 当前页之前的链接。
    pub fn previous_page_url(&self) -> Option<String> {
        (self.current_page > 1).then(|| self.url(self.current_page - 1))
    }

    /// 当前页之后的链接。
    pub fn next_page_url(&self) -> Option<String> {
        (self.current_page < self.last_page).then(|| self.url(self.current_page + 1))
    }

    /// 为链接设置片段。
    #[must_use]
    pub fn fragment(mut self, fragment: impl Into<String>) -> Self {
        self.fragment = Some(fragment.into().trim_start_matches('#').to_owned());
        self
    }

    /// 显式把当前请求的全部查询参数加入链接。
    #[must_use]
    pub fn with_query_string(mut self) -> Self {
        let context = CURRENT_PAGE_CONTEXT
            .try_with(Clone::clone)
            .unwrap_or_default();
        self.query = context.query;
        self
    }

    /// 显式追加已审查的查询参数；同名参数覆盖既有值。
    #[must_use]
    pub fn appends(mut self, parameters: impl IntoIterator<Item = (String, String)>) -> Self {
        for (key, value) in parameters {
            self.query.retain(|(existing, _)| existing != &key);
            if key != self.page_name {
                self.query.push((key, value));
            }
        }
        self
    }
}

/// 当前请求的分页输入；命令和队列未安装时使用第一页。
#[derive(Clone, Default)]
pub struct PageContext {
    path: String,
    query: Vec<(String, String)>,
    input: Arc<Mutex<Option<serde_json::Value>>>,
    path_resolver: Option<Arc<dyn Fn() -> Result<String> + Send + Sync>>,
}

impl PageContext {
    /// 从请求 URI 提取路径及查询字段，不读取正文或请求头。
    pub fn from_uri(uri: &http::Uri) -> Self {
        Self {
            path: uri.path().to_owned(),
            query: uri
                .query()
                .map(|query| {
                    url::form_urlencoded::parse(query.as_bytes())
                        .map(|(key, value)| (key.into_owned(), value.into_owned()))
                        .collect()
                })
                .unwrap_or_default(),
            input: Arc::new(Mutex::new(None)),
            path_resolver: None,
        }
    }

    /// 延迟解析可信外部 URL；只在实际生成分页对象时调用。
    #[must_use]
    pub fn with_path_resolver<F>(mut self, resolver: F) -> Self
    where
        F: Fn() -> Result<String> + Send + Sync + 'static,
    {
        self.path_resolver = Some(Arc::new(resolver));
        self
    }

    fn resolved_path(&self) -> Result<String> {
        self.path_resolver
            .as_ref()
            .map_or_else(|| Ok(self.path.clone()), |resolver| resolver())
    }

    fn current_page(&self, page_name: &str) -> u64 {
        let input = self
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let from_managed = input.as_ref().and_then(|input| input.get(page_name));
        let from_query = self
            .query
            .iter()
            .rev()
            .find(|(key, _)| key == page_name)
            .map(|(_, value)| serde_json::Value::String(value.clone()));
        let page = from_managed.or(from_query.as_ref());
        page.and_then(|value| match value {
            serde_json::Value::String(text) => parse_page(text),
            serde_json::Value::Number(number) => {
                number.as_i64().and_then(|value| u64::try_from(value).ok())
            }
            _ => None,
        })
        .filter(|page| *page > 0)
        .unwrap_or(1)
    }
}

fn parse_page(value: &str) -> Option<u64> {
    if value.len() > 1 && value.starts_with('0') {
        return None;
    }
    value
        .parse::<i64>()
        .ok()
        .and_then(|page| u64::try_from(page).ok())
}

/// 受管请求解析后登记已规范化输入，使分页读取 Laravel `Request::input`。
pub(crate) fn bind_page_input(input: serde_json::Value) {
    let _ = CURRENT_PAGE_CONTEXT.try_with(|context| {
        *context
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(input);
    });
}

tokio::task_local! {
    static CURRENT_PAGE_CONTEXT: PageContext;
}

/// 在单一请求 future 中安装分页解析上下文；取消时自动释放。
pub async fn with_page_context<F: Future>(context: PageContext, future: F) -> F::Output {
    CURRENT_PAGE_CONTEXT.scope(context, future).await
}

fn page_url(
    path: &str,
    query: &[(String, String)],
    page_name: &str,
    page: u64,
    fragment: Option<&str>,
) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in query {
        if key != page_name {
            serializer.append_pair(key, value);
        }
    }
    serializer.append_pair(page_name, &page.to_string());
    let query = serializer.finish();
    let mut url = format!("{}?{query}", if path.is_empty() { "/" } else { path });
    if let Some(fragment) = fragment {
        url.push('#');
        url.push_str(fragment);
    }
    url
}

/// 用于 `where_nested` 的有括号条件组。
pub struct WhereGroup<'tx: 'cap, 'cap> {
    context: QueryContext<'tx>,
    condition: PredicateChain<'cap>,
}

/// 应用明确声明的受信 SQL 片段；值仍须另行绑定。
#[derive(Clone)]
pub struct RawSql(String);

/// 查询排序方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// 升序。
    Asc,
    /// 降序。
    Desc,
}

/// 关系连接类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinKind {
    /// 只保留满足连接条件的行。
    Inner,
    /// 保留左侧无匹配行。
    Left,
    /// 保留右侧无匹配行。
    Right,
}

impl RawSql {
    /// 构造 SQL 逃生片段；调用方必须审查标识符与语法来源。
    pub fn new(sql: impl Into<String>) -> Self {
        Self(sql.into())
    }
}

impl WhereGroup<'_, '_> {
    /// 在组内追加 AND 相等条件。
    pub fn where_eq(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.r#where(column, WhereValue::from(Into::<Value>::into(value)))
    }

    /// 在组内追加 OR 相等条件。
    pub fn or_where_eq(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.or_where_value(column, WhereValue::from(Into::<Value>::into(value)))
    }
}

impl Db {
    /// 从当前连接开启无FROM查询；显式选择表时调用`from`。
    pub fn query<'tx: 'cap, 'cap>(&self) -> QueryBuilder<'tx, 'cap> {
        QueryBuilder::from_state(QueryState::empty(self.clone(), None))
    }
    /// 从当前连接的逻辑表名开启查询。
    pub fn table<'tx: 'cap, 'cap>(&self, logical_name: &str) -> Result<QueryBuilder<'tx, 'cap>> {
        self.query().from(logical_name)
    }
}

impl<'tx: 'cap, 'cap> QueryState<'tx, 'cap> {
    /// 只选择给定列；列名不是 SQL 绑定值，必须来自应用可信代码。
    pub fn select(mut self, columns: &[&str]) -> Result<Self> {
        if columns.is_empty() {
            return Err(invalid_identifier());
        }
        self.projection = Some(
            columns
                .iter()
                .map(|column| Expr::col(Alias::new(*column)))
                .collect(),
        );
        self.select.clear_selects();
        for column in columns {
            validate_identifier(column)?;
            self.select.column(Alias::new(*column));
        }
        Ok(self)
    }

    /// 去重返回行；分页总数对去重后的完整结果计数。
    #[must_use]
    pub fn distinct(mut self) -> Self {
        self.select.distinct();
        self
    }

    /// `where column = value`；值由 `SeaQuery` 绑定。
    pub fn where_eq(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.r#where(column, WhereValue::from(Into::<Value>::into(value)))
    }

    /// `where column > value`。
    pub fn where_gt(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.where_compare(
            column,
            WhereOperator::Gt,
            WhereValue::from(Into::<Value>::into(value)),
        )
    }

    /// `where column < value`。
    pub fn where_lt(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.where_compare(
            column,
            WhereOperator::Lt,
            WhereValue::from(Into::<Value>::into(value)),
        )
    }

    /// 按 SQL LIKE 语法匹配；通配符由调用方选择，字符串仍绑定为值。
    pub fn where_like(self, column: &str, pattern: impl Into<String>) -> Result<Self> {
        self.where_compare(column, WhereOperator::Like, pattern.into())
    }

    /// 按 SQL NOT IN 匹配；空集合为永真。
    pub fn where_not_in<V, I>(mut self, column: &str, values: I) -> Result<Self>
    where
        V: Into<Value>,
        I: IntoIterator<Item = V>,
    {
        validate_identifier(column)?;
        let values = values
            .into_iter()
            .map(|value| Expr::val(value.into()))
            .collect::<Vec<_>>();
        let predicate = if values.is_empty() {
            Expr::val(1).eq(Expr::val(1))
        } else {
            Expr::col(Alias::new(column)).is_not_in(values)
        };
        self.predicates = self.predicates.add(predicate);
        Ok(self)
    }

    /// `where column is null`。
    pub fn where_null(mut self, column: &str) -> Result<Self> {
        validate_identifier(column)?;
        let predicate = Expr::col(Alias::new(column)).is_null();
        self.predicates = self.predicates.add(predicate);
        Ok(self)
    }

    /// `where column is not null`。
    pub fn where_not_null(mut self, column: &str) -> Result<Self> {
        validate_identifier(column)?;
        self.predicates = self
            .predicates
            .add(Expr::col(Alias::new(column)).is_not_null());
        Ok(self)
    }

    /// `where column between lower and upper`，边界均作为绑定值。
    pub fn where_between(
        mut self,
        column: &str,
        lower: impl Into<Value>,
        upper: impl Into<Value>,
    ) -> Result<Self> {
        validate_identifier(column)?;
        self.predicates = self.predicates.add(
            Expr::col(Alias::new(column)).between(Expr::val(lower.into()), Expr::val(upper.into())),
        );
        Ok(self)
    }

    /// 将一个 OR 条件与已有条件组合；嵌套条件保留括号。
    pub fn or_where_eq(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.or_where_value(column, WhereValue::from(Into::<Value>::into(value)))
    }

    /// 以闭包生成一个有括号的嵌套 where 组。
    pub fn where_nested<F>(mut self, configure: F) -> Result<Self>
    where
        F: FnOnce(WhereGroup<'tx, 'cap>) -> Result<WhereGroup<'tx, 'cap>>,
    {
        let group = configure(WhereGroup {
            context: QueryContext {
                db: self.db.clone(),
                table: self.table.clone().unwrap_or_default(),
                transaction: self.transaction,
            },
            condition: PredicateChain::all(),
        })?;
        group.context.validate_same(&self.context())?;
        self.predicates.inherit(&group.condition);
        self.predicates = self.predicates.add_nested(group.condition, group.context);
        Ok(self)
    }

    fn where_exists<'src, 'childcap>(
        mut self,
        subquery: &QueryBuilder<'src, 'childcap>,
    ) -> Result<Self>
    where
        'src: 'childcap + 'cap,
        'childcap: 'cap,
    {
        let node = subqueries::DeferredChild::capture(subquery, false)?;
        node.validate(&self.context())?;
        self.predicates = self.predicates.add_exists(node);
        Ok(self)
    }

    /// JSON 包含；PostgreSQL 使用 `@>`，MySQL 使用 `JSON_CONTAINS`。
    pub fn where_json_contains(mut self, column: &str, value: serde_json::Value) -> Result<Self> {
        validate_identifier(column)?;
        let predicate = json_contains::predicate(self.db.backend()?, column, value)?;
        self.predicates = self.predicates.add(predicate);
        Ok(self)
    }

    /// 显式受信 SQL where 逃生口；动态值只能通过 `bindings` 传入。
    #[must_use]
    pub fn where_raw(mut self, sql: RawSql, bindings: Vec<Value>) -> Self {
        self.predicates = self.predicates.add(Expr::cust_with_values(sql.0, bindings));
        self
    }

    /// `where column in (...)`；空集合遵 SQL 永假语义。
    pub fn where_in<V, I>(mut self, column: &str, values: I) -> Result<Self>
    where
        V: Into<Value>,
        I: IntoIterator<Item = V>,
    {
        validate_identifier(column)?;
        let values = values
            .into_iter()
            .map(|value| Expr::val(value.into()))
            .collect::<Vec<_>>();
        let predicate = if values.is_empty() {
            Expr::val(1).eq(Expr::val(2))
        } else {
            Expr::col(Alias::new(column)).is_in(values)
        };
        self.predicates = self.predicates.add(predicate);
        Ok(self)
    }

    /// 按可信列排序。
    pub fn order_by(mut self, column: &str, direction: SortDirection) -> Result<Self> {
        validate_identifier(column)?;
        if self.unions.is_empty() {
            self.orders.push((column.to_owned(), direction));
            self.select.order_by(
                Alias::new(column),
                match direction {
                    SortDirection::Asc => Order::Asc,
                    SortDirection::Desc => Order::Desc,
                },
            );
        } else {
            self.union_orders.push((column.to_owned(), direction));
        }
        Ok(self)
    }

    /// 按可信列分组。
    pub fn group_by(mut self, column: &str) -> Result<Self> {
        validate_identifier(column)?;
        self.grouped = true;
        self.select.group_by_col(Alias::new(column));
        Ok(self)
    }

    /// 在可信逻辑表之间按列相等条件连接。
    pub fn join(
        mut self,
        logical_table: &str,
        own_column: &str,
        foreign_column: &str,
        kind: JoinKind,
    ) -> Result<Self> {
        validate_identifier(own_column)?;
        validate_identifier(foreign_column)?;
        let joined = self.db.table_name(logical_table)?;
        let condition = Expr::col((Alias::new(self.required_table()?), Alias::new(own_column)))
            .equals((Alias::new(&joined), Alias::new(foreign_column)));
        self.select.join(
            match kind {
                JoinKind::Inner => JoinType::InnerJoin,
                JoinKind::Left => JoinType::LeftJoin,
                JoinKind::Right => JoinType::RightJoin,
            },
            Alias::new(&joined),
            condition,
        );
        self.has_joins = true;
        Ok(self)
    }

    fn union<'src, 'childcap>(
        mut self,
        other: &QueryBuilder<'src, 'childcap>,
        all: bool,
    ) -> Result<Self>
    where
        'src: 'childcap + 'cap,
        'childcap: 'cap,
    {
        let child = subqueries::DeferredChild::capture(other, false)?;
        child.validate(&self.context())?;
        self.predicates.has_subquery = true;
        self.unions.push(subqueries::DeferredUnion { child, all });
        Ok(self)
    }

    /// 在当前事务中获取排他行锁。
    pub fn lock_for_update(mut self) -> Result<Self> {
        if self.transaction.is_none() {
            return Err(Error::custom(
                "query_lock_requires_transaction",
                "row lock requires an active transaction",
            ));
        }
        self.select.lock(LockType::Update);
        Ok(self)
    }

    /// 对聚合列添加 `having column = value`。
    pub fn having_eq(mut self, column: &str, value: impl Into<Value>) -> Result<Self> {
        validate_identifier(column)?;
        self.having = true;
        self.select
            .and_having(Expr::col(Alias::new(column)).eq(Expr::val(value.into())));
        Ok(self)
    }

    /// 限制返回行数。
    #[must_use]
    pub fn limit(mut self, rows: u64) -> Self {
        if self.unions.is_empty() {
            self.limit = Some(rows);
            self.select.limit(rows);
        } else {
            self.union_limit = Some(rows);
        }
        self
    }

    /// 跳过指定行数。
    #[must_use]
    pub fn offset(mut self, rows: u64) -> Self {
        if self.unions.is_empty() {
            self.offset = rows;
            self.select.offset(rows);
        } else {
            self.union_offset = Some(rows);
        }
        self
    }

    /// 读取当前查询的所有行；每个 `QueryResult` 可使用类型化 `try_get`。
    pub async fn get(&self) -> Result<Vec<QueryResult>> {
        let statement = self.statement(&self.scoped_select(false)?)?;
        match self.transaction {
            Some(transaction) => transaction
                .query_all_raw(statement)
                .await
                .map_err(Error::internal),
            None => self
                .db
                .read_connection()?
                .query_all_raw(statement)
                .await
                .map_err(Error::internal),
        }
    }

    async fn count(&self) -> Result<u64> {
        let aggregate = |bindings: bool| -> Result<SelectStatement> {
            let mut select = self.scoped_select(bindings)?;
            if !self.unions.is_empty() || self.having {
                Ok(Query::select()
                    .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("total"))
                    .from_subquery(select, Alias::new("aggregate_table"))
                    .to_owned())
            } else {
                select
                    .clear_selects()
                    .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("total"));
                Ok(select)
            }
        };
        let statement = self.statement_pair(&aggregate(false)?, &aggregate(true)?)?;
        let row = match self.transaction {
            Some(transaction) => transaction.query_one_raw(statement).await,
            None => self.db.read_connection()?.query_one_raw(statement).await,
        }
        .map_err(Error::internal)?;
        let Some(row) = row else {
            return Ok(0);
        };
        let count = row.try_get::<i64>("", "total").map_err(Error::internal)?;
        u64::try_from(count).map_err(Error::internal)
    }

    /// 插入一行，返回数据库报告的受影响行数。
    pub async fn insert(&self, values: Vec<(&str, Value)>) -> Result<u64> {
        if values.is_empty() {
            return Err(empty_values());
        }
        self.execute(self.compile_insert(values, None)?).await
    }

    fn compile_insert(
        &self,
        values: Vec<(&str, Value)>,
        returning: Option<&str>,
    ) -> Result<(String, Values)> {
        let mut statement: InsertStatement = Query::insert();
        statement.into_table(Alias::new(self.required_table()?));
        if values.is_empty() {
            statement.or_default_values();
        } else {
            let mut columns = Vec::with_capacity(values.len());
            let mut expressions = Vec::with_capacity(values.len());
            for (column, value) in values {
                validate_identifier(column)?;
                columns.push(Alias::new(column));
                expressions.push(Expr::val(value));
            }
            statement
                .columns(columns)
                .values(expressions)
                .map_err(Error::internal)?;
        }
        if let Some(column) = returning {
            validate_identifier(column)?;
            statement.returning(Query::returning().exprs([
                Expr::col(Alias::new(column)),
                Expr::cust_with_expr(
                    "$1 AS \"__openrust_insert_id_text\"",
                    Expr::col(Alias::new(column)).cast_as(Alias::new("text")),
                ),
            ]));
        }
        match self.db.backend()? {
            DbBackend::Postgres => Ok(statement.build(PostgresQueryBuilder)),
            DbBackend::MySql => Ok(statement.build(MysqlQueryBuilder)),
            DbBackend::Sqlite => Ok(statement.build(SqliteQueryBuilder)),
            _ => Err(unsupported_backend()),
        }
    }

    fn validate_upsert(
        rows: &[Vec<(&str, Value)>],
        unique_by: &[&str],
        update_columns: &[&str],
    ) -> Result<()> {
        let first = rows.first().ok_or_else(empty_values)?;
        if first.is_empty() || unique_by.is_empty() || update_columns.is_empty() {
            return Err(empty_values());
        }
        let columns = first.iter().map(|(column, _)| *column).collect::<Vec<_>>();
        for column in columns
            .iter()
            .chain(unique_by.iter())
            .chain(update_columns.iter())
        {
            validate_identifier(column)?;
        }
        if columns
            .iter()
            .any(|column| columns.iter().filter(|other| *other == column).count() != 1)
            || unique_by.iter().any(|column| !columns.contains(column))
            || update_columns
                .iter()
                .any(|column| !columns.contains(column))
        {
            return Err(Error::custom(
                "query_upsert_columns_invalid",
                "upsert columns must be present and distinct",
            ));
        }
        for row in rows {
            if row.len() != columns.len()
                || row
                    .iter()
                    .zip(&columns)
                    .any(|((column, _), expected)| column != expected)
            {
                return Err(Error::custom(
                    "query_upsert_rows_mismatch",
                    "upsert rows must use the same ordered columns",
                ));
            }
        }
        Ok(())
    }

    /// 按唯一键插入或更新多行；所有行必须提供相同且同序的列。
    ///
    /// MySQL 根据表上任意唯一索引决定冲突，PostgreSQL/SQLite 使用 `unique_by`。
    /// 返回驱动报告的受影响行数，MySQL 的插入与更新计数规则可能不同。
    pub async fn upsert(
        &self,
        rows: Vec<Vec<(&str, Value)>>,
        unique_by: &[&str],
        update_columns: &[&str],
    ) -> Result<u64> {
        let first = rows.first().ok_or_else(empty_values)?;
        if first.is_empty() || unique_by.is_empty() || update_columns.is_empty() {
            return Err(empty_values());
        }
        let columns = first.iter().map(|(column, _)| *column).collect::<Vec<_>>();
        for column in columns
            .iter()
            .chain(unique_by.iter())
            .chain(update_columns.iter())
        {
            validate_identifier(column)?;
        }
        if columns
            .iter()
            .any(|column| columns.iter().filter(|other| *other == column).count() != 1)
            || unique_by.iter().any(|column| !columns.contains(column))
            || update_columns
                .iter()
                .any(|column| !columns.contains(column))
        {
            return Err(Error::custom(
                "query_upsert_columns_invalid",
                "upsert columns must be present and distinct",
            ));
        }
        let mut statement = Query::insert();
        statement
            .into_table(Alias::new(self.required_table()?))
            .columns(columns.iter().map(|column| Alias::new(*column)));
        for row in rows {
            if row.len() != columns.len()
                || row
                    .iter()
                    .zip(&columns)
                    .any(|((column, _), expected)| column != expected)
            {
                return Err(Error::custom(
                    "query_upsert_rows_mismatch",
                    "upsert rows must use the same ordered columns",
                ));
            }
            statement
                .values(row.into_iter().map(|(_, value)| Expr::val(value)))
                .map_err(Error::internal)?;
        }
        let mut conflict = OnConflict::columns(unique_by.iter().map(|column| Alias::new(*column)));
        conflict.update_columns(update_columns.iter().map(|column| Alias::new(*column)));
        statement.on_conflict(conflict);
        let compiled = match self.db.backend()? {
            DbBackend::Postgres => statement.build(PostgresQueryBuilder),
            DbBackend::MySql => statement.build(MysqlQueryBuilder),
            DbBackend::Sqlite => statement.build(SqliteQueryBuilder),
            _ => return Err(unsupported_backend()),
        };
        self.execute(compiled).await
    }

    /// 更新满足当前 where 约束的行。
    pub async fn update(&self, values: Vec<(&str, Value)>) -> Result<u64> {
        if values.is_empty() {
            return Err(empty_values());
        }
        let mut pairs = Vec::with_capacity(values.len());
        for (column, value) in values {
            validate_identifier(column)?;
            pairs.push((Alias::new(column), Expr::val(value)));
        }
        let build = |bindings: bool| -> Result<(String, Values)> {
            if self.db.backend()? == DbBackend::Sqlite {
                return self.sqlite_compile_update(&pairs, bindings);
            }
            let mut statement = Query::update();
            statement
                .table(Alias::new(self.required_table()?))
                .values(pairs.clone());
            statement.cond_where(self.predicates.compile(&self.context(), bindings)?);
            match self.db.backend()? {
                DbBackend::Postgres => Ok(statement.build(PostgresQueryBuilder)),
                DbBackend::MySql => Ok(statement.build(MysqlQueryBuilder)),
                DbBackend::Sqlite => Ok(statement.build(SqliteQueryBuilder)),
                _ => Err(unsupported_backend()),
            }
        };
        let current = build(false)?;
        let captured = build(true)?;
        if current.1.0.len() != captured.1.0.len() {
            return Err(Error::custom(
                "query_subquery_bindings_mismatch",
                "实时子查询SQL与接入时冻结绑定数量不一致",
            ));
        }
        self.execute((current.0, captured.1)).await
    }

    /// 删除满足当前 where 约束的行。
    pub async fn delete(&self) -> Result<u64> {
        let build = |bindings: bool| -> Result<(String, Values)> {
            if self.db.backend()? == DbBackend::Sqlite {
                return self.sqlite_compile_delete(bindings);
            }
            let mut statement = Query::delete();
            statement.from_table(Alias::new(self.required_table()?));
            statement.cond_where(self.predicates.compile(&self.context(), bindings)?);
            match self.db.backend()? {
                DbBackend::Postgres => Ok(statement.build(PostgresQueryBuilder)),
                DbBackend::MySql => Ok(statement.build(MysqlQueryBuilder)),
                DbBackend::Sqlite => Ok(statement.build(SqliteQueryBuilder)),
                _ => Err(unsupported_backend()),
            }
        };
        let current = build(false)?;
        let captured = build(true)?;
        if current.1.0.len() != captured.1.0.len() {
            return Err(Error::custom(
                "query_subquery_bindings_mismatch",
                "实时子查询SQL与接入时冻结绑定数量不一致",
            ));
        }
        self.execute((current.0, captured.1)).await
    }

    fn reaches(
        &self,
        target: usize,
        visited: &mut std::collections::HashSet<usize>,
    ) -> Result<bool> {
        if self.predicates.reaches(target, visited)? {
            return Ok(true);
        }
        for union in &self.unions {
            if union.child.reaches(target, visited)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn context(&self) -> QueryContext<'tx> {
        QueryContext {
            db: self.db.clone(),
            table: self.table.clone().unwrap_or_default(),
            transaction: self.transaction,
        }
    }

    fn validate(&self) -> Result<()> {
        if let Some((code, message)) = &self.failure {
            return Err(Error::custom(code, message));
        }
        self.predicates.validate_sources(&self.context())?;
        for union in &self.unions {
            union.child.validate(&self.context())?;
        }
        Ok(())
    }

    fn build(&self, select: &SelectStatement) -> Result<(String, Values)> {
        self.validate()?;
        match self.db.backend()? {
            DbBackend::Postgres => Ok(select.build(PostgresQueryBuilder)),
            DbBackend::MySql => Ok(select.build(MysqlQueryBuilder)),
            DbBackend::Sqlite => Ok(select.build(SqliteQueryBuilder)),
            _ => Err(unsupported_backend()),
        }
    }

    fn statement_pair(
        &self,
        select: &SelectStatement,
        bindings: &SelectStatement,
    ) -> Result<Statement> {
        let (sql, current_values) = self.build(select)?;
        let (_, values) = self.build(bindings)?;
        if current_values.0.len() != values.0.len() {
            return Err(Error::custom(
                "query_subquery_bindings_mismatch",
                "实时子查询SQL与接入时冻结绑定数量不一致",
            ));
        }
        Ok(Statement::from_sql_and_values(
            self.db.backend()?,
            sql,
            values.0,
        ))
    }

    fn statement(&self, select: &SelectStatement) -> Result<Statement> {
        self.statement_pair(select, &self.scoped_select(true)?)
    }

    fn clear_orders(&mut self) {
        self.select.clear_order_by();
        self.orders.clear();
    }
    fn remove_orders_for(&mut self, column: &str) {
        self.orders.retain(|(existing, _)| existing != column);
        self.select.clear_order_by();
        for (column, direction) in &self.orders {
            self.select.order_by(
                Alias::new(column),
                match direction {
                    SortDirection::Asc => Order::Asc,
                    SortDirection::Desc => Order::Desc,
                },
            );
        }
        self.union_orders.retain(|(existing, _)| existing != column);
    }

    fn restore_projection(&mut self, projection: Option<Vec<Expr>>) {
        self.select.clear_selects();
        if let Some(columns) = &projection {
            for expression in columns {
                self.select.expr(expression.clone());
            }
        } else {
            self.select.column(Asterisk);
        }
        self.projection = projection;
    }

    fn scoped_select(&self, bindings: bool) -> Result<SelectStatement> {
        self.validate()?;
        let mut select = self.select.clone();
        select.cond_where(self.predicates.compile(&self.context(), bindings)?);
        if !self.unions.is_empty()
            && (self.limit.is_some() || self.offset > 0 || !self.orders.is_empty())
        {
            let mut wrapped = Query::select();
            wrapped
                .column(Asterisk)
                .from_subquery(select, Alias::new("union_base"));
            select = wrapped;
        }
        for union in &self.unions {
            union.child.validate(&self.context())?;
            select.union(
                if union.all {
                    UnionType::All
                } else {
                    UnionType::Distinct
                },
                union.child.prepare(bindings, true)?,
            );
        }
        for (column, direction) in &self.union_orders {
            select.order_by(
                Alias::new(column),
                match direction {
                    SortDirection::Asc => Order::Asc,
                    SortDirection::Desc => Order::Desc,
                },
            );
        }
        if let Some(limit) = self.union_limit {
            select.limit(limit);
        }
        if let Some(offset) = self.union_offset {
            select.offset(offset);
        }
        Ok(select)
    }

    async fn execute(&self, compiled: (String, Values)) -> Result<u64> {
        Ok(self.execute_result(compiled).await?.rows_affected())
    }

    async fn execute_result(&self, (sql, values): (String, Values)) -> Result<ExecResult> {
        self.validate()?;
        let statement = Statement::from_sql_and_values(self.db.backend()?, sql, values.0);
        let result = match self.transaction {
            Some(transaction) => transaction.execute_raw(statement).await,
            None => self.db.primary_connection()?.execute_raw(statement).await,
        }
        .map_err(Error::internal)?;
        Ok(result)
    }
}

fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        Err(invalid_identifier())
    } else {
        Ok(())
    }
}

fn invalid_identifier() -> Error {
    Error::custom(
        "query_identifier_invalid",
        "query identifier must contain only letters, digits, and underscores",
    )
}
fn empty_values() -> Error {
    Error::custom("query_values_empty", "query values must not be empty")
}
fn unsupported_backend() -> Error {
    Error::custom(
        "query_backend_unsupported",
        "query backend is not supported",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_resolver_rejects_invalid_and_array_values() -> Result<()> {
        for (input, expected) in [
            ("?page=0", 1),
            ("?page=-1", 1),
            ("?page=abc", 1),
            ("?page=2", 2),
            ("?page=01", 1),
        ] {
            let uri: http::Uri = format!("/articles{input}")
                .parse()
                .map_err(Error::internal)?;
            assert_eq!(PageContext::from_uri(&uri).current_page("page"), expected);
        }
        let context = PageContext::from_uri(&"/articles?page=3".parse().map_err(Error::internal)?);
        *context
            .input
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(serde_json::json!({"page": [4]}));
        assert_eq!(context.current_page("page"), 1);
        Ok(())
    }

    #[test]
    fn links_do_not_echo_other_query_parameters_by_default() {
        let page = QueryPage {
            items: Vec::new(),
            current_page: 2,
            per_page: 15,
            total: 31,
            last_page: 3,
            path: "/articles".to_owned(),
            query: Vec::new(),
            page_name: "articles_page".to_owned(),
            fragment: None,
        };
        assert_eq!(page.url(3), "/articles?articles_page=3");
        assert_eq!(
            page.previous_page_url().as_deref(),
            Some("/articles?articles_page=1")
        );
        assert_eq!(
            page.appends([("filter".to_owned(), "rust".to_owned())])
                .fragment("results")
                .url(3),
            "/articles?filter=rust&articles_page=3#results"
        );
    }

    #[test]
    fn identifiers_reject_sql_fragments() {
        assert!(validate_identifier("owner_id").is_ok());
        assert!(validate_identifier("owner_id; DROP TABLE users").is_err());
        assert!(validate_identifier("table.column").is_err());
    }
}
