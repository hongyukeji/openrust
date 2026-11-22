use futures_util::{Stream, TryStreamExt as _, future::Either};
use sqlx::SqlSafeStr as _;
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use sqlx::{
    Database, Executor as _, MySql, MySqlPool, PgPool, Postgres, Sqlite, SqlitePool,
    Statement as _, pool::PoolConnection,
};

use sqlx_core::transaction::TransactionManager;

use crate::{
    contracts::{
        container::Container as _,
        database::LostConnectionDetector as _,
        events::{Dispatcher as _, Listener},
    },
    events::{Delivery, Dispatcher, EventContext},
    support::facades::application_scope,
};

use super::{
    Bindings, DatabaseCursor, DatabaseError, DatabaseFuture, DatabaseResult,
    LostConnectionDetector, QueryLogEntry, RawQueryLogEntry, Row, Transaction, Value,
    bindings::{PreparedBindings, render_query},
    connection_pools::{ConnectionPools, PoolGeneration},
    events::{
        ConnectionEstablished, QueryDuration, QueryDurationHandler, QueryExecuted,
        QueryNotification, QueryRole,
    },
    query_log::{PreviewScope, QueryLog},
    test_events,
    test_transaction::{self, QueryPhysical},
    transaction::BeginCallback,
    value,
};

pub(super) type BeforeExecutingCallback = Arc<
    dyn for<'a> Fn(
            &'a str,
            Bindings<'a>,
            &'a Connection,
            Option<&'a mut Transaction>,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub(super) enum Pool {
    Sqlite(SqlitePool),
    Mysql(Arc<[MySqlPool]>),
    Postgres(PgPool, bool),
    SqlServer(super::sql_server::Pool),
}

/// A named database connection sharing its application's pool.
#[derive(Clone)]
pub struct Connection {
    pub(super) name: Arc<str>,
    pub(super) config: Arc<serde_json::Value>,
    pub(super) prefix_indexes: bool,
    pub(super) sqlite_json_types: super::schema::SqliteJsonTypes,
    pub(super) table_options: Arc<super::schema::TableOptions>,
    pub(super) postgres_dont_drop: Arc<[String]>,
    pub(super) postgres_schemas: Arc<[String]>,
    pub(super) grammar: super::query::grammars::Grammar,
    pub(super) database: Arc<str>,
    pub(super) pools: Arc<ConnectionPools>,
    pub(super) sqlite: bool,
    pub(super) sticky: bool,
    pub(super) has_direct_connection: bool,
    pub(super) model_timezone: jiff::tz::TimeZone,
    pub(super) model_container: std::sync::Weak<crate::container::Container>,
    pub(super) schema_defaults: Arc<std::sync::RwLock<super::schema::SchemaDefaults>>,
    pub(super) routing: Arc<ReadRouting>,
    pub(super) query_duration: Arc<QueryDuration>,
    pub(super) query_role: Option<QueryRole>,
    pub(super) query_log: Arc<QueryLog>,
    pub(super) test_query_counts:
        Option<std::sync::Weak<super::test_query_counts::TestQueryCounts>>,
    pub(super) before_executing: Arc<Mutex<Vec<BeforeExecutingCallback>>>,
    pub(super) before_transaction: Arc<Mutex<Vec<BeginCallback>>>,
}

tokio::task_local! {
    static ESTABLISHING_CONNECTIONS: Vec<usize>;
}

fn is_establishing(generation: &Arc<PoolGeneration>) -> bool {
    let identity = Arc::as_ptr(generation).addr();
    ESTABLISHING_CONNECTIONS
        .try_with(|connections| connections.contains(&identity))
        .unwrap_or(false)
}

#[derive(Default)]
pub(crate) struct ReadRouting {
    records_modified: AtomicBool,
    read_on_write: AtomicBool,
}

#[derive(Default)]
pub(crate) struct ConnectionScope {
    routing: Mutex<Vec<(Arc<ReadRouting>, Arc<ReadRouting>)>>,
}

impl ConnectionScope {
    pub(crate) fn state(&self, origin: &Arc<ReadRouting>) -> Arc<ReadRouting> {
        let mut routes = self
            .routing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, state)) = routes.iter().find(|(id, _)| Arc::ptr_eq(id, origin)) {
            return state.clone();
        }
        let state = Arc::new(ReadRouting::default());
        routes.push((origin.clone(), state.clone()));
        state
    }
}

pub(super) enum PhysicalConnection {
    Sqlite(Lease<Sqlite>),
    Mysql(Lease<MySql>),
    Postgres(Lease<Postgres>, bool),
    SqlServer(Box<super::sql_server::Lease>),
}

pub(super) struct QueryLease<T = QueryPhysical> {
    pub physical: T,
    pub generation: Option<Arc<PoolGeneration>>,
    role: QueryRole,
    wait: Duration,
}

pub(super) struct Lease<D: Database> {
    connection: PoolConnection<D>,
    dirty: bool,
    retire: bool,
}

impl<D: Database> Lease<D> {
    async fn acquire(pool: &sqlx::Pool<D>) -> DatabaseResult<Self> {
        let connection = pool
            .acquire()
            .await
            .map_err(DatabaseError::driver("connect"))?;
        Ok(Self {
            connection,
            dirty: false,
            retire: false,
        })
    }

    fn finish<T>(&mut self, result: Result<T, sqlx::Error>) -> Result<T, sqlx::Error> {
        self.dirty = result.as_ref().err().is_some_and(|error| {
            !matches!(error, sqlx::Error::Database(_))
                || (D::NAME == "MySQL"
                    && (super::transaction::mysql_deadlock(error)
                        || LostConnectionDetector.causedByLostConnection(error)))
        });
        result
    }

    fn start(&mut self) -> DatabaseResult<()> {
        if self.dirty {
            return Err(DatabaseError::TransactionInterrupted);
        }
        self.dirty = true;
        Ok(())
    }

