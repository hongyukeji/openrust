use std::future::Future;

use crate::{container::ServiceKey, support::facades::Facade};

use super::{
    Bindings, Connection, DatabaseFuture, DatabaseManager, DatabaseResult, QueryLogEntry,
    RawQueryLogEntry, Row, Transaction, Value,
};

/// Executes queries on the current application's database connections.
pub struct Db;

impl Facade for Db {
    type Service = DatabaseManager;

    fn get_facade_accessor() -> ServiceKey {
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

impl Db {
    pub fn table(table: &str) -> DatabaseResult<super::query::Builder<'static>> {
        Ok(Self::connection(None)?.table(table))
    }

    pub fn raw(sql: impl Into<String>) -> super::query::Expression {
        super::query::Expression::new(sql)
    }

    pub fn connection(name: Option<&str>) -> DatabaseResult<Connection> {
        Self::get_facade_root()?.connection(name)
    }

    /// Controls destructive database Artisan commands for the current application.
    pub fn prohibit_destructive_commands(prohibit: bool) -> DatabaseResult<()> {
        Self::get_facade_root()?.set_destructive_commands_prohibited(prohibit);
        Ok(())
    }

    /// Returns the configured prefix of the current application's default connection.
    pub fn get_table_prefix() -> DatabaseResult<String> {
        Ok(Self::connection(None)?.get_table_prefix().to_owned())
    }

    pub fn disconnect(name: Option<&str>) -> DatabaseResult<()> {
        Self::get_facade_root()?.disconnect(name)
    }

    pub fn purge(name: Option<&str>) -> DatabaseResult<()> {
        Self::get_facade_root()?.purge(name)
    }

    pub async fn reconnect(name: Option<&str>) -> DatabaseResult<Connection> {
        Self::get_facade_root()?.reconnect(name).await
    }

    pub fn listen(
        listener: std::sync::Arc<dyn crate::contracts::events::Listener>,
    ) -> DatabaseResult<()> {
        Self::connection(None)?.listen(listener)
    }

    pub fn when_querying_for_longer_than(
        threshold: std::time::Duration,
        listener: std::sync::Arc<dyn crate::contracts::events::Listener>,
    ) -> DatabaseResult<()> {
        Self::connection(None)?.when_querying_for_longer_than(threshold, listener)
    }

    pub fn enable_query_log() -> DatabaseResult<()> {
        Self::connection(None)?.enable_query_log();
        Ok(())
    }

    pub fn disable_query_log() -> DatabaseResult<()> {
        Self::connection(None)?.disable_query_log();
        Ok(())
    }

    pub fn logging() -> DatabaseResult<bool> {
        Ok(Self::connection(None)?.logging())
    }

    pub fn flush_query_log() -> DatabaseResult<()> {
        Self::connection(None)?.flush_query_log();
        Ok(())
    }

    pub fn get_query_log() -> DatabaseResult<Vec<QueryLogEntry>> {
        Ok(Self::connection(None)?.get_query_log())
    }

    pub fn get_raw_query_log() -> DatabaseResult<Vec<RawQueryLogEntry>> {
        Self::connection(None)?.get_raw_query_log()
    }

    pub async fn select_from_write_connection<'a>(
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Row>> {
        Self::connection(None)?
            .select_from_write_connection(sql, bindings)
            .await
    }

    pub async fn select_result_sets<'a>(
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
        use_read: bool,
    ) -> DatabaseResult<Vec<Vec<Row>>> {
        Self::connection(None)?
            .select_result_sets(sql, bindings, use_read)
            .await
    }

    pub async fn pretend(
        callback: impl for<'a> FnOnce(&'a Connection) -> DatabaseFuture<'a, DatabaseResult<()>> + Send,
    ) -> DatabaseResult<Vec<QueryLogEntry>> {
        Self::connection(None)?.pretend(callback).await
    }

    pub async fn without_pretending<T>(
        callback: impl Future<Output = DatabaseResult<T>>,
    ) -> DatabaseResult<T> {
        Self::connection(None)?.without_pretending(callback).await
    }

    pub fn pretending() -> DatabaseResult<bool> {
        Ok(Self::connection(None)?.pretending())
    }

    pub fn before_executing(
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
        Self::connection(None)?.before_executing(callback);
        Ok(())
    }

    pub fn before_starting_transaction(
        callback: impl for<'a> Fn(
            &'a Connection,
            Option<&'a mut Transaction>,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync
        + 'static,
    ) -> DatabaseResult<()> {
        Self::connection(None)?.before_starting_transaction(callback);
        Ok(())
    }

    pub async fn begin_transaction() -> DatabaseResult<Transaction> {
        Self::connection(None)?.begin_transaction().await
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
        select_one -> Option<Row>;
        scalar -> Value;
        insert -> bool;
        update -> u64;
        delete -> u64;
        statement -> bool;
        affecting_statement -> u64;
    }

    pub async fn unprepared(sql: &str) -> DatabaseResult<bool> {
        Self::connection(None)?.unprepared(sql).await
    }
}
