use std::future::Future;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{
    Bindings, Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult,
    QueryLogEntry, RawQueryLogEntry, Row, Transaction, Value,
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
        $(pub async fn $method<'a>(sql: &str, bindings: impl Into<Bindings<'a>>) -> DatabaseResult<$result> {
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

    /// Returns the configured prefix of the current application's default connection.
    pub fn getTablePrefix() -> DatabaseResult<String> {
        Ok(Self::connection(None)?.getTablePrefix().to_owned())
    }

    pub fn disconnect(name: Option<&str>) -> DatabaseResult<()> {
        Self::getFacadeRoot()
            .map_err(DatabaseError::Unavailable)?
            .disconnect(name)
    }

    pub fn purge(name: Option<&str>) -> DatabaseResult<()> {
        Self::getFacadeRoot()
            .map_err(DatabaseError::Unavailable)?
            .purge(name)
    }

    pub async fn reconnect(name: Option<&str>) -> DatabaseResult<Connection> {
        Self::getFacadeRoot()
            .map_err(DatabaseError::Unavailable)?
            .reconnect(name)
            .await
    }

    pub fn listen(
        listener: std::sync::Arc<dyn crate::contracts::events::Listener>,
    ) -> DatabaseResult<()> {
        Self::connection(None)?.listen(listener)
    }

    pub fn whenQueryingForLongerThan(
        threshold: std::time::Duration,
        listener: std::sync::Arc<dyn crate::contracts::events::Listener>,
    ) -> DatabaseResult<()> {
        Self::connection(None)?.whenQueryingForLongerThan(threshold, listener)
    }

    pub fn enableQueryLog() -> DatabaseResult<()> {
        Self::connection(None)?.enableQueryLog();
        Ok(())
    }

    pub fn disableQueryLog() -> DatabaseResult<()> {
        Self::connection(None)?.disableQueryLog();
        Ok(())
    }

    pub fn logging() -> DatabaseResult<bool> {
        Ok(Self::connection(None)?.logging())
    }

    pub fn flushQueryLog() -> DatabaseResult<()> {
        Self::connection(None)?.flushQueryLog();
        Ok(())
    }

    pub fn getQueryLog() -> DatabaseResult<Vec<QueryLogEntry>> {
        Ok(Self::connection(None)?.getQueryLog())
    }

    pub fn getRawQueryLog() -> DatabaseResult<Vec<RawQueryLogEntry>> {
        Self::connection(None)?.getRawQueryLog()
    }

    pub async fn selectFromWriteConnection<'a>(
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Row>> {
        Self::connection(None)?
            .selectFromWriteConnection(sql, bindings)
            .await
    }

    pub async fn selectResultSets<'a>(
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
        use_read: bool,
    ) -> DatabaseResult<Vec<Vec<Row>>> {
        Self::connection(None)?
            .selectResultSets(sql, bindings, use_read)
            .await
    }

    pub async fn pretend(
        callback: impl for<'a> FnOnce(&'a Connection) -> DatabaseFuture<'a, DatabaseResult<()>> + Send,
    ) -> DatabaseResult<Vec<QueryLogEntry>> {
        Self::connection(None)?.pretend(callback).await
    }

    pub async fn withoutPretending<T>(
        callback: impl Future<Output = DatabaseResult<T>>,
    ) -> DatabaseResult<T> {
        Self::connection(None)?.withoutPretending(callback).await
    }

    pub fn pretending() -> DatabaseResult<bool> {
        Ok(Self::connection(None)?.pretending())
    }

    pub fn beforeExecuting(
        callback: impl for<'a> Fn(
            &'a str,
            Bindings<'a>,
            &'a Connection,
            Option<&'a mut Transaction>,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync
        + 'static,
    ) -> DatabaseResult<()> {
        Self::connection(None)?.beforeExecuting(callback);
        Ok(())
    }

    pub fn beforeStartingTransaction(
        callback: impl for<'a> Fn(
            &'a Connection,
            Option<&'a mut Transaction>,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync
        + 'static,
    ) -> DatabaseResult<()> {
        Self::connection(None)?.beforeStartingTransaction(callback);
        Ok(())
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
