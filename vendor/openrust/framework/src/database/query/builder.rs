use std::{fmt::Write as _, sync::Arc};

use futures_util::TryStreamExt as _;

use crate::database::{
    Connection, DatabaseCursor, DatabaseError, DatabaseResult, Row, Transaction, Value,
};

use super::{
    Conditions, Expression, QueryLock,
    conditions::{date_methods, predicate_methods},
    grammars::{DatePart, Grammar, IndexHintKind},
};

pub(super) mod bindings;
mod chunks;
mod pagination;
mod subqueries;
mod vectors;
mod writes;

pub use bindings::BindingValues;
pub(in crate::database) use chunks::{
    ChunkKey, collection_chunk_index, flatten_batches, map_chunk_rows, visit_batches,
};
use subqueries::{Join, Source};

enum Executor<'connection> {
    Connection(Connection),
    Transaction(&'connection mut Transaction),
    Detached(Connection),
}

impl Executor<'_> {
    fn source(&self) -> &Connection {
        match self {
            Self::Connection(connection) | Self::Detached(connection) => connection,
            Self::Transaction(transaction) => transaction.query_connection(),
        }
    }
}

/// A selected column name or an explicitly trusted expression.
#[derive(Clone)]
pub enum Column {
    Identifier(String),
    Raw(Expression),
}

impl From<&str> for Column {
    fn from(value: &str) -> Self {
        Self::Identifier(value.to_owned())
    }
}

impl From<Expression> for Column {
    fn from(value: Expression) -> Self {
        Self::Raw(value)
    }
}

impl Column {
    pub(super) fn compile(
        &self,
        grammar: &Grammar,
        sql: &mut String,
        bindings: &mut Vec<Value>,
        projection: bool,
    ) -> DatabaseResult<()> {
        match self {
            Self::Identifier(column) => {
                let name = if projection {
                    grammar.projection(column)?
                } else {
                    grammar.column(column)?
                };
                sql.push_str(&name);
            }
            Self::Raw(expression) => {
                sql.push_str(&expression.sql);
                bindings.extend_from_slice(&expression.bindings);
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
enum Order {
    Column(String, String),
    Raw(Expression),
    InOrderOf(Column, Vec<Value>),
    FullText(Vec<String>, String, super::FullTextOptions),
}

#[derive(Clone)]
struct GroupLimit {
    partition: String,
    limit: Option<u64>,
}

type BeforeQuery = dyn for<'a> Fn(Builder<'a>) -> DatabaseResult<Builder<'a>> + Send + Sync;
type AfterQuery = dyn Fn(Vec<Row>) -> DatabaseResult<Vec<Row>> + Send + Sync;

/// Builds and executes a query using the grammar of the selected connection.
pub struct Builder<'connection> {
    executor: Executor<'connection>,
    grammar: Grammar,
    source: Source,
    joins: Vec<Join>,
    unions: Vec<(bool, Expression)>,
    union_orders: Vec<Order>,
    union_limit: Option<u64>,
    union_offset: Option<u64>,
    lock: Option<QueryLock>,
    index_hint: Option<(IndexHintKind, String)>,
    columns: Vec<Column>,
    distinct: bool,
    predicates: Conditions,
    groups: Vec<Column>,
    havings: Conditions,
    orders: Vec<Order>,
    limit: Option<u64>,
    offset: Option<u64>,
    group_limit: Option<Box<GroupLimit>>,
    use_write: bool,
    before_query: Vec<Arc<BeforeQuery>>,
    after_query: Vec<Arc<AfterQuery>>,
    binding_changes: Option<Box<bindings::Changes>>,
}

impl<'connection> Builder<'connection> {
    pub(crate) fn connection(
        connection: Connection,
        grammar: Grammar,
        table: &str,
    ) -> Builder<'static> {
        Builder::new(Executor::Connection(connection), grammar, table)
    }

    pub(crate) fn transaction(
        transaction: &'connection mut Transaction,
        grammar: Grammar,
        table: &str,
    ) -> Self {
        Self::new(Executor::Transaction(transaction), grammar, table)
    }

    pub(crate) fn in_transaction<'a>(
        self,
        transaction: &'a mut Transaction,
    ) -> DatabaseResult<Builder<'a>>
    where
        'connection: 'a,
    {
        if !transaction.belongs_to(self.executor.source()) {
            return Err(DatabaseError::InvalidQuery("query transaction connection"));
        }
        let mut query: Builder<'a> = self;
        query.executor = Executor::Transaction(transaction);
        Ok(query)
    }

