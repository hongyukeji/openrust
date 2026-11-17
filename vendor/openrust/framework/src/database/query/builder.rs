use std::fmt::Write as _;

use futures_util::TryStreamExt as _;

use crate::database::{
    Connection, DatabaseCursor, DatabaseError, DatabaseResult, Row, Transaction, Value,
};

use super::{
    Conditions, Expression,
    conditions::{date_methods, predicate_methods},
    grammars::{DatePart, Grammar, IndexHintKind},
};

mod chunks;
mod pagination;
mod subqueries;
mod vectors;
mod writes;

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

enum Order {
    Column(String, String),
    Raw(Expression),
    InOrderOf(Column, Vec<Value>),
    FullText(Vec<String>, String, super::FullTextOptions),
}

struct GroupLimit {
    partition: String,
    limit: Option<u64>,
}

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
    lock: Option<bool>,
    index_hint: Option<(IndexHintKind, String)>,
    columns: Vec<Column>,
    distinct: bool,
    predicates: Conditions,
    groups: Vec<Column>,
    havings: Conditions,
    orders: Vec<Order>,
    limit: Option<u64>,
    offset: Option<u64>,
    group_limit: Option<GroupLimit>,
    use_write: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's Query Builder API")]
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
        }
    }

    fn conditions(&mut self) -> &mut Conditions {
        &mut self.predicates
    }

    predicate_methods!();

    pub fn from(mut self, table: &str) -> Self {
        self.source = Source::Table(table.into());
        self
    }

    pub fn select<C: Into<Column>>(mut self, columns: impl IntoIterator<Item = C>) -> Self {
        self.columns.clear();
        self.addSelect(columns)
    }

    pub fn addSelect<C: Into<Column>>(mut self, columns: impl IntoIterator<Item = C>) -> Self {
        self.columns.extend(columns.into_iter().map(Into::into));
        self
    }

    pub fn selectRaw(self, sql: &str, bindings: Vec<Value>) -> Self {
        self.addSelect([Expression::new(sql).with_bindings(bindings)])
    }

    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    pub fn groupBy(mut self, columns: &[&str]) -> Self {
        self.groups.extend(
            columns
                .iter()
                .map(|column| Column::Identifier((*column).into())),
        );
        self
    }

    pub fn groupByRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.groups
            .push(Column::Raw(Expression::new(sql).with_bindings(bindings)));
        self
    }

    pub fn having(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        self.havings.compare(false, column, operator, value.into());
        self
    }

    pub fn orHaving(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        self.havings.compare(true, column, operator, value.into());
        self
    }

    pub fn havingBetween(mut self, column: &str, bounds: [Value; 2]) -> Self {
        self.havings.between(false, column, bounds, false);
        self
    }

    pub fn havingRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.havings
            .raw(false, Expression::new(sql).with_bindings(bindings));
        self
    }

    pub fn orHavingRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.havings
            .raw(true, Expression::new(sql).with_bindings(bindings));
        self
    }

    pub fn orderBy(mut self, column: &str, direction: &str) -> Self {
        self.active_orders()
            .push(Order::Column(column.into(), direction.into()));
        self
    }

    pub fn orderByDesc(self, column: &str) -> Self {
        self.orderBy(column, "desc")
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
    pub fn inOrderOf(mut self, column: impl Into<Column>, values: Vec<Value>) -> Self {
        if !values.is_empty() {
            self.active_orders()
                .push(Order::InOrderOf(column.into(), values));
        }
        self
    }

    pub fn orderByRaw(mut self, sql: &str, bindings: Vec<Value>) -> Self {
        self.active_orders()
            .push(Order::Raw(Expression::new(sql).with_bindings(bindings)));
        self
    }

    pub fn reorder(mut self) -> Self {
        self.active_orders().clear();
        self
    }

    pub fn latest(self, column: &str) -> Self {
        self.orderByDesc(column)
    }

    pub fn oldest(self, column: &str) -> Self {
        self.orderBy(column, "asc")
    }

    /// Limits the rows returned for each value of the partition column.
    pub fn groupLimit(mut self, limit: u64, column: &str) -> Self {
        self.group_limit = Some(GroupLimit {
            partition: column.into(),
            limit: Some(limit),
        });
        self
    }

    pub(in crate::database) fn group_limit(mut self, partition: &str) -> Self {
        if self.group_limit.is_none() && (self.limit.is_some() || self.offset.is_some()) {
            self.group_limit = Some(GroupLimit {
                partition: partition.into(),
                limit: self.limit.take(),
            });
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

    pub fn forPage(self, page: u64, per_page: u64) -> Self {
        self.offset(page.saturating_sub(1).saturating_mul(per_page))
            .limit(per_page)
    }

    pub fn useWritePdo(mut self) -> Self {
        self.use_write = true;
        self
    }

    pub fn useIndex(self, index: &str) -> Self {
        self.with_index_hint(IndexHintKind::Use, index)
    }

    pub fn forceIndex(self, index: &str) -> Self {
        self.with_index_hint(IndexHintKind::Force, index)
    }

    pub fn ignoreIndex(self, index: &str) -> Self {
        self.with_index_hint(IndexHintKind::Ignore, index)
    }

    fn with_index_hint(mut self, kind: IndexHintKind, index: &str) -> Self {
        self.index_hint = Some((kind, index.to_owned()));
        self
    }

    fn compile_table_hints(&self, sql: &mut String, lock: Option<bool>) -> DatabaseResult<()> {
        let hint = self
            .index_hint
            .as_ref()
            .map(|(kind, name)| (*kind, name.as_str()));
        self.grammar.table_hints(hint, lock, sql)
    }

    pub fn lockForUpdate(mut self) -> Self {
        self.lock = Some(true);
        self.useWritePdo()
    }

    pub fn sharedLock(mut self) -> Self {
        self.lock = Some(false);
        self.useWritePdo()
    }

    pub fn toSql(&self) -> DatabaseResult<String> {
        self.compile(None).map(|(sql, _)| sql)
    }

    pub fn getBindings(&self) -> DatabaseResult<Vec<Value>> {
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

    pub async fn get(mut self) -> DatabaseResult<Vec<Row>> {
        let (sql, bindings) = self.compile(None)?;
        let mut rows = self.execute(&sql, &bindings).await?;
        self.remove_group_rank(&mut rows);
        Ok(rows)
    }

    /// Streams the query results without collecting them into a vector.
    pub fn cursor(mut self) -> DatabaseCursor<'connection> {
        Box::pin(async_stream::try_stream! {
            let grouped = self.group_limit.is_some();
            let (sql, bindings) = self.compile(None)?;
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
                yield row;
            }
        })
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

    pub async fn first(self) -> DatabaseResult<Option<Row>> {
        Ok(self.limit(1).get().await?.into_iter().next())
    }

    pub async fn find(self, id: impl Into<Value>) -> DatabaseResult<Option<Row>> {
        self.r#where("id", "=", id).first().await
    }

    pub async fn value(self, column: &str) -> DatabaseResult<Value> {
        self.select([column])
            .first()
            .await?
            .map_or(Ok(Value::Null), Row::scalar)
    }

    pub async fn pluck(self, column: &str) -> DatabaseResult<Vec<Value>> {
        self.select([column])
            .get()
            .await?
            .into_iter()
            .map(Row::scalar)
            .collect()
    }

    pub async fn exists(mut self) -> DatabaseResult<bool> {
        self.exists_query().await
    }

    async fn exists_query(&mut self) -> DatabaseResult<bool> {
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

    pub async fn doesntExist(self) -> DatabaseResult<bool> {
        self.exists().await.map(|value| !value)
    }

    pub async fn count(self, column: &str) -> DatabaseResult<u64> {
        match self.aggregate("count", column).await? {
            Value::Null => Ok(0),
            Value::Unsigned(count) => Ok(count),
            Value::Integer(count) => {
                u64::try_from(count).map_err(|_| DatabaseError::InvalidQuery("count result"))
            }
            _ => Err(DatabaseError::InvalidQuery("count result")),
        }
    }

    pub async fn min(self, column: &str) -> DatabaseResult<Value> {
        self.aggregate("min", column).await
    }

    pub async fn max(self, column: &str) -> DatabaseResult<Value> {
        self.aggregate("max", column).await
    }

    pub async fn sum(self, column: &str) -> DatabaseResult<Value> {
        match self.aggregate("sum", column).await? {
            Value::Null => Ok(0.into()),
            value => Ok(value),
        }
    }

    pub async fn avg(self, column: &str) -> DatabaseResult<Value> {
        self.aggregate("avg", column).await
    }

    pub async fn average(self, column: &str) -> DatabaseResult<Value> {
        self.avg(column).await
    }

    async fn aggregate(mut self, function: &'static str, column: &str) -> DatabaseResult<Value> {
        let (sql, bindings) = self.compile(Some((function, column)))?;
        first_value(self.execute(&sql, &bindings).await?)
    }

    async fn execute(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<Vec<Row>> {
        match &mut self.executor {
            Executor::Connection(connection) if self.use_write => {
                connection.selectFromWriteConnection(sql, bindings).await
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

        if let Some(rank) = rank {
            sql.push_str(", ");
            sql.push_str(&rank.sql);
            bindings.extend_from_slice(&rank.bindings);
        }
        sql.push_str(" from ");
        sql.push_str(
            &self
                .source
                .compile(&self.grammar, database, &mut bindings)?,
        );
        self.compile_table_hints(&mut sql, self.lock)?;
        self.compile_joins(&mut sql, &mut bindings)?;

        self.compile_predicates(&mut sql, &mut bindings)?;
        if !self.groups.is_empty() {
            sql.push_str(" group by ");
            self.compile_columns(&self.groups, &mut sql, &mut bindings, false)?;
        }

        if !self.havings.is_empty() {
            sql.push_str(" having ");
            self.havings
                .compile(&self.grammar, &mut sql, &mut bindings)?;
        }

        let ordered = rank.is_none()
            && !self.orders.is_empty()
            && (aggregate.is_none() || !self.groups.is_empty());
        if ordered {
            self.compile_orders(&self.orders, &mut sql, &mut bindings)?;
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
        if let Some(exclusive) = self.lock {
            sql.push_str(self.grammar.lock(exclusive));
        }
        if !self.unions.is_empty() {
            sql = self.compile_unions(sql, &mut bindings)?;
        }
        Ok((sql, bindings))
    }

    fn compile_joins(&self, sql: &mut String, bindings: &mut Vec<Value>) -> DatabaseResult<()> {
        for join in &self.joins {
            join.compile(&self.grammar, sql, bindings)?;
        }
        Ok(())
    }

    fn compile_predicates(
        &self,
        sql: &mut String,
        bindings: &mut Vec<Value>,
    ) -> DatabaseResult<()> {
        if !self.predicates.is_empty() {
            sql.push_str(" where ");
            self.predicates.compile(&self.grammar, sql, bindings)?;
        }
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
