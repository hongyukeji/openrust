use crate::{container::ServiceKey, support::facades::Facade};

use super::{
    Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Row, Transaction,
    Value,
};

/// Executes queries on the current application's database connections.
pub struct DB;

impl Facade for DB {
    type Service = DatabaseManager;

    fn getFacadeAccessor() -> ServiceKey {
        "db".into()
    }
}

macro_rules! statements {
    ($($method:ident -> $result:ty;)+) => {
        $(pub async fn $method(sql: &str, bindings: &[Value]) -> DatabaseResult<$result> {
            Self::connection(None)?.$method(sql, bindings).await
        })+
    };
}

#[allow(non_snake_case, reason = "Matches Laravel's DB Facade API")]
impl DB {
    pub fn table(table: &str) -> DatabaseResult<super::query::Builder<'static>> {
        Ok(Self::connection(None)?.table(table))
    }

    pub fn raw(sql: impl Into<String>) -> super::query::Expression {
        super::query::Expression::new(sql)
    }

    pub fn connection(name: Option<&str>) -> DatabaseResult<Connection> {
        Self::getFacadeRoot()
            .map_err(DatabaseError::Unavailable)?
            .connection(name)
    }

    pub async fn selectFromWriteConnection(
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Vec<Row>> {
        Self::connection(None)?
            .selectFromWriteConnection(sql, bindings)
            .await
    }

    pub async fn beginTransaction() -> DatabaseResult<Transaction> {
        Self::connection(None)?.beginTransaction().await
    }

    pub async fn transaction<T>(
        callback: impl for<'a> FnMut(&'a mut Transaction) -> DatabaseFuture<'a, DatabaseResult<T>>
        + Send,
        attempts: u32,
    ) -> DatabaseResult<T>
    where
        T: Send,
    {
        Self::connection(None)?
            .transaction(callback, attempts)
            .await
    }

    statements! {
        select -> Vec<Row>;
        selectOne -> Option<Row>;
        scalar -> Value;
        insert -> bool;
        update -> u64;
        delete -> u64;
        statement -> bool;
        affectingStatement -> u64;
    }

    pub async fn unprepared(sql: &str) -> DatabaseResult<bool> {
        Self::connection(None)?.unprepared(sql).await
    }
}