    fn new(executor: Executor<'connection>, grammar: Grammar, table: &str) -> Self {
        Self {
            executor,
            grammar,
            source: Source::Table(table.into()),
            joins: Vec::new(),
            unions: Vec::new(),
            union_orders: Vec::new(),
            union_limit: None,
            union_offset: None,
            lock: None,
            index_hint: None,
            columns: Vec::new(),
            distinct: false,
            predicates: Conditions::default(),
            groups: Vec::new(),
            havings: Conditions::default(),
            orders: Vec::new(),
            limit: None,
            offset: None,
            group_limit: None,
            use_write: false,
            before_query: Vec::new(),
            after_query: Vec::new(),
            binding_changes: None,
        }
    }

    fn conditions(&mut self) -> &mut Conditions {
        &mut self.predicates
    }

    predicate_methods!();

    pub fn get_connection(&self) -> &Connection {
        self.executor.source()
    }

    pub(in crate::database) fn group_predicates(mut self) -> Self {
        self.predicates = Conditions::default().where_nested(|_| self.predicates);
        self
    }

    pub fn from(mut self, table: &str) -> Self {
        self.retain_source_bindings();
        self.source = Source::Table(table.into());
        self
    }

    pub(in crate::database) fn has_columns(&self) -> bool {
        !self.columns.is_empty()
    }

    pub fn select<C: Into<Column>>(mut self, columns: impl IntoIterator<Item = C>) -> Self {
        self.columns.clear();
        self.clear_binding_changes(0);
        self.add_select(columns)
    }

    pub fn add_select<C: Into<Column>>(mut self, columns: impl IntoIterator<Item = C>) -> Self {
        self.columns.extend(columns.into_iter().map(Into::into));
        self
    }

    pub fn select_raw(self, sql: &str, bindings: Vec<Value>) -> Self {
        self.add_select([Expression::new(sql).with_bindings(bindings)])
    }

    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    pub fn group_by(mut self, columns: &[&str]) -> Self {
        self.groups.extend(
            columns
                .iter()
                .map(|column| Column::Identifier((*column).into())),
        );
        self
    }