    async fn control(
        &mut self,
        operation: &'static str,
        command: TransactionCommand,
    ) -> DatabaseResult<()> {
        self.start()?;
        let result = match command {
            TransactionCommand::Begin => {
                D::TransactionManager::begin(&mut self.connection, None).await
            }
            TransactionCommand::BeginImmediate => {
                D::TransactionManager::begin(
                    &mut self.connection,
                    Some(sqlx::AssertSqlSafe("BEGIN IMMEDIATE").into_sql_str()),
                )
                .await
            }
            TransactionCommand::Commit => D::TransactionManager::commit(&mut self.connection).await,
            TransactionCommand::RollBack => {
                D::TransactionManager::rollback(&mut self.connection).await
            }
        };
        self.finish(result)
            .map_err(DatabaseError::driver(operation))
    }
}

impl<D: Database> Drop for Lease<D> {
    fn drop(&mut self) {
        let depth = D::TransactionManager::get_transaction_depth(&self.connection);
        if self.retire || (matches!(D::NAME, "MySQL" | "PostgreSQL") && (self.dirty || depth > 0)) {
            // Discard unfinished protocol exchanges and active transactions before pooling.
            self.connection.close_on_drop();
        } else if D::NAME == "SQLite" {
            // The worker runs queued rollbacks before the pool's health check.
            // A cancelled commit/rollback may consume one queued command for its acknowledgement.
            for _ in 0..depth + usize::from(self.dirty) {
                D::TransactionManager::start_rollback(&mut self.connection);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum TransactionCommand {
    Begin,
    BeginImmediate,
    Commit,
    RollBack,
}

impl Pool {
    pub(super) fn retire(&self) {
        fn retire<D: Database>(pool: &sqlx::Pool<D>) {
            // close() marks the pool closed immediately. Awaiting it would wait for active leases.
            drop(pool.close());
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let pool = pool.clone();
                runtime.spawn(async move { pool.close().await });
            }
        }
        match self {
            Self::Sqlite(pool) => retire(pool),
            Self::Postgres(pool, _) => retire(pool),
            Self::SqlServer(pool) => pool.retire(),
            Self::Mysql(pools) => pools.iter().for_each(retire),
        }
    }

    pub(super) async fn acquire(&self) -> DatabaseResult<PhysicalConnection> {
        match self {
            Self::Sqlite(pool) => Ok(PhysicalConnection::Sqlite(Lease::acquire(pool).await?)),
            Self::Postgres(pool, emulate) => Ok(PhysicalConnection::Postgres(
                Lease::acquire(pool).await?,
                *emulate,
            )),
            Self::SqlServer(pool) => Ok(PhysicalConnection::SqlServer(Box::new(
                Box::pin(pool.acquire()).await?,
            ))),
            Self::Mysql(pools) => {
                let (last, alternatives) = pools
                    .split_last()
                    .expect("at least one configured MySQL host");
                for pool in alternatives {
                    if let Ok(lease) = Lease::acquire(pool).await {
                        return Ok(PhysicalConnection::Mysql(lease));
                    }
                }
                Ok(PhysicalConnection::Mysql(Lease::acquire(last).await?))
            }
        }
    }
}

macro_rules! bind {
    ($query:expr, $values:expr, $unsigned:expr) => {{
        let mut query = $query;
        for value in $values {
            query = match value {
                Value::Null => query.bind(None::<&str>),
                Value::Bool(value) => query.bind(*value),
                Value::Integer(value) => query.bind(*value),
                Value::Unsigned(value) => query.bind($unsigned(*value)?),
                Value::Real(value) => query.bind(*value),
                Value::Text(value) => query.bind(value.as_str()),
                Value::Bytes(value) => query.bind(value.as_slice()),
                Value::Json(value) => query.bind(sqlx::types::Json(value)),
            };
        }
        query
    }};
}

fn bind_postgres<'a, 'b: 'a>(
    statement: &'a sqlx::postgres::PgStatement,
    values: impl Iterator<Item = &'b Value>,
) -> DatabaseResult<sqlx::query::Query<'a, Postgres, sqlx::postgres::PgArguments>> {
    fn integer_binding<T: TryFrom<i64>>(value: i64) -> DatabaseResult<T> {
        T::try_from(value).map_err(|_| DatabaseError::InvalidQuery("PostgreSQL integer parameter"))
    }

    let types = match statement.parameters() {
        Some(sqlx::Either::Left(types)) => types,
        _ => &[],
    };
    let mut query = statement.query();
    for (position, value) in values.enumerate() {
        let kind = types.get(position).map(sqlx::TypeInfo::name);
        if matches!(kind, Some("TEXT" | "VARCHAR" | "BPCHAR" | "NAME")) {
            // 按服务端推断的参数类型编码；整数的二进制字节不能直接作为 UTF-8 文本发送。
            let text = match value {
                Value::Null => None,
                Value::Text(value) => Some(value.clone()),
                Value::Integer(value) => Some(value.to_string()),
                Value::Unsigned(value) => Some(value.to_string()),
                Value::Real(value) => Some(value.to_string()),
                Value::Bool(value) => Some(u8::from(*value).to_string()),
                Value::Json(value) => Some(value.to_string()),
                Value::Bytes(value) => Some(
                    String::from_utf8(value.clone())
                        .map_err(|_| DatabaseError::InvalidQuery("PostgreSQL text parameter"))?,
                ),
            };
            query = query.bind(text);
            continue;
        }
        if kind == Some("NUMERIC") {
            let decimal = match value {
                Value::Null => None,
                Value::Integer(value) => Some(bigdecimal::BigDecimal::from(*value)),
                Value::Unsigned(value) => Some(bigdecimal::BigDecimal::from(*value)),
                Value::Real(value) if value.is_finite() => Some(
                    value
                        .to_string()
                        .parse::<bigdecimal::BigDecimal>()
                        .map_err(|_| DatabaseError::InvalidQuery("PostgreSQL numeric parameter"))?,
                ),
                Value::Text(value) => Some(
                    value
                        .trim()
                        .parse::<bigdecimal::BigDecimal>()
                        .map_err(|_| DatabaseError::InvalidQuery("PostgreSQL numeric parameter"))?,
                ),
                _ => return Err(DatabaseError::InvalidQuery("PostgreSQL numeric parameter")),
            };
            query = query.bind(decimal);
            continue;
        }
        let integer = match value {
            Value::Integer(value) => Some(*value),
            Value::Text(value) if matches!(kind, Some("INT2" | "INT4" | "INT8")) => Some(
                value
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| DatabaseError::InvalidQuery("PostgreSQL integer parameter"))?,
            ),
            Value::Unsigned(value) => Some(
                i64::try_from(*value)
                    .map_err(|_| DatabaseError::InvalidQuery("PostgreSQL integer parameter"))?,
            ),
            _ => None,
        };
        if let Some(value) = integer {
            query = match kind {
                Some("INT2") => query.bind(integer_binding::<i16>(value)?),
                Some("INT4") => query.bind(integer_binding::<i32>(value)?),
                _ => query.bind(value),
            };
        } else if let (Value::Text(value), Some("TIMESTAMP")) = (value, kind) {
            let format = if value.contains('.') {
                time::macros::format_description!(
                    "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond]"
                )
            } else {
                time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]")
            };
            let timestamp = time::PrimitiveDateTime::parse(value, format)
                .map_err(|_| DatabaseError::InvalidQuery("PostgreSQL timestamp parameter"))?;
            query = query.bind(timestamp);
        } else {
            query = bind!(query, [value], sqlite_unsigned);
        }
    }
    Ok(query)
}

