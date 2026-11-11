use sqlx::SqlSafeStr as _;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use sqlx::{
    Database, Executor as _, MySql, MySqlPool, Sqlite, SqlitePool, Statement as _,
    pool::PoolConnection,
};

use sqlx_core::transaction::TransactionManager;

use super::{DatabaseError, DatabaseResult, Row, Value, value};

#[derive(Clone)]
pub(super) enum Pool {
    Sqlite(SqlitePool),
    Mysql(Arc<[MySqlPool]>),
}

/// A named database connection sharing its application's pool.
#[derive(Clone)]
pub struct Connection {
    pub(super) name: Arc<str>,
    pub(super) prefix_indexes: bool,
    pub(super) grammar: super::query::grammars::Grammar,
    pub(super) database: Arc<str>,
    pub(super) pool: Pool,
    pub(super) read_pool: Option<(Arc<str>, Pool)>,
    pub(super) sticky: bool,
    pub(super) routing: Arc<ReadRouting>,
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
                || (D::NAME == "MySQL" && super::transaction::mysql_deadlock(error))
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
        if self.retire || (D::NAME == "MySQL" && (self.dirty || depth > 0)) {
            // An abandoned command may leave an incomplete MySQL protocol exchange.
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
    pub(super) async fn acquire(&self) -> DatabaseResult<PhysicalConnection> {
        match self {
            Self::Sqlite(pool) => Ok(PhysicalConnection::Sqlite(Lease::acquire(pool).await?)),
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

macro_rules! execute_query {
    ($lease:ident, $sql:ident, $bindings:ident, $unsigned:expr, $method:ident, $operation:literal) => {{
        $lease.start()?;
        let prepared = $lease
            .connection
            .prepare(sqlx::AssertSqlSafe($sql).into_sql_str())
            .await;
        let statement = $lease
            .finish(prepared)
            .map_err(DatabaseError::driver("prepare"))?;
        validate_bindings(&statement, $bindings.len())?;
        let query = bind!(statement.query(), $bindings, $unsigned);
        $lease.start()?;
        let result = query.$method(&mut *$lease.connection).await;
        $lease
            .finish(result)
            .map_err(DatabaseError::driver($operation))?
    }};
}

macro_rules! query {
    ($self:ident, $sql:ident, $bindings:ident, $method:ident, $operation:literal, $map:expr, $sqlite_map:expr, $mysql_map:expr) => {
        match $self {
            Self::Sqlite(lease) => {
                let result =
                    execute_query!(lease, $sql, $bindings, sqlite_unsigned, $method, $operation);
                $map(result, $sqlite_map)
            }
            Self::Mysql(lease) => {
                let result = execute_query!(
                    lease,
                    $sql,
                    $bindings,
                    Ok::<u64, DatabaseError>,
                    $method,
                    $operation
                );
                $map(result, $mysql_map)
            }
        }
    };
}

pub(super) struct StatementResult {
    pub(super) affected: u64,
    pub(super) insert_id: Value,
}

impl PhysicalConnection {
    pub(super) async fn control(
        &mut self,
        operation: &'static str,
        command: TransactionCommand,
    ) -> DatabaseResult<()> {
        match self {
            Self::Sqlite(lease) => lease.control(operation, command).await,
            Self::Mysql(lease) => lease.control(operation, command).await,
        }
    }

    pub(super) fn retire_on_drop(&mut self, retire: bool) {
        match self {
            Self::Sqlite(lease) => lease.retire = retire,
            Self::Mysql(lease) => lease.retire = retire,
        }
    }

    pub(super) fn interrupted(&self) -> bool {
        match self {
            Self::Sqlite(lease) => lease.dirty,
            Self::Mysql(lease) => lease.dirty,
        }
    }

    pub(super) async fn select(
        &mut self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Vec<Row>> {
        query!(
            self,
            sql,
            bindings,
            fetch_all,
            "select",
            |rows: Vec<_>, decode| rows.into_iter().map(decode).collect(),
            value::sqlite,
            value::mysql
        )
    }

    pub(super) async fn select_one(
        &mut self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Option<Row>> {
        query!(
            self,
            sql,
            bindings,
            fetch_optional,
            "select",
            |row: Option<_>, decode| row.map(decode).transpose(),
            value::sqlite,
            value::mysql
        )
    }

    pub(super) async fn affecting_statement(
        &mut self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<u64> {
        self.execute_statement(sql, bindings)
            .await
            .map(|result| result.affected)
    }

    pub(super) async fn execute_statement(
        &mut self,
        sql: &str,
        bindings: &[Value],
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
            }
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
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Connection API")]
impl Connection {
    pub fn getSchemaBuilder(&self) -> super::schema::Builder<'static> {
        super::schema::Builder::connection(self.clone())
    }

    pub fn table(&self, table: &str) -> super::query::Builder<'static> {
        super::query::Builder::connection(self.clone(), self.grammar.clone(), table)
    }

    pub(super) fn same_query_source(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.routing, &other.routing)
    }

    pub(super) async fn insert_get_id(
        &self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Value> {
        let mut connection = self.pool.acquire().await?;
        self.setRecordModificationState(true);
        connection
            .execute_statement(sql, bindings)
            .await
            .map(|result| result.insert_id)
    }

    pub fn getName(&self) -> &str {
        &self.name
    }

    pub fn getDatabaseName(&self) -> &str {
        &self.database
    }

    fn routing(&self) -> Arc<ReadRouting> {
        crate::support::facades::database_routing(&self.routing)
            .unwrap_or_else(|| self.routing.clone())
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

    fn read_pool(&self) -> &Pool {
        if self.routing().read_on_write.load(Ordering::Relaxed)
            || (self.sticky && self.hasModifiedRecords())
        {
            return &self.pool;
        }
        self.read_pool.as_ref().map_or(&self.pool, |(_, pool)| pool)
    }

    pub async fn select(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<Vec<Row>> {
        self.read_pool()
            .acquire()
            .await?
            .select(sql, bindings)
            .await
    }

    pub async fn selectFromWriteConnection(
        &self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Vec<Row>> {
        self.pool.acquire().await?.select(sql, bindings).await
    }

    pub async fn selectOne(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<Option<Row>> {
        self.read_pool()
            .acquire()
            .await?
            .select_one(sql, bindings)
            .await
    }

    pub async fn scalar(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<Value> {
        self.selectOne(sql, bindings)
            .await?
            .map_or(Ok(Value::Null), Row::scalar)
    }

    pub async fn statement(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<bool> {
        let mut connection = self.pool.acquire().await?;
        self.setRecordModificationState(true);
        connection
            .affecting_statement(sql, bindings)
            .await
            .map(|_| true)
    }

    pub async fn insert(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<bool> {
        self.statement(sql, bindings).await
    }

    pub async fn update(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<u64> {
        self.affectingStatement(sql, bindings).await
    }

    pub async fn delete(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<u64> {
        self.affectingStatement(sql, bindings).await
    }

    pub async fn affectingStatement(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<u64> {
        let count = self
            .pool
            .acquire()
            .await?
            .affecting_statement(sql, bindings)
            .await?;
        if count > 0 {
            self.setRecordModificationState(true);
        }
        Ok(count)
    }

    /// Executes raw SQL without parameter binding. Use only application-controlled SQL.
    pub async fn unprepared(&self, sql: &str) -> DatabaseResult<bool> {
        let result = self.pool.acquire().await?.unprepared(sql).await?;
        self.setRecordModificationState(true);
        Ok(result)
    }
}