    pub fn group_by_raw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.groups
            .push(Column::Raw(Expression::new(sql).with_bindings(bindings)));
        self
    }

    pub fn having(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        self.havings.compare(false, column, operator, value.into());
        self
    }

    pub fn or_having(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        self.havings.compare(true, column, operator, value.into());
        self
    }

    pub fn having_between(mut self, column: &str, bounds: [Value; 2]) -> Self {
        self.havings.between(false, column, bounds, false);
        self
    }

    pub fn having_null(mut self, columns: &[&str]) -> Self {
        for column in columns {
            self.havings.null(false, column, false);
        }
        self
    }

    pub fn or_having_null(mut self, columns: &[&str]) -> Self {
        for column in columns {
            self.havings.null(true, column, false);
        }
        self
    }

    pub fn having_not_null(mut self, columns: &[&str]) -> Self {
        for column in columns {
            self.havings.null(false, column, true);
        }
        self
    }

    pub fn or_having_not_null(mut self, columns: &[&str]) -> Self {
        for column in columns {
            self.havings.null(true, column, true);
        }
        self
    }

    pub fn having_not_between(mut self, column: &str, bounds: [Value; 2]) -> Self {
        self.havings.between(false, column, bounds, true);
        self
    }

    pub fn or_having_between(mut self, column: &str, bounds: [Value; 2]) -> Self {
        self.havings.between(true, column, bounds, false);
        self
    }

    pub fn or_having_not_between(mut self, column: &str, bounds: [Value; 2]) -> Self {
        self.havings.between(true, column, bounds, true);
        self
    }

    pub fn having_nested(
        mut self,
        configure: impl FnOnce(Builder<'static>) -> Builder<'static>,
    ) -> Self {
        let mut query = self.subquery("");
        query.source = self.source.clone();
        let mut query = configure(query);
        let change = query
            .binding_changes
            .as_mut()
            .and_then(|changes| changes[5].take());
        self.havings
            .nested(false, |_| query.havings.with_binding_change(change), false);
        self
    }

    pub fn or_having_nested(
        mut self,
        configure: impl FnOnce(Builder<'static>) -> Builder<'static>,
    ) -> Self {
        let mut query = self.subquery("");
        query.source = self.source.clone();
        let mut query = configure(query);
        let change = query
            .binding_changes
            .as_mut()
            .and_then(|changes| changes[5].take());
        self.havings
            .nested(true, |_| query.havings.with_binding_change(change), false);
        self
    }

    pub fn having_raw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.havings
            .raw(false, Expression::new(sql).with_bindings(bindings));
        self
    }

    pub fn or_having_raw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.havings
            .raw(true, Expression::new(sql).with_bindings(bindings));
        self
    }

    pub fn order_by(mut self, column: &str, direction: &str) -> Self {
        self.active_orders()
            .push(Order::Column(column.into(), direction.into()));
        self
    }

    pub fn order_by_desc(self, column: &str) -> Self {
        self.order_by(column, "desc")
    }

    pub(crate) fn order_by_full_text(
        mut self,
        columns: Vec<String>,
        term: String,
        options: super::FullTextOptions,
    ) -> Self {
        self.active_orders()
            .push(Order::FullText(columns, term, options));
        self
    }

    /// Orders listed values first, in the supplied order.
    pub fn in_order_of(mut self, column: impl Into<Column>, values: Vec<Value>) -> Self {
        if !values.is_empty() {
            self.active_orders()
                .push(Order::InOrderOf(column.into(), values));
        }
        self
    }

    pub fn order_by_raw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.active_orders()
            .push(Order::Raw(Expression::new(sql).with_bindings(bindings)));
        self
    }

    pub fn reorder(mut self) -> Self {
        self.orders.clear();
        self.union_orders.clear();
        self.clear_binding_changes(6);
        self.clear_binding_changes(8);
        self
    }

    pub fn reorder_desc(self, column: &str) -> Self {
        self.reorder().order_by_desc(column)
    }

    pub fn in_random_order(self, seed: Option<i64>) -> Self {
        let sql = self.grammar.random(seed);
        self.order_by_raw(&sql, Vec::new())
    }

    pub fn for_page_before_id(
        mut self,
        per_page: u64,
        last_id: Option<Value>,
        column: &str,
    ) -> Self {
        self.orders
            .retain(|order| !matches!(order, Order::Column(name, _) if name == column));
        let query = match last_id.filter(|value| !matches!(value, Value::Null)) {
            Some(id) => self.r#where(column, "<", id),
            None => self.where_not_null(column),
        };
        query.order_by_desc(column).limit(per_page)
    }

    pub fn for_page_after_id(
        mut self,
        per_page: u64,
        last_id: Option<Value>,
        column: &str,
    ) -> Self {
        self.orders
            .retain(|order| !matches!(order, Order::Column(name, _) if name == column));
        let query = match last_id.filter(|value| !matches!(value, Value::Null)) {
            Some(id) => self.r#where(column, ">", id),
            None => self.where_not_null(column),
        };
        query.order_by(column, "asc").limit(per_page)
    }

    pub fn latest(self, column: &str) -> Self {
        self.order_by_desc(column)
    }

    pub fn oldest(self, column: &str) -> Self {
        self.order_by(column, "asc")
    }

    /// Limits the rows returned for each value of the partition column.
    pub fn group_limit(mut self, limit: u64, column: &str) -> Self {
        self.group_limit = Some(Box::new(GroupLimit {
            partition: column.into(),
            limit: Some(limit),
        }));
        self
    }

    pub(in crate::database) fn configure_relation_group_limit(mut self, partition: &str) -> Self {
        if self.group_limit.is_none() && (self.limit.is_some() || self.offset.is_some()) {
            self.group_limit = Some(Box::new(GroupLimit {
                partition: partition.into(),
                limit: self.limit.take(),
            }));
        }
        self
    }

    pub fn limit(mut self, rows: u64) -> Self {
        if self.unions.is_empty() {
            self.limit = Some(rows);
        } else {
            self.union_limit = Some(rows);
        }
        self
    }

    pub fn take(self, rows: u64) -> Self {
        self.limit(rows)
    }

    pub fn offset(mut self, rows: u64) -> Self {
        if self.unions.is_empty() {
            self.offset = Some(rows);
        } else {
            self.union_offset = Some(rows);
        }
        self
    }

    pub fn skip(self, rows: u64) -> Self {
        self.offset(rows)
    }

    pub fn for_page(self, page: u64, per_page: u64) -> Self {
        self.offset(page.saturating_sub(1).saturating_mul(per_page))
            .limit(per_page)
    }

    pub fn use_write_pdo(mut self) -> Self {
        self.use_write = true;
        self
    }

    pub fn use_index(self, index: &str) -> Self {
        self.with_index_hint(IndexHintKind::Use, index)
    }

    pub fn force_index(self, index: &str) -> Self {
        self.with_index_hint(IndexHintKind::Force, index)
    }

    pub fn ignore_index(self, index: &str) -> Self {
        self.with_index_hint(IndexHintKind::Ignore, index)
    }

    fn with_index_hint(mut self, kind: IndexHintKind, index: &str) -> Self {
        self.index_hint = Some((kind, index.to_owned()));
        self
    }

    fn compile_table_hints(
        &self,
        sql: &mut String,
        lock: Option<&QueryLock>,
    ) -> DatabaseResult<()> {
        let hint = self
            .index_hint
            .as_ref()
            .map(|(kind, name)| (*kind, name.as_str()));
        self.grammar.table_hints(hint, lock, sql)
    }

    /// Routes a pessimistic locking read through the write connection.
    pub fn lock(mut self, lock: impl Into<QueryLock>) -> Self {
        self.lock = Some(lock.into());
        self.use_write_pdo()
    }

    pub fn lock_for_update(self) -> Self {
        self.lock(true)
    }

    pub fn shared_lock(self) -> Self {
        self.lock(false)
    }

    /// 回调返回修改后的查询，沿用 Builder 的所有权链式接口。
    pub fn before_query(
        mut self,
        callback: impl for<'a> Fn(Builder<'a>) -> DatabaseResult<Builder<'a>> + Send + Sync + 'static,
    ) -> Self {
        self.before_query.push(Arc::new(callback));
        self
    }
    pub fn after_query(
        mut self,
        callback: impl Fn(Vec<Row>) -> DatabaseResult<Vec<Row>> + Send + Sync + 'static,
    ) -> Self {
        self.after_query.push(Arc::new(callback));
        self
    }
    /// 复制查询结构，保留连接的独占借用；回调失败时原查询及事务仍有效。
    fn snapshot(&self) -> Builder<'static> {
        Builder {
            executor: Executor::Detached(self.executor.source().clone()),
            grammar: self.grammar.clone(),
            source: self.source.clone(),
            joins: self.joins.clone(),
            unions: self.unions.clone(),
            union_orders: self.union_orders.clone(),
            union_limit: self.union_limit,
            union_offset: self.union_offset,
            lock: self.lock.clone(),
            index_hint: self.index_hint.clone(),
            columns: self.columns.clone(),
            distinct: self.distinct,
            predicates: self.predicates.clone(),
            groups: self.groups.clone(),
            havings: self.havings.clone(),
            orders: self.orders.clone(),
            limit: self.limit,
            offset: self.offset,
            group_limit: self.group_limit.clone(),
            use_write: self.use_write,
            before_query: self.before_query.clone(),
            after_query: self.after_query.clone(),
            binding_changes: self.binding_changes.clone(),
        }
    }
    pub fn apply_before_query_callbacks(&mut self) -> DatabaseResult<()> {
        let callbacks = self.before_query.clone();
        for callback in callbacks {
            let mut changed = callback(self.snapshot())?;
            std::mem::swap(&mut self.executor, &mut changed.executor);
            *self = changed;
        }
        self.before_query.clear();
        Ok(())
    }
    pub fn apply_after_query_callbacks(&self, mut rows: Vec<Row>) -> DatabaseResult<Vec<Row>> {
        for callback in &self.after_query {
            rows = callback(rows)?;
        }
        Ok(rows)
    }
    pub fn to_raw_sql(&mut self) -> DatabaseResult<String> {
        self.apply_before_query_callbacks()?;
        let (sql, bindings) = self.compile(None)?;
        crate::database::bindings::render_query(
            &sql,
            &crate::database::events::QueryBindings::Positional(bindings),
            !self.executor.source().is_sqlite(),
        )
    }
    pub fn to_sql(&mut self) -> DatabaseResult<String> {
        self.apply_before_query_callbacks()?;
        self.compile(None).map(|(sql, _)| sql)
    }

    pub fn get_bindings(&self) -> DatabaseResult<Vec<Value>> {
        self.compile(None).map(|(_, bindings)| bindings)
    }

    /// Fetches ordered pages and calls the callback with each nonempty page and its number.
    pub async fn chunk<Fut>(
        mut self,
        count: u64,
        mut callback: impl FnMut(Vec<Row>, u64) -> Fut,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let mut pages = self.chunk_pages(count);
        let mut page = 1;
        while let Some(rows) = pages.try_next().await? {
            if !callback(rows, page).await? {
                return Ok(false);
            }
            page += 1;
        }
        Ok(true)
    }

    pub(in crate::database) fn chunk_pages(&mut self, count: u64) -> DatabaseCursor<'_, Vec<Row>> {
        Box::pin(async_stream::try_stream! {
            if count == 0 {
                Err(DatabaseError::InvalidQuery("chunk size must be positive"))?;
            }
            if self.active_orders().is_empty() {
                Err(DatabaseError::InvalidQuery("chunk requires an order by clause"))?;
            }
            let (mut offset, mut remaining) = if self.unions.is_empty() {
                (self.offset.unwrap_or(0), self.limit.unwrap_or(u64::MAX))
            } else {
                (self.union_offset.unwrap_or(0), self.union_limit.unwrap_or(u64::MAX))
            };
            self.apply_before_query_callbacks()?;
            while remaining != 0 {
                let size = count.min(remaining);
                if self.unions.is_empty() {
                    self.limit = Some(size);
                    self.offset = Some(offset);
                } else {
                    self.union_limit = Some(size);
                    self.union_offset = Some(offset);
                }
                let (sql, bindings) = self.compile(None)?;
                let mut rows = self.execute(&sql, &bindings).await?;
                self.remove_group_rank(&mut rows);
                let rows = self.apply_after_query_callbacks(rows)?;
                let length = rows.len() as u64;
                if length == 0 {
                    break;
                }
                yield rows;
                if length < size {
                    break;
                }
                remaining -= length;
                if remaining != 0 {
                    offset = offset.checked_add(length).ok_or(DatabaseError::InvalidQuery("chunk offset overflow"))?;
                }
            }
        })
    }

    #[track_caller]
    pub fn get(mut self) -> impl Future<Output = DatabaseResult<Vec<Row>>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.apply_before_query_callbacks()?;
            let (sql, bindings) = self.compile(None)?;
            let mut rows = self.execute(&sql, &bindings).await?;
            self.remove_group_rank(&mut rows);
            self.apply_after_query_callbacks(rows)
        })
    }

    /// Streams the query results without collecting them into a vector.
    #[track_caller]
    pub fn cursor(mut self) -> DatabaseCursor<'connection> {
        Box::pin(crate::database::events::stream_with_query_caller(
            std::panic::Location::caller(),
            async_stream::try_stream! {
            self.apply_before_query_callbacks()?;
            let grouped = self.group_limit.is_some();
            let (sql, bindings) = self.compile(None)?;
            let callbacks = self.after_query.clone();
            let mut rows = match &mut self.executor {
                Executor::Connection(connection) =>
                    connection.cursor_with_mode(&sql, &bindings, self.use_write),
                Executor::Transaction(transaction) => transaction.cursor(&sql, &bindings),
                Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery"))?,
            };
            while let Some(mut row) = rows.try_next().await? {
                if grouped {
                    row.remove_last_column();
                }
                let mut mapped = vec![row];
                for callback in &callbacks { mapped = callback(mapped)?; }
                if let Some(row) = mapped.into_iter().next() { yield row; }
            }
            },
        ))
    }

    fn remove_group_rank(&self, rows: &mut [Row]) {
        if self.group_limit.is_some() {
            rows.iter_mut().for_each(Row::remove_last_column);
        }
    }

    fn compile_group_limit(
        &self,
        group: &GroupLimit,
        database: Option<&str>,
    ) -> DatabaseResult<(String, Vec<Value>)> {
        if !self.unions.is_empty() {
            return Err(DatabaseError::InvalidQuery("group limit with union"));
        }
        let partition = self.grammar.column(&group.partition)?;
        let rank = self.grammar.quote("laravel_row")?;
        let alias = self.grammar.quote("laravel_table")?;
        let mut window = format!("row_number() over (partition by {partition}");
        let mut window_bindings = Vec::new();
        self.compile_orders(&self.orders, &mut window, &mut window_bindings)?;
        self.change_bindings(6, &mut window_bindings, 0);
        write!(window, ") as {rank}").expect("writing to String");
        let rank_projection = Expression::new(window).with_bindings(window_bindings);
        let (sql, mut bindings) =
            self.compile_components(None, database, Some(&rank_projection))?;
        let mut wrapped = format!("select * from ({sql}) as {alias}");
        let bound = |value: u64| Value::Integer(i64::try_from(value).unwrap_or(i64::MAX));
        if let Some(limit) = group.limit {
            write!(wrapped, " where {rank} <= ?").expect("writing to String");
            bindings.push(bound(self.offset.unwrap_or(0).saturating_add(limit)));
        }
        if let Some(offset) = self.offset {
            let separator = if group.limit.is_some() {
                "and"
            } else {
                "where"
            };
            write!(wrapped, " {separator} {rank} > ?").expect("writing to String");
            bindings.push(bound(offset));
        }
        write!(wrapped, " order by {rank}").expect("writing to String");
        Ok((wrapped, bindings))
    }

    pub(crate) fn get_with_transaction<'a>(
        self,
        transaction: Option<&'a mut Transaction>,
    ) -> crate::database::DatabaseFuture<'a, DatabaseResult<Vec<Row>>>
    where
        'connection: 'a,
    {
        match transaction {
            Some(tx) if tx.belongs_to(self.executor.source()) => {
                Box::pin(async move { self.in_transaction(tx)?.get().await })
            }
            _ => Box::pin(self.get()),
        }
    }

    #[track_caller]
    pub fn first(self) -> impl Future<Output = DatabaseResult<Option<Row>>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            Ok(self.limit(1).get().await?.into_iter().next())
        })
    }

    #[track_caller]
    pub fn find(self, id: impl Into<Value>) -> impl Future<Output = DatabaseResult<Option<Row>>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.r#where("id", "=", id).first().await
        })
    }

    #[track_caller]
    pub fn value(self, column: &str) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.select([column])
                .first()
                .await?
                .map_or(Ok(Value::Null), Row::scalar)
        })
    }

    #[track_caller]
    pub fn raw_value(
        self,
        expression: &str,
        bindings: Vec<Value>,
    ) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            Ok(self
                .select_raw(expression, bindings)
                .first()
                .await?
                .and_then(|row| row.get_at(0).cloned())
                .unwrap_or(Value::Null))
        })
    }

    pub async fn exists_or<T, Fut>(
        self,
        callback: impl FnOnce() -> Fut,
    ) -> DatabaseResult<super::ExistsOr<T>>
    where
        Fut: Future<Output = DatabaseResult<T>>,
    {
        if self.exists().await? {
            Ok(super::ExistsOr::Satisfied)
        } else {
            callback().await.map(super::ExistsOr::Callback)
        }
    }

    pub async fn doesnt_exist_or<T, Fut>(
        self,
        callback: impl FnOnce() -> Fut,
    ) -> DatabaseResult<super::ExistsOr<T>>
    where
        Fut: Future<Output = DatabaseResult<T>>,
    {
        if self.doesnt_exist().await? {
            Ok(super::ExistsOr::Satisfied)
        } else {
            callback().await.map(super::ExistsOr::Callback)
        }
    }

    #[track_caller]
    pub fn pluck(self, column: &str) -> impl Future<Output = DatabaseResult<Vec<Value>>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.select([column])
                .get()
                .await?
                .into_iter()
                .map(Row::scalar)
                .collect()
        })
    }

    #[track_caller]
    pub fn exists(mut self) -> impl Future<Output = DatabaseResult<bool>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.exists_query().await
        })
    }

    async fn exists_query(&mut self) -> DatabaseResult<bool> {
        self.apply_before_query_callbacks()?;
        let (sql, bindings) = self.compile(None)?;
        let alias = self.grammar.column("exists")?;
        let sql = format!("select exists({sql}) as {alias}");
        let rows = self.execute(&sql, &bindings).await?;
        match first_value(rows)? {
            Value::Integer(value) => Ok(value != 0),
            Value::Unsigned(value) => Ok(value != 0),
            Value::Bool(value) => Ok(value),
            _ => Err(DatabaseError::InvalidQuery("exists result")),
        }
    }

    #[track_caller]
    pub fn doesnt_exist(self) -> impl Future<Output = DatabaseResult<bool>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.exists().await.map(|value| !value)
        })
    }

    #[track_caller]
    pub fn count(self, column: &str) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            match self.aggregate("count", column).await? {
                Value::Null => Ok(0),
                Value::Unsigned(count) => Ok(count),
                Value::Integer(count) => {
                    u64::try_from(count).map_err(|_| DatabaseError::InvalidQuery("count result"))
                }
                _ => Err(DatabaseError::InvalidQuery("count result")),
            }
        })
    }

    #[track_caller]
    pub fn min(self, column: &str) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.aggregate("min", column).await
        })
    }

    #[track_caller]
    pub fn max(self, column: &str) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.aggregate("max", column).await
        })
    }

    #[track_caller]
    pub fn sum(self, column: &str) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            match self.aggregate("sum", column).await? {
                Value::Null => Ok(0.into()),
                value => Ok(value),
            }
        })
    }

    #[track_caller]
    pub fn avg(self, column: &str) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.aggregate("avg", column).await
        })
    }

    #[track_caller]
    pub fn average(self, column: &str) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.avg(column).await
        })
    }

    async fn aggregate(mut self, function: &'static str, column: &str) -> DatabaseResult<Value> {
        self.apply_before_query_callbacks()?;
        let (sql, bindings) = self.compile(Some((function, column)))?;
        let rows = self.execute(&sql, &bindings).await?;
        first_value(self.apply_after_query_callbacks(rows)?)
    }

    async fn execute(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<Vec<Row>> {
        match &mut self.executor {
            Executor::Connection(connection) if self.use_write => {
                connection.select_from_write_connection(sql, bindings).await
            }
            Executor::Connection(connection) => connection.select(sql, bindings).await,
            Executor::Transaction(transaction) => transaction.select(sql, bindings).await,
            Executor::Detached(_) => Err(DatabaseError::InvalidQuery("detached subquery")),
        }
    }

    fn compile(&self, aggregate: Option<(&str, &str)>) -> DatabaseResult<(String, Vec<Value>)> {
        self.compile_with_database(aggregate, None)
    }

    fn compile_with_database(
        &self,
        aggregate: Option<(&str, &str)>,
        database: Option<&str>,
    ) -> DatabaseResult<(String, Vec<Value>)> {
        if let Some(group) = &self.group_limit {
            let (sql, bindings) = self.compile_group_limit(group, database)?;
            return match aggregate {
                Some((function, column)) => {
                    Ok((self.wrap_aggregate(function, column, &sql)?, bindings))
                }
                None => Ok((sql, bindings)),
            };
        }
        if let Some((function, column)) = aggregate
            && !self.unions.is_empty()
        {
            let (sql, bindings) = self.compile_with_database(None, database)?;
            return Ok((self.wrap_aggregate(function, column, &sql)?, bindings));
        }
        self.compile_components(aggregate, database, None)
    }

    fn wrap_aggregate(&self, function: &str, column: &str, sql: &str) -> DatabaseResult<String> {
        let distinct = if self.distinct && column != "*" {
            "distinct "
        } else {
            ""
        };
        let column = self.grammar.column(column)?;
        let alias = self.grammar.column("aggregate")?;
        let subquery = self.grammar.table("temp_table")?;
        Ok(format!(
            "select {function}({distinct}{column}) as {alias} from ({sql}) as {subquery}"
        ))
    }

    fn compile_components(
        &self,
        aggregate: Option<(&str, &str)>,
        database: Option<&str>,
        rank: Option<&Expression>,
    ) -> DatabaseResult<(String, Vec<Value>)> {
        let mut sql = String::from("select ");
        let mut bindings = Vec::new();
        let sql_server = self.grammar.is_sql_server();

        if let Some((function, column)) = aggregate {
            let distinct = if self.distinct && column != "*" {
                "distinct "
            } else {
                ""
            };
            let column = self.grammar.column(column)?;
            let alias = self.grammar.column("aggregate")?;
            sql.push_str(&format!("{function}({distinct}{column}) as {alias}"));
        } else {
            if self.distinct {
                sql.push_str("distinct ");
            }
            if sql_server && self.limit == Some(0) {
                sql.push_str("top (0) ");
            }
            if self.columns.is_empty() {
                sql.push('*');
            } else {
                self.compile_columns(&self.columns, &mut sql, &mut bindings, true)?;
            }
        }

        if aggregate.is_none() {
            self.change_bindings(0, &mut bindings, 0);
        }
        if let Some(rank) = rank {
            sql.push_str(", ");
            sql.push_str(&rank.sql);
            bindings.extend_from_slice(&rank.bindings);
        }
        sql.push_str(" from ");
        let source_start = bindings.len();
        sql.push_str(
            &self
                .source
                .compile(&self.grammar, database, &mut bindings)?,
        );
        self.change_bindings(1, &mut bindings, source_start);
        self.compile_table_hints(&mut sql, self.lock.as_ref())?;
        self.compile_joins(&mut sql, &mut bindings)?;

        self.compile_predicates(&mut sql, &mut bindings)?;
        let group_start = bindings.len();
        if !self.groups.is_empty() {
            sql.push_str(" group by ");
            self.compile_columns(&self.groups, &mut sql, &mut bindings, false)?;
        }

        self.change_bindings(4, &mut bindings, group_start);
        let having_start = bindings.len();
        if !self.havings.is_empty() {
            sql.push_str(" having ");
            self.havings
                .compile(&self.grammar, &mut sql, &mut bindings)?;
        }

        self.change_bindings(5, &mut bindings, having_start);
        let order_start = bindings.len();
        let ordered = rank.is_none()
            && !self.orders.is_empty()
            && (aggregate.is_none() || !self.groups.is_empty());
        if ordered {
            self.compile_orders(&self.orders, &mut sql, &mut bindings)?;
        }

        if rank.is_none() && (aggregate.is_none() || !self.groups.is_empty()) {
            self.change_bindings(6, &mut bindings, order_start);
        }
        let offset = if rank.is_some() { None } else { self.offset };
        if sql_server {
            if self.limit != Some(0) && (self.limit.is_some() || offset.is_some()) {
                if !ordered {
                    sql.push_str(" order by (select 0)");
                }
                use std::fmt::Write as _;
                write!(sql, " offset {} rows", offset.unwrap_or(0)).expect("writing to String");
                if let Some(limit) = self.limit {
                    write!(sql, " fetch next {limit} rows only").expect("writing to String");
                }
            }
        } else {
            Grammar::pagination(&mut sql, self.limit, offset);
        }
        if let Some(lock) = &self.lock {
            self.grammar.compile_lock(lock, &mut sql);
        }
        if !self.unions.is_empty() {
            sql = self.compile_unions(sql, &mut bindings)?;
        } else {
            let start = bindings.len();
            self.change_bindings(7, &mut bindings, start);
            let start = bindings.len();
            self.change_bindings(8, &mut bindings, start);
        }
        Ok((sql, bindings))
    }

    fn compile_joins(&self, sql: &mut String, bindings: &mut Vec<Value>) -> DatabaseResult<()> {
        let start = bindings.len();
        for join in &self.joins {
            join.compile(&self.grammar, sql, bindings)?;
        }
        self.change_bindings(2, bindings, start);
        Ok(())
    }

    fn compile_predicates(
        &self,
        sql: &mut String,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<()> {
        let start = bindings.len();
        if !self.predicates.is_empty() {
            sql.push_str(" where ");
            self.predicates.compile(&self.grammar, sql, bindings)?;
        }
        self.change_bindings(3, bindings, start);
        Ok(())
    }

    fn compile_columns(
        &self,
        columns: &[Column],
        sql: &mut String,
        bindings: &mut Vec<Value>,
        projection: bool,
    ) -> DatabaseResult<()> {
        for (index, column) in columns.iter().enumerate() {
            if index != 0 {
                sql.push_str(", ");
            }
            column.compile(&self.grammar, sql, bindings, projection)?;
        }
        Ok(())
    }

    fn compile_orders(
        &self,
        orders: &[Order],
        sql: &mut String,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<()> {
        if orders.is_empty() {
            return Ok(());
        }
        sql.push_str(" order by ");
        for (index, order) in orders.iter().enumerate() {
            if index != 0 {
                sql.push_str(", ");
            }
            match order {
                Order::Column(column, direction) => {
                    let direction = direction.to_ascii_lowercase();
                    if direction != "asc" && direction != "desc" {
                        return Err(DatabaseError::InvalidQuery("order direction"));
                    }
                    sql.push_str(&self.grammar.column(column)?);
                    sql.push(' ');
                    sql.push_str(&direction);
                }
                Order::Raw(expression) => {
                    sql.push_str(&expression.sql);
                    bindings.extend_from_slice(&expression.bindings);
                }
                Order::InOrderOf(column, values) => {
                    sql.push_str("case");
                    for (rank, value) in values.iter().enumerate() {
                        sql.push_str(" when ");
                        column.compile(&self.grammar, sql, bindings, false)?;
                        write!(sql, " = ? then {rank}").expect("writing to String");
                        bindings.push(value.clone());
                    }
                    write!(sql, " else {} end", values.len()).expect("writing to String");
                }
                Order::FullText(columns, term, options) => {
                    sql.push_str(&self.grammar.postgres_full_text_rank(columns, *options)?);
                    bindings.push(Value::from(term.as_str()));
                }
            }
        }
        Ok(())
    }
}

fn first_value(rows: Vec<Row>) -> DatabaseResult<Value> {
    rows.into_iter().next().map_or(Ok(Value::Null), Row::scalar)
}