macro_rules! bind_statement {
    ($statement:ident, $values:ident, postgres) => {
        bind_postgres(&$statement, $values)?
    };
    ($statement:ident, $values:ident, mysql) => {
        bind!($statement.query(), $values, Ok::<u64, DatabaseError>)
    };
    ($statement:ident, $values:ident, sqlite) => {
        bind!($statement.query(), $values, sqlite_unsigned)
    };
}

fn sqlite_unsigned(value: u64) -> DatabaseResult<i64> {
    i64::try_from(value)
        .map_err(|_| DatabaseError::InvalidConfiguration("SQLite integer parameter"))
}

fn validate_bindings(statement: &impl sqlx::Statement, actual: usize) -> DatabaseResult<()> {
    let expected = statement.parameters().map(|parameters| match parameters {
        sqlx::Either::Left(types) => types.len(),
        sqlx::Either::Right(count) => count,
    });
    match expected {
        Some(expected) if expected != actual => {
            Err(DatabaseError::InvalidBindings { expected, actual })
        }
        _ => Ok(()),
    }
}

macro_rules! prepare_query {
    ($lease:ident, $sql:ident, $bindings:ident) => {{
        $lease.start()?;
        let prepared = $lease
            .connection
            .prepare(sqlx::AssertSqlSafe($sql).into_sql_str())
            .await;
        let statement = $lease
            .finish(prepared)
            .map_err(DatabaseError::driver("prepare"))?;
        validate_bindings(&statement, $bindings.len())?;
        statement
    }};
}

macro_rules! execute_query {
    ($lease:ident, $sql:ident, $bindings:ident, $binding:ident, $method:ident, $operation:literal, $persistent:expr) => {{
        let statement = prepare_query!($lease, $sql, $bindings);
        let query = bind_statement!(statement, $bindings, $binding).persistent($persistent);
        $lease.start()?;
        let result = query.$method(&mut *$lease.connection).await;
        $lease
            .finish(result)
            .map_err(DatabaseError::driver($operation))?
    }};
}

macro_rules! cursor_rows {
    ($lease:ident, $sql:ident, $bindings:ident, $binding:ident, $decode:path, $persistent:expr) => {
        async_stream::try_stream! {
            let statement = async {
                Ok::<_, DatabaseError>(prepare_query!($lease, $sql, $bindings))
            }.await?;
            let query = (|| Ok::<_, DatabaseError>(bind_statement!(statement, $bindings, $binding)))()?;
            let query = query.persistent($persistent);
            $lease.start()?;
            let result = {
                let mut rows = query.fetch(&mut *$lease.connection);
                loop {
                    match rows.try_next().await {
                        Ok(Some(row)) => yield $decode(row)?,
                        Ok(None) => break Ok(()),
                        Err(error) => break Err(error),
                    }
                }
            };
            $lease.finish(result).map_err(DatabaseError::driver("cursor"))?;
        }
    };
}

macro_rules! sqlite_persistent {
    (execute) => {
        true
    };
    ($method:ident) => {
        false
    };
}

macro_rules! query {
    ($self:ident, $sql:ident, $bindings:ident, $method:ident, $operation:literal, $map:expr, $sqlite_map:expr, $mysql_map:expr, $pg_map:expr, $tds:expr) => {{
        let prepared = $self.prepare($sql, $bindings.into())?;
        let $sql = prepared.sql.as_ref();
        let $bindings = prepared.values();
        match $self {
            Self::Sqlite(lease) => {
                // SQLx caches SQLite result metadata across schema changes. Reprepare
                // the executing statement so a changed projection is decoded correctly.
                let result = execute_query!(
                    lease,
                    $sql,
                    $bindings,
                    sqlite,
                    $method,
                    $operation,
                    sqlite_persistent!($method)
                );
                $map(result, $sqlite_map)
            }
            Self::Mysql(lease) => {
                let result =
                    execute_query!(lease, $sql, $bindings, mysql, $method, $operation, true);
                $map(result, $mysql_map)
            }
            Self::Postgres(lease, emulate) => {
                let result = if *emulate {
                    let sql = prepared.emulate_postgres()?;
                    lease.start()?;
                    let result = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
                        .$method(&mut *lease.connection)
                        .await;
                    lease
                        .finish(result)
                        .map_err(DatabaseError::driver($operation))?
                } else {
                    execute_query!(lease, $sql, $bindings, postgres, $method, $operation, true)
                };
                $map(result, $pg_map)
            }
            Self::SqlServer(lease) => Box::pin($tds(lease, $sql, $bindings)).await,
        }
    }};
}

pub(super) struct StatementResult {
    pub(super) affected: u64,
    pub(super) insert_id: Value,
}

