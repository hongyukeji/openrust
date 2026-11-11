use sqlx::SqlSafeStr as _;
use sqlx::{
    Database, Executor as _, MySql, MySqlPool, Sqlite, SqlitePool, Statement as _,
    pool::PoolConnection,
};
use std::sync::Arc;

use super::{DatabaseError, DatabaseResult, Row, Value, value};

#[derive(Clone)]
pub(super) enum Pool {
    Sqlite(SqlitePool),
    Mysql(MySqlPool),
}

/// A named database connection sharing its application's pool.
#[derive(Clone)]
pub struct Connection {
    pub(super) name: Arc<str>,
    pub(super) database: Arc<str>,
    pub(super) pool: Pool,
}

enum PhysicalConnection {
    Sqlite(Lease<Sqlite>),
    Mysql(Lease<MySql>),
}

struct Lease<D: Database> {
    connection: PoolConnection<D>,
    dirty: bool,
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
        })
    }

    fn finish<T>(&mut self, result: Result<T, sqlx::Error>) -> Result<T, sqlx::Error> {
        self.dirty = result
            .as_ref()
            .err()
            .is_some_and(|error| !matches!(error, sqlx::Error::Database(_)));
        result
    }
}

impl<D: Database> Drop for Lease<D> {
    fn drop(&mut self) {
        if D::NAME == "MySQL" && self.dirty {
            // A cancelled query can leave an incomplete protocol exchange.
            self.connection.close_on_drop();
        }
    }
}

impl Pool {
    async fn acquire(&self) -> DatabaseResult<PhysicalConnection> {
        match self {
            Self::Sqlite(pool) => Ok(PhysicalConnection::Sqlite(Lease::acquire(pool).await?)),
            Self::Mysql(pool) => Ok(PhysicalConnection::Mysql(Lease::acquire(pool).await?)),
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
        $lease.dirty = true;
        let prepared = $lease
            .connection
            .prepare(sqlx::AssertSqlSafe($sql).into_sql_str())
            .await;
        let statement = $lease
            .finish(prepared)
            .map_err(DatabaseError::driver("prepare"))?;
        validate_bindings(&statement, $bindings.len())?;
        let query = bind!(statement.query(), $bindings, $unsigned);
        $lease.dirty = true;
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

impl PhysicalConnection {
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
        query!(
            self,
            sql,
            bindings,
            execute,
            "execute",
            |result, affected: fn(_) -> u64| Ok(affected(result)),
            |result: sqlx::sqlite::SqliteQueryResult| result.rows_affected(),
            |result: sqlx::mysql::MySqlQueryResult| result.rows_affected()
        )
    }

    pub(super) async fn unprepared(&mut self, sql: &str) -> DatabaseResult<bool> {
        async fn execute<D: Database>(lease: &mut Lease<D>, sql: &str) -> DatabaseResult<bool>
        where
            for<'a> &'a mut D::Connection: sqlx::Executor<'a, Database = D>,
        {
            lease.dirty = true;
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
    pub fn getName(&self) -> &str {
        &self.name
    }

    pub fn getDatabaseName(&self) -> &str {
        &self.database
    }

    pub async fn select(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<Vec<Row>> {
        self.pool.acquire().await?.select(sql, bindings).await
    }

    pub async fn selectOne(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<Option<Row>> {
        self.pool.acquire().await?.select_one(sql, bindings).await
    }

    pub async fn scalar(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<Value> {
        self.selectOne(sql, bindings)
            .await?
            .map_or(Ok(Value::Null), Row::scalar)
    }

    pub async fn statement(&self, sql: &str, bindings: &[Value]) -> DatabaseResult<bool> {
        let mut connection = self.pool.acquire().await?;
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
        self.pool
            .acquire()
            .await?
            .affecting_statement(sql, bindings)
            .await
    }

    /// Executes raw SQL without parameter binding. Use only application-controlled SQL.
    pub async fn unprepared(&self, sql: &str) -> DatabaseResult<bool> {
        self.pool.acquire().await?.unprepared(sql).await
    }
}