impl PhysicalConnection {
    pub(super) fn prepare<'a>(
        &self,
        sql: &'a str,
        bindings: Bindings<'a>,
    ) -> DatabaseResult<PreparedBindings<'a>> {
        if matches!(self, Self::Postgres(..)) {
            return bindings.prepare_postgres(sql);
        }
        let prepared = bindings.prepare(sql, matches!(self, Self::Mysql(_)))?;
        Ok(if matches!(self, Self::SqlServer(_)) {
            prepared.sql_server()
        } else {
            prepared
        })
    }

    pub(super) async fn control(
        &mut self,
        operation: &'static str,
        command: TransactionCommand,
    ) -> DatabaseResult<()> {
        match self {
            Self::Sqlite(lease) => lease.control(operation, command).await,
            Self::Mysql(lease) => lease.control(operation, command).await,
            Self::Postgres(lease, _) => lease.control(operation, command).await,
            Self::SqlServer(lease) => Box::pin(lease.control(command)).await,
        }
    }

    pub(super) fn retire_on_drop(&mut self, retire: bool) {
        match self {
            Self::Sqlite(lease) => lease.retire = retire,
            Self::Mysql(lease) => lease.retire = retire,
            Self::Postgres(lease, _) => lease.retire = retire,
            Self::SqlServer(lease) => lease.retire = retire,
        }
    }

    pub(super) fn in_transaction(&self) -> bool {
        match self {
            Self::Sqlite(lease) => {
                <Sqlite as Database>::TransactionManager::get_transaction_depth(&lease.connection)
                    > 0
            }
            Self::Mysql(lease) => {
                <MySql as Database>::TransactionManager::get_transaction_depth(&lease.connection)
                    > 0
            }
            Self::Postgres(lease, _) => {
                <Postgres as Database>::TransactionManager::get_transaction_depth(&lease.connection)
                    > 0
            }
            Self::SqlServer(lease) => lease.depth > 0,
        }
    }

    pub(super) fn interrupted(&self) -> bool {
        match self {
            Self::Sqlite(lease) => lease.dirty,
            Self::Mysql(lease) => lease.dirty,
            Self::Postgres(lease, _) => lease.dirty,
            Self::SqlServer(lease) => lease.dirty,
        }
    }

    pub(super) fn cursor<'a>(
        &'a mut self,
        prepared: &'a PreparedBindings<'_>,
    ) -> impl Stream<Item = DatabaseResult<Row>> + Send + 'a {
        let sql = prepared.sql.as_ref();
        let bindings = prepared.values();
        match self {
            Self::Sqlite(lease) => Either::Left(Either::Left(cursor_rows!(
                lease,
                sql,
                bindings,
                sqlite,
                value::sqlite,
                false
            ))),
            Self::Mysql(lease) => Either::Left(Either::Right(cursor_rows!(
                lease,
                sql,
                bindings,
                mysql,
                value::mysql,
                true
            ))),
            Self::Postgres(lease, false) => Either::Right(Either::Left(Either::Left(
                cursor_rows!(lease, sql, bindings, postgres, value::postgres, true),
            ))),
            Self::Postgres(lease, true) => {
                Either::Right(Either::Left(Either::Right(async_stream::try_stream! {
                    let sql = prepared.emulate_postgres()?;
                    lease.start()?;
                    let result = {
                        let mut rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
                            .fetch(&mut *lease.connection);
                        loop {
                            match rows.try_next().await {
                                Ok(Some(row)) => yield value::postgres(row)?,
                                Ok(None) => break Ok(()),
                                Err(error) => break Err(error),
                            }
                        }
                    };
                    lease.finish(result).map_err(DatabaseError::driver("cursor"))?;
                })))
            }
            Self::SqlServer(lease) => {
                Either::Right(Either::Right(Box::pin(lease.cursor(sql, bindings))))
            }
        }
    }

    pub(super) async fn select<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Row>> {
        query!(
            self,
            sql,
            bindings,
            fetch_all,
            "select",
            |rows: Vec<_>, decode| rows.into_iter().map(decode).collect(),
            value::sqlite,
            value::mysql,
            value::postgres,
            super::sql_server::Lease::fetch_all
        )
    }

    pub(super) async fn select_one<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Option<Row>> {
        query!(
            self,
            sql,
            bindings,
            fetch_optional,
            "select",
            |row: Option<_>, decode| row.map(decode).transpose(),
            value::sqlite,
            value::mysql,
            value::postgres,
            super::sql_server::Lease::fetch_optional
        )
    }

    pub(super) async fn select_result_sets<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Vec<Row>>> {
        if let Self::SqlServer(lease) = self {
            let prepared = bindings.into().prepare(sql, false)?.sql_server();
            return Box::pin(lease.select(&prepared.sql, prepared.values())).await;
        }
        let Self::Mysql(lease) = self else {
            return Err(DatabaseError::UnsupportedOperation("selectResultSets"));
        };
        let prepared = bindings.into().prepare(sql, true)?;
        let sql = prepared.sql.as_ref();
        let bindings = prepared.values();
        let statement = prepare_query!(lease, sql, bindings);
        let query = bind!(statement.query(), bindings, Ok::<u64, DatabaseError>);
        lease.start()?;
        let mut sets = Vec::new();
        let mut rows = Vec::new();
        let result = {
            let mut packets = (&mut *lease.connection).fetch_many(query);
            loop {
                match packets.try_next().await {
                    Ok(Some(sqlx::Either::Right(row))) => rows.push(value::mysql(row)?),
                    // Both row-set delimiters and MySQL's final CALL status produce a PDO rowset.
                    Ok(Some(sqlx::Either::Left(_))) => sets.push(std::mem::take(&mut rows)),
                    Ok(None) => break Ok(sets),
                    Err(error) => break Err(error),
                }
            }
        };
        lease
            .finish(result)
            .map_err(DatabaseError::driver("select result sets"))
    }

    pub(super) async fn affecting_statement<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<u64> {
        self.execute_statement(sql, bindings)
            .await
            .map(|result| result.affected)
    }

    pub(super) async fn execute_statement<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<StatementResult> {
        query!(
            self,
            sql,
            bindings,
            execute,
            "execute",
            |result, map: fn(_) -> StatementResult| Ok(map(result)),
            |result: sqlx::sqlite::SqliteQueryResult| StatementResult {
                affected: result.rows_affected(),
                insert_id: result.last_insert_rowid().into(),
            },
            |result: sqlx::mysql::MySqlQueryResult| StatementResult {
                affected: result.rows_affected(),
                insert_id: i64::try_from(result.last_insert_id())
                    .map_or_else(|_| Value::Unsigned(result.last_insert_id()), Value::Integer,),
            },
            |result: sqlx::postgres::PgQueryResult| StatementResult {
                affected: result.rows_affected(),
                insert_id: Value::Null,
            },
            super::sql_server::Lease::statement
        )
    }

    pub(super) async fn unprepared(&mut self, sql: &str) -> DatabaseResult<bool> {
        async fn execute<D: Database>(lease: &mut Lease<D>, sql: &str) -> DatabaseResult<bool>
        where
            for<'a> &'a mut D::Connection: sqlx::Executor<'a, Database = D>,
        {
            lease.start()?;
            let result = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                .execute(&mut *lease.connection)
                .await;
            lease
                .finish(result)
                .map(|_| true)
                .map_err(DatabaseError::driver("execute"))
        }
        match self {
            Self::Sqlite(lease) => execute(lease, sql).await,
            Self::Mysql(lease) => execute(lease, sql).await,
            Self::Postgres(lease, _) => execute(lease, sql).await,
            Self::SqlServer(lease) => Box::pin(lease.raw(sql)).await,
        }
    }
}

macro_rules! observed_query {
    ($connection:ident, $sql:ident, $bindings:expr, $read:expr,
      |$physical:ident, $values:ident| $operation:expr, $preview:expr, $before:expr, $after:expr) => {{
        let $values: Bindings<'_> = $bindings.into();
        $connection
            .run_before_executing($sql, $values, None)
            .await?;
        if $connection.pretending() {
            $connection.preview_query($sql, $values, $read).await?;
            return Ok($preview);
        }
        let mut replayed = false;
        let mut elapsed = Duration::ZERO;
        loop {
            let QueryLease {
                physical: mut $physical,
                role,
                wait,
                generation,
            } = $connection.acquire_query($read).await?;
            let start = Instant::now();
            $physical.validate($sql, true)?;
            $before();
            let result = $operation.await;
            elapsed += wait + start.elapsed();
            drop($physical);
            match result {
                Ok(result) => {
                    $after(&result);
                    $connection
                        .dispatch_query($sql, $values, elapsed, role)
                        .await?;
                    break Ok::<_, DatabaseError>(result);
                }
                Err(error)
                    if $connection.retry_lost_connection(
                        generation.as_ref(),
                        &error,
                        &mut replayed,
                    ) => {}
                Err(error) => break Err(error),
            }
        }
    }};
}

#[allow(non_snake_case, reason = "Matches Laravel's Connection API")]
impl Connection {
    pub(super) fn model_container(
        connection: Option<&Self>,
    ) -> Option<Arc<crate::container::Container>> {
        match connection {
            Some(connection) => connection.model_container.upgrade(),
            None => crate::support::facades::current_container(),
        }
    }

    /// Registers a hook before each logical SQL execution, including the first poll of a cursor.
    pub fn beforeExecuting(
        &self,
        callback: impl for<'a> Fn(
            &'a str,
            Bindings<'a>,
            &'a Connection,
            Option<&'a mut Transaction>,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync
        + 'static,
    ) -> &Self {
        self.before_executing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::new(callback));
        self
    }

    pub(super) async fn run_before_executing(
        &self,
        sql: &str,
        bindings: Bindings<'_>,
        mut transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<()> {
        let callbacks = self
            .before_executing
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if callbacks.is_empty() {
            return Ok(());
        }
        let work = async {
            for callback in callbacks {
                callback(sql, bindings, self, transaction.as_deref_mut()).await?;
            }
            Ok(())
        };
        match Self::model_container(Some(self)) {
            Some(owner) => application_scope(owner, work).await,
            None => work.await,
        }
    }

    /// Retires the current pools while leaving the logical connection intact.
    pub fn disconnect(&self) {
        self.pools.disconnect();
    }

    /// Delivers `ConnectionEstablished` once for the current pool generation.
    pub async fn establish(&self) -> DatabaseResult<()> {
        let generation = self.pools.generation()?;
        self.establish_generation(&generation).await
    }

    async fn establish_generation(&self, generation: &Arc<PoolGeneration>) -> DatabaseResult<()> {
        if generation.established.initialized() {
            return Ok(());
        }

        if is_establishing(generation) {
            return Ok(());
        }

        let identity = Arc::as_ptr(generation).addr();
        let mut result = Ok(());
        generation
            .established
            .get_or_init(|| async {
                let mut connections = ESTABLISHING_CONNECTIONS
                    .try_with(Clone::clone)
                    .unwrap_or_default();
                connections.push(identity);
                result = ESTABLISHING_CONNECTIONS
                    .scope(connections, self.fire_established())
                    .await;
            })
            .await;
        result
    }

    async fn fire_established(&self) -> DatabaseResult<()> {
        if test_events::muted(self) {
            return Ok(());
        }
        let Some(owner) = Self::model_container(Some(self)).filter(|owner| owner.has("events"))
        else {
            return Ok(());
        };
        let dispatcher = owner.get::<Dispatcher>("events")?;
        if dispatcher.observes(ConnectionEstablished::NAME) {
            let event = Arc::new(ConnectionEstablished {
                connection: self.clone(),
            });
            let mut context = EventContext::new();
            application_scope(owner, dispatcher.dispatch(event, &mut context))
                .await
                .map_err(|source| DatabaseError::ConnectionEvent(Box::new(source)))?;
        }
        Ok(())
    }

    pub(super) async fn acquire(&self, read: bool) -> DatabaseResult<PhysicalConnection> {
        self.ensure_real_operation()?;
        if test_transaction::active(self) {
            return Err(DatabaseError::UnsupportedOperation(
                "this database operation requires a session outside the test transaction",
            ));
        }
        self.acquire_pooled_query(read)
            .await
            .map(|lease| lease.physical)
    }

    pub(super) fn ensure_real_operation(&self) -> DatabaseResult<()> {
        if self.pretending() {
            return Err(DatabaseError::UnsupportedOperation(
                "this database operation requires a live session",
            ));
        }
        Ok(())
    }

    pub(super) fn write_query_role(&self) -> QueryRole {
        self.query_role.unwrap_or(QueryRole::Write)
    }

    pub(super) async fn acquire_query(&self, read: bool) -> DatabaseResult<QueryLease> {
        if let Some(physical) = test_transaction::checkout(self)? {
            return Ok(QueryLease {
                physical,
                generation: None,
                role: QueryRole::Write,
                wait: Duration::ZERO,
            });
        }
        let QueryLease {
            physical,
            generation,
            role,
            wait,
        } = self.acquire_pooled_query(read).await?;
        Ok(QueryLease {
            physical: QueryPhysical::Pooled(physical),
            generation,
            role,
            wait,
        })
    }

    async fn acquire_pooled_query(
        &self,
        read: bool,
    ) -> DatabaseResult<QueryLease<PhysicalConnection>> {
        let mut wait = Duration::ZERO;
        loop {
            let generation = self.pools.generation()?;
            self.establish_generation(&generation).await?;
            let (pool, role) = match &generation.reader {
                Some(reader) if read && !self.reads_from_writer() => (reader, QueryRole::Read),
                _ => (&generation.writer, self.write_query_role()),
            };
            let start = Instant::now();
            let result = pool.acquire().await;
            wait += start.elapsed();
            match result {
                Err(DatabaseError::Driver {
                    source: sqlx::Error::PoolClosed,
                    ..
                }) => continue,
                result => {
                    return result.map(|physical| QueryLease {
                        physical,
                        role,
                        wait,
                        generation: Some(generation),
                    });
                }
            }
        }
    }

    pub(super) fn retry_lost_connection(
        &self,
        generation: Option<&Arc<PoolGeneration>>,
        error: &DatabaseError,
        replayed: &mut bool,
    ) -> bool {
        let Some(generation) = generation else {
            return false;
        };
        if *replayed || is_establishing(generation) {
            return false;
        }
        if !matches!(error, DatabaseError::Driver { source, .. }
            if LostConnectionDetector.causedByLostConnection(source))
        {
            return false;
        }
        self.pools.retire_if_current(generation);
        *replayed = true;
        true
    }

    /// Subscribes to queries on this application's database connections.
    pub fn listen(&self, listener: Arc<dyn Listener>) -> DatabaseResult<()> {
        let Some(owner) = Self::model_container(Some(self)).filter(|owner| owner.has("events"))
        else {
            return Ok(());
        };
        let dispatcher = owner.get::<Dispatcher>("events")?;
        dispatcher
            .listen(QueryExecuted::NAME, listener, Delivery::Sync)
            .map_err(|source| DatabaseError::ConnectionEvent(Box::new(source)))
    }

    pub fn totalQueryDuration(&self) -> Duration {
        self.query_duration.total()
    }

    pub fn resetTotalQueryDuration(&self) {
        self.query_duration.reset();
    }

    pub fn allowQueryDurationHandlersToRunAgain(&self) {
        self.query_duration.rearm();
    }

    /// Runs the listener once when this connection's cumulative query time exceeds the threshold.
    pub fn whenQueryingForLongerThan(
        &self,
        threshold: Duration,
        listener: Arc<dyn Listener>,
    ) -> DatabaseResult<()> {
        self.listen(Arc::new(QueryDurationHandler::new(
            &self.query_duration,
            threshold,
            listener,
        )))
    }

    pub fn enableQueryLog(&self) {
        self.query_log.set_enabled(true);
    }

    pub fn disableQueryLog(&self) {
        self.query_log.set_enabled(false);
    }

    pub fn logging(&self) -> bool {
        self.query_log.enabled()
    }

    pub fn flushQueryLog(&self) {
        self.query_log.clear();
    }

    pub fn getQueryLog(&self) -> Vec<QueryLogEntry> {
        self.query_log.snapshot()
    }

    pub fn getRawQueryLog(&self) -> DatabaseResult<Vec<RawQueryLogEntry>> {
        self.getQueryLog()
            .iter()
            .map(|entry| {
                let raw_query = render_query(&entry.query, &entry.bindings, !self.is_sqlite())?;
                Ok(RawQueryLogEntry {
                    raw_query,
                    time: entry.time,
                })
            })
            .collect()
    }

    /// Collects queries within this future without executing them.
    pub async fn pretend(
        &self,
        callback: impl for<'a> FnOnce(&'a Self) -> DatabaseFuture<'a, DatabaseResult<()>> + Send,
    ) -> DatabaseResult<Vec<QueryLogEntry>> {
        let log = Arc::new(QueryLog::default());
        log.set_enabled(true);
        PreviewScope {
            connection: self.preview_identity(),
            dry: true,
            log: log.clone(),
        }
        .run(async { callback(self).await })
        .await?;
        Ok(log.snapshot())
    }

    /// Allows real queries in the given future while retaining the preview log.
    pub async fn withoutPretending<T>(&self, callback: impl Future<Output = T>) -> T {
        match PreviewScope::current(self.preview_identity()) {
            Some(scope) if scope.dry => {
                PreviewScope {
                    dry: false,
                    ..scope
                }
                .run(callback)
                .await
            }
            _ => callback.await,
        }
    }

    pub fn pretending(&self) -> bool {
        PreviewScope::current(self.preview_identity()).is_some_and(|scope| scope.dry)
    }

    fn preview_identity(&self) -> usize {
        Arc::as_ptr(&self.routing).addr()
    }

    /// Records a completed query after its event has been delivered.
    pub(super) fn record_query(&self, entry: impl FnOnce() -> QueryLogEntry) {
        if let Some(scope) = PreviewScope::current(self.preview_identity()) {
            let mut entry = entry();
            if scope.dry {
                if let Ok(raw) = render_query(&entry.query, &entry.bindings, !self.is_sqlite()) {
                    entry.query = raw;
                }
                scope.log.push(|| entry);
            } else {
                self.query_log.push(|| entry.clone());
                scope.log.push(|| entry);
            }
        } else {
            self.query_log.push(entry);
        }
    }

    async fn preview_query(
        &self,
        sql: &str,
        bindings: Bindings<'_>,
        read: bool,
    ) -> DatabaseResult<()> {
        let role = if read && !self.reads_from_writer() && self.pools.has_reader() {
            QueryRole::Read
        } else {
            self.write_query_role()
        };
        self.dispatch_query(sql, bindings, Duration::ZERO, role)
            .await
    }

    pub(super) fn observe_query(
        &self,
        sql: &str,
        bindings: Bindings<'_>,
        time: Duration,
        read_write_type: QueryRole,
    ) -> DatabaseResult<Option<QueryNotification>> {
        self.query_duration.add(time);
        let muted = test_events::muted(self);
        if !muted
            && let Some(counts) = self
                .test_query_counts
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
        {
            counts.record(self);
        }
        if !muted
            && let Some(owner) =
                Self::model_container(Some(self)).filter(|owner| owner.has("events"))
        {
            let dispatcher = owner.get::<Dispatcher>("events")?;
            if dispatcher.observes(QueryExecuted::NAME) {
                return Ok(Some(QueryNotification {
                    owner,
                    dispatcher,
                    event: Arc::new(QueryExecuted {
                        connection: self.clone(),
                        sql: sql.into(),
                        bindings: bindings.into(),
                        time,
                        read_write_type,
                    }),
                }));
            }
        }
        self.record_query(|| QueryLogEntry {
            query: sql.into(),
            bindings: bindings.into(),
            time,
            read_write_type,
        });
        Ok(None)
    }

    async fn dispatch_query(
        &self,
        sql: &str,
        bindings: Bindings<'_>,
        time: Duration,
        role: QueryRole,
    ) -> DatabaseResult<()> {
        if let Some(notification) = self.observe_query(sql, bindings, time, role)? {
            notification.dispatch(&mut EventContext::new()).await?;
        }
        Ok(())
    }

    pub fn getSchemaBuilder(&self) -> super::schema::Builder<'static> {
        super::schema::Builder::connection(self.clone())
    }

    pub fn table(&self, table: &str) -> super::query::Builder<'static> {
        super::query::Builder::connection(self.clone(), self.grammar.clone(), table)
    }

    pub(crate) fn same_query_source(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.routing, &other.routing)
    }

    pub(crate) fn is_sqlite(&self) -> bool {
        self.sqlite
    }

    pub(crate) fn is_postgres(&self) -> bool {
        self.grammar.is_postgres()
    }

    pub(crate) fn is_mariadb(&self) -> bool {
        self.grammar.is_mariadb()
    }

    pub(crate) fn is_sql_server(&self) -> bool {
        self.grammar.is_sql_server()
    }

    pub(crate) fn queue_table(&self, table: &str) -> DatabaseResult<String> {
        self.grammar.table(table)
    }

    pub(crate) async fn insert_get_id(
        &self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Value> {
        if self.grammar.is_postgres() || self.grammar.is_sql_server() {
            return observed_query!(
                self,
                sql,
                bindings,
                false,
                |physical, values| physical.select_one(sql, values),
                Value::Null,
                || self.setRecordModificationState(true),
                |_| ()
            )
            .and_then(|row| row.map_or(Ok(Value::Null), Row::scalar));
        }
        observed_query!(
            self,
            sql,
            bindings,
            false,
            |physical, values| physical.execute_statement(sql, values),
            Value::Null,
            || self.setRecordModificationState(true),
            |_| ()
        )
        .map(|result| result.insert_id)
    }

    pub(super) async fn drop_mysql_schema_objects(
        &self,
        drop_tables: bool,
        drop_views: bool,
    ) -> DatabaseResult<()> {
        let mut physical = self.acquire(false).await?;
        let connection = self.clone();
        let result = tokio::spawn(async move {
            let checks = if drop_tables {
                let checks = physical.select_one("select @@foreign_key_checks", &[]).await?
                    .and_then(|row| row.get_at(0).cloned());
                let restore = match checks {
                    Some(Value::Integer(1) | Value::Unsigned(1) | Value::Bool(true)) => true,
                    Some(Value::Integer(0) | Value::Unsigned(0) | Value::Bool(false)) => false,
                    _ => return Err(DatabaseError::InvalidSchema("MySQL foreign key check state")),
                };
                physical.retire_on_drop(true);
                physical.unprepared("set foreign_key_checks = 0").await?;
                Some(restore)
            } else {
                None
            };
            let result = async {
                let rows = physical.select(
                    "select table_schema, table_name, table_type from information_schema.tables                      where table_schema = database() order by table_name",
                    &[],
                ).await?;
                let mut tables = Vec::new();
                let mut views = Vec::new();
                for row in &rows {
                    let [schema, name, kind] = [0, 1, 2].map(|index| match row.get_at(index) {
                        Some(Value::Text(value)) => Ok(value.as_str()),
                        Some(Value::Bytes(value)) => std::str::from_utf8(value)
                            .map_err(|_| DatabaseError::InvalidSchema("catalog encoding")),
                        _ => Err(DatabaseError::InvalidSchema("catalog value type")),
                    });
                    let target = format!("{}.{}", connection.grammar.quote(schema?)?, connection.grammar.quote(name?)?);
                    match kind? {
                        "BASE TABLE" | "SYSTEM VERSIONED" if drop_tables => tables.push(target),
                        "VIEW" if drop_views => views.push(target),
                        _ => {}
                    }
                }
                for (kind, names) in [("view", views), ("table", tables)] {
                    if !names.is_empty() {
                        physical.unprepared(&format!("drop {kind} {}", names.join(", "))).await?;
                    }
                }
                Ok(())
            }.await;
            if let Some(checks) = checks {
                physical.unprepared(if checks { "set foreign_key_checks = 1" } else { "set foreign_key_checks = 0" }).await?;
                physical.retire_on_drop(false);
            }
            result
        }).await.map_err(|_| DatabaseError::InvalidSchema("MySQL schema task interrupted"))?;
        if result.is_ok() {
            self.setRecordModificationState(true);
        }
        result
    }

    #[cfg(feature = "sanctum")]
    pub(crate) fn model_timezone(&self) -> jiff::tz::TimeZone {
        self.model_timezone.clone()
    }

    pub fn getName(&self) -> &str {
        &self.name
    }

    /// Returns a value from the configuration used to construct this connection.
    pub fn getConfig(&self, key: &str) -> Option<serde_json::Value> {
        let mut value = self.config.as_ref();
        for segment in key.split('.') {
            value = value.get(segment)?;
        }
        Some(value.clone())
    }

    /// Returns the configured database driver's Laravel name.
    pub fn getDriverName(&self) -> &'static str {
        if self.grammar.is_mariadb() {
            "mariadb"
        } else if self.grammar.is_mysql() {
            "mysql"
        } else if self.grammar.is_postgres() {
            "pgsql"
        } else if self.grammar.is_sql_server() {
            "sqlsrv"
        } else {
            "sqlite"
        }
    }

    pub fn getDatabaseName(&self) -> &str {
        &self.database
    }

    /// Reports whether PostgreSQL has a separate direct endpoint configured.
    pub fn hasDirectConnection(&self) -> bool {
        self.has_direct_connection
    }

    /// Returns the prefix applied to logical table names on this connection.
    pub fn getTablePrefix(&self) -> &str {
        self.grammar.table_prefix()
    }

    fn routing(&self) -> Arc<ReadRouting> {
        crate::support::facades::database_routing(&self.routing)
            .unwrap_or_else(|| self.routing.clone())
    }

    /// Carries this scope's read-routing state into a separate database task.
    #[cfg(windows)]
    pub(crate) fn with_captured_routing(&self) -> Self {
        let mut connection = self.clone();
        connection.routing = self.routing();
        connection
    }

    pub fn hasModifiedRecords(&self) -> bool {
        self.routing().records_modified.load(Ordering::Relaxed)
    }

    pub fn setRecordModificationState(&self, value: bool) {
        self.routing()
            .records_modified
            .store(value, Ordering::Relaxed);
    }

    pub fn forgetRecordModificationState(&self) {
        self.setRecordModificationState(false);
    }

    pub fn useWriteConnectionWhenReading(&self, value: bool) {
        self.routing().read_on_write.store(value, Ordering::Relaxed);
    }

    fn reads_from_writer(&self) -> bool {
        let routing = self.routing();
        routing.read_on_write.load(Ordering::Relaxed)
            || (self.sticky && routing.records_modified.load(Ordering::Relaxed))
    }

    /// Executes the select on first poll and decodes one row at a time.
    pub fn cursor<'a>(
        &'a self,
        sql: &'a str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseCursor<'a> {
        self.cursor_with_mode(sql, bindings, false)
    }

    pub(super) fn cursor_with_mode<'a>(
        &'a self,
        sql: &'a str,
        bindings: impl Into<Bindings<'a>>,
        write: bool,
    ) -> DatabaseCursor<'a> {
        let bindings = bindings.into();
        Box::pin(async_stream::try_stream! {
            self.run_before_executing(sql, bindings, None).await?;
            if self.pretending() {
                self.preview_query(sql, bindings, !write).await?;
                return;
            }
            let mut replayed = false;
            let mut elapsed = Duration::ZERO;
            let mut delivered = false;
            loop {
                let QueryLease { mut physical, role, wait, generation } = self.acquire_query(!write).await?;
                physical.validate(sql, true)?;
                elapsed += wait;
                let result = {
                    let prepared = physical.prepare(sql, bindings)?;
                    let mut rows = std::pin::pin!(physical.cursor(&prepared));
                    loop {
                        let start = Instant::now();
                        let next = rows.try_next().await;
                        elapsed += start.elapsed();
                        match next {
                            Ok(Some(row)) => {
                                delivered = true;
                                yield row;
                            }
                            Ok(None) => break Ok(()),
                            Err(error) => break Err(error),
                        }
                    }
                };
                drop(physical);
                match result {
                    Ok(()) => {
                        self.dispatch_query(sql, bindings, elapsed, role).await?;
                        break;
                    }
                    Err(error) if !delivered && self.retry_lost_connection(generation.as_ref(), &error, &mut replayed) => {}
                    Err(error) => Err(error)?,
                }
            }
        })
    }

    pub async fn select<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Row>> {
        observed_query!(
            self,
            sql,
            bindings,
            true,
            |physical, values| physical.select(sql, values),
            Vec::new(),
            || (),
            |_| ()
        )
    }

    pub async fn selectFromWriteConnection<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Row>> {
        observed_query!(
            self,
            sql,
            bindings,
            false,
            |physical, values| physical.select(sql, values),
            Vec::new(),
            || (),
            |_| ()
        )
    }

    pub(super) async fn insert_returning(
        &self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Vec<Row>> {
        observed_query!(
            self,
            sql,
            bindings,
            false,
            |physical, values| physical.select(sql, values),
            Vec::new(),
            || (),
            |rows: &Vec<Row>| {
                if !rows.is_empty() {
                    self.setRecordModificationState(true);
                }
            }
        )
    }

    /// Returns the ordered rowsets of a prepared MySQL-family or SQL Server statement.
    pub async fn selectResultSets<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
        use_read: bool,
    ) -> DatabaseResult<Vec<Vec<Row>>> {
        observed_query!(
            self,
            sql,
            bindings,
            use_read,
            |physical, values| physical.select_result_sets(sql, values),
            Vec::new(),
            || (),
            |_| ()
        )
    }

    pub async fn selectOne<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Option<Row>> {
        self.select_one_with_role(sql, bindings, true).await
    }

    pub(super) async fn select_one_with_role<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
        use_read: bool,
    ) -> DatabaseResult<Option<Row>> {
        observed_query!(
            self,
            sql,
            bindings,
            use_read,
            |physical, values| physical.select_one(sql, values),
            None,
            || (),
            |_| ()
        )
    }

    pub async fn scalar<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Value> {
        self.selectOne(sql, bindings)
            .await?
            .map_or(Ok(Value::Null), Row::scalar)
    }

    pub async fn statement<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<bool> {
        observed_query!(
            self,
            sql,
            bindings,
            false,
            |physical, values| physical.affecting_statement(sql, values),
            true,
            || self.setRecordModificationState(true),
            |_| ()
        )
        .map(|_| true)
    }

    pub async fn insert<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<bool> {
        self.statement(sql, bindings).await
    }

    pub async fn update<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<u64> {
        self.affectingStatement(sql, bindings).await
    }

    pub async fn delete<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<u64> {
        self.affectingStatement(sql, bindings).await
    }

    pub async fn affectingStatement<'a>(
        &self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<u64> {
        observed_query!(
            self,
            sql,
            bindings,
            false,
            |physical, values| physical.affecting_statement(sql, values),
            0,
            || (),
            |count: &u64| {
                if *count > 0 {
                    self.setRecordModificationState(true);
                }
            }
        )
    }

    /// Executes raw SQL without parameter binding. Use only application-controlled SQL.
    pub async fn unprepared(&self, sql: &str) -> DatabaseResult<bool> {
        if test_transaction::active(self) {
            return Err(DatabaseError::UnsupportedOperation(
                "test transaction requires a prepared transactional statement",
            ));
        }
        observed_query!(
            self,
            sql,
            &[],
            false,
            |physical, _bindings| physical.unprepared(sql),
            true,
            || (),
            |_| self.setRecordModificationState(true)
        )
    }
}
