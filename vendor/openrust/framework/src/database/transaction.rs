use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    contracts::{container::Container as _, events::Dispatcher as _},
    events::{Dispatcher, EventContext},
    support::facades::application_scope,
};

use futures_util::TryStreamExt as _;
use sqlx::error::DatabaseError as _;

use super::{
    Bindings, Connection, DatabaseCursor, DatabaseError, DatabaseFuture, DatabaseResult,
    DatabaseTransactionsManager, Row, Value,
    connection::{PhysicalConnection, TransactionCommand},
    events::{
        TransactionBeginning, TransactionCommitted, TransactionCommitting, TransactionEvent,
        TransactionRolledBack,
    },
};

pub(super) type BeginCallback = Arc<
    dyn for<'a> Fn(
            &'a Connection,
            Option<&'a mut Transaction>,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync,
>;

/// An exclusive database transaction on the write connection.
///
/// Dropping an active transaction rolls it back. Use this handle for all SQL
/// that must participate in the transaction.
pub struct Transaction {
    connection: Connection,
    physical: Option<PhysicalConnection>,
    level: u32,
    sqlite_schema_session: Option<SqliteSchemaSession>,
    transactions: DatabaseTransactionsManager,
}

struct SqliteSchemaSession {
    foreign_keys: bool,
    legacy_alter: bool,
}

pub(super) fn mysql_deadlock(error: &sqlx::Error) -> bool {
    error.as_database_error().is_some_and(|error| {
        error
            .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
            .is_some_and(|error| error.number() == 1213)
    })
}

fn concurrency_error(error: &DatabaseError) -> bool {
    let DatabaseError::Driver { source, .. } = error else {
        return false;
    };
    mysql_deadlock(source)
        || source.as_database_error().is_some_and(|error| {
            error.code().as_deref() == Some("40001")
                || error
                    .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                    .is_some_and(|error| error.number() == 1205)
                || error
                    .try_downcast_ref::<sqlx::sqlite::SqliteError>()
                    .and_then(|error| error.code()?.parse::<u32>().ok())
                    .is_some_and(|code| matches!(code & 0xff, 5 | 6))
        })
}

#[allow(non_snake_case, reason = "Matches Laravel's transaction API")]
impl Connection {
    /// Registers a hook before starting a transaction or savepoint.
    pub fn beforeStartingTransaction(
        &self,
        callback: impl for<'a> Fn(
            &'a Connection,
            Option<&'a mut Transaction>,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + Sync
        + 'static,
    ) -> &Self {
        self.before_transaction
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Arc::new(callback));
        self
    }

    async fn run_before_transaction(
        &self,
        mut active: Option<&mut Transaction>,
    ) -> DatabaseResult<()> {
        let callbacks = self
            .before_transaction
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if callbacks.is_empty() {
            return Ok(());
        }
        let work = async {
            for callback in callbacks {
                callback(self, active.as_deref_mut()).await?;
            }
            Ok(())
        };
        match Self::model_container(Some(self)) {
            Some(owner) => application_scope(owner, work).await,
            None => work.await,
        }
    }

    pub async fn beginTransaction(&self) -> DatabaseResult<Transaction> {
        self.run_before_transaction(None).await?;
        let mut transaction = Transaction {
            connection: self.clone(),
            physical: Some(self.acquire(false).await?),
            level: 0,
            sqlite_schema_session: None,
            transactions: DatabaseTransactionsManager::default(),
        };
        transaction
            .begin("begin transaction", TransactionCommand::Begin)
            .await?;
        Ok(transaction)
    }

    pub(super) async fn sqlite_schema_transaction(
        &self,
        callback: impl for<'a> FnOnce(
            &'a mut Transaction,
            bool,
        ) -> DatabaseFuture<'a, DatabaseResult<()>>
        + Send
        + 'static,
    ) -> DatabaseResult<()> {
        self.run_before_transaction(None).await?;
        let mut physical = self.acquire(false).await?;
        let connection = self.clone();
        // Let the owning task restore its session state even when the caller is cancelled.
        let result = tokio::spawn(async move {
            let foreign_keys = sqlite_pragma(&mut physical, "pragma foreign_keys").await?;
            let legacy_alter = sqlite_pragma(&mut physical, "pragma legacy_alter_table").await?;
            physical.retire_on_drop(true);
            physical.unprepared("pragma foreign_keys = off").await?;
            physical
                .unprepared("pragma legacy_alter_table = on")
                .await?;
            let mut transaction = Transaction {
                connection,
                physical: Some(physical),
                level: 0,
                sqlite_schema_session: Some(SqliteSchemaSession {
                    foreign_keys,
                    legacy_alter,
                }),
                transactions: DatabaseTransactionsManager::default(),
            };
            let result = match transaction
                .begin(
                    "begin schema transaction",
                    TransactionCommand::BeginImmediate,
                )
                .await
            {
                Ok(()) => {
                    transaction
                        .run(|transaction| callback(transaction, foreign_keys))
                        .await
                }
                Err(error) if transaction.level != 0 => transaction.abort(1, error).await,
                Err(error) => Err(error),
            };
            if transaction.level == 0 {
                transaction.release_session().await?;
            }
            result
        })
        .await
        .map_err(|_| DatabaseError::InvalidSchema("SQLite schema task interrupted"))?;
        if result.is_ok() {
            self.setRecordModificationState(true);
        }
        result
    }

    pub async fn transaction<T>(
        &self,
        mut callback: impl for<'a> FnMut(&'a mut Transaction) -> DatabaseFuture<'a, DatabaseResult<T>>
        + Send,
        attempts: u32,
    ) -> DatabaseResult<T>
    where
        T: Send,
    {
        if attempts == 0 {
            return Err(DatabaseError::InvalidTransactionAttempts);
        }
        let mut remaining = attempts;
        loop {
            let result = self.beginTransaction().await?.run(&mut callback).await;
            remaining -= 1;
            match result {
                Err(ref error) if remaining > 0 && concurrency_error(error) => continue,
                result => return result,
            }
        }
    }
}

macro_rules! observed_transaction_query {
    ($transaction:ident, $sql:ident, $bindings:expr, $operation:ident, $after:expr) => {{
        let bindings: Bindings<'_> = $bindings.into();
        let start = Instant::now();
        let result = $transaction.physical()?.$operation($sql, bindings).await?;
        let elapsed = start.elapsed();
        $after(&result);
        $transaction.dispatch_query($sql, bindings, elapsed).await?;
        Ok::<_, DatabaseError>(result)
    }};
}

#[allow(non_snake_case, reason = "Matches Laravel's transaction API")]
impl Transaction {
    pub fn getSchemaBuilder(&mut self) -> super::schema::Builder<'_> {
        super::schema::Builder::transaction(self)
    }

    pub fn table(&mut self, table: &str) -> super::query::Builder<'_> {
        let grammar = self.connection.grammar.clone();
        super::query::Builder::transaction(self, grammar, table)
    }

    pub(super) fn query_connection(&self) -> &Connection {
        &self.connection
    }

    pub(crate) fn belongs_to(&self, connection: &Connection) -> bool {
        self.connection.same_query_source(connection)
    }

    pub(crate) async fn insert_get_id(
        &mut self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Value> {
        self.connection.setRecordModificationState(true);
        observed_transaction_query!(self, sql, bindings, execute_statement, |_| ())
            .map(|result| result.insert_id)
    }

    fn physical(&mut self) -> DatabaseResult<&mut PhysicalConnection> {
        self.physical
            .as_mut()
            .ok_or(DatabaseError::TransactionClosed)
    }

    async fn release_session(&mut self) -> DatabaseResult<()> {
        let physical = self.physical.take();
        if let (Some(mut physical), Some(settings)) = (physical, self.sqlite_schema_session.take())
        {
            physical
                .unprepared(if settings.legacy_alter {
                    "pragma legacy_alter_table = on"
                } else {
                    "pragma legacy_alter_table = off"
                })
                .await?;
            physical
                .unprepared(if settings.foreign_keys {
                    "pragma foreign_keys = on"
                } else {
                    "pragma foreign_keys = off"
                })
                .await?;
            physical.retire_on_drop(false);
        }
        Ok(())
    }

    async fn run_rollback_callbacks(&mut self) -> DatabaseResult<()> {
        let callbacks = self.transactions.rollback(self.level);
        match Connection::model_container(Some(&self.connection)) {
            Some(owner) => application_scope(owner, callbacks).await,
            None => callbacks.await,
        }
    }

    pub fn transactionLevel(&self) -> u32 {
        self.level
    }

    pub async fn beginTransaction(&mut self) -> DatabaseResult<()> {
        self.physical()?;
        self.connection
            .clone()
            .run_before_transaction(Some(self))
            .await?;
        self.begin("begin transaction", TransactionCommand::Begin)
            .await
    }

    async fn begin(
        &mut self,
        operation: &'static str,
        command: TransactionCommand,
    ) -> DatabaseResult<()> {
        self.physical()?.control(operation, command).await?;
        self.level += 1;
        self.fire::<TransactionBeginning>().await
    }

    async fn fire<E: TransactionEvent>(&mut self) -> DatabaseResult<()> {
        let Some(owner) =
            Connection::model_container(Some(&self.connection)).filter(|owner| owner.has("events"))
        else {
            return Ok(());
        };
        let dispatcher = owner
            .get::<Dispatcher>("events")
            .map_err(DatabaseError::Unavailable)?;
        if dispatcher.observes(E::NAME) {
            let event = Arc::new(E::new(self.connection.clone(), self.level));
            let mut context = if self.level == 0 {
                EventContext::new()
            } else {
                EventContext::in_transaction(self)
            };
            application_scope(owner, dispatcher.dispatch(event, &mut context))
                .await
                .map_err(|source| DatabaseError::ConnectionEvent(Box::new(source)))?;
        }
        Ok(())
    }

    /// Runs an owned callback after the outermost successful commit.
    pub fn afterCommit(
        &mut self,
        callback: impl FnOnce() -> DatabaseFuture<'static, DatabaseResult<()>> + Send + 'static,
    ) -> DatabaseResult<()> {
        self.transactions.add(self.level, Box::new(callback))
    }

    /// Runs an owned callback if the current transaction level is rolled back.
    pub fn afterRollBack(
        &mut self,
        callback: impl FnOnce() -> DatabaseFuture<'static, DatabaseResult<()>> + Send + 'static,
    ) -> DatabaseResult<()> {
        self.transactions
            .add_rollback(self.level, Box::new(callback))
    }

    pub async fn commit(&mut self) -> DatabaseResult<()> {
        if self.level == 0 {
            return Err(DatabaseError::TransactionClosed);
        }
        if self.level == 1 {
            self.fire::<TransactionCommitting>().await?;
        }
        self.physical()?
            .control("commit", TransactionCommand::Commit)
            .await?;
        self.level -= 1;
        let release = if self.level == 0 {
            self.release_session().await
        } else {
            Ok(())
        };
        self.transactions.commit(self.level + 1).await?;
        release.map_err(|source| DatabaseError::AfterCommit {
            source: Box::new(source),
        })?;
        self.fire::<TransactionCommitted>().await.map_err(|source| {
            if self.level == 0 {
                DatabaseError::AfterCommit {
                    source: Box::new(source),
                }
            } else {
                source
            }
        })
    }

    pub async fn rollBack(&mut self, toLevel: Option<u32>) -> DatabaseResult<()> {
        let toLevel = toLevel.unwrap_or_else(|| self.level.saturating_sub(1));
        if self.level <= toLevel {
            return Ok(());
        }
        let original_level = self.level;
        let result = async {
            while self.level > toLevel {
                self.physical()?
                    .control("roll back", TransactionCommand::RollBack)
                    .await?;
                self.level -= 1;
            }
            Ok::<_, DatabaseError>(())
        }
        .await;
        let release = if self.level == 0 {
            self.release_session().await
        } else {
            Ok(())
        };
        let callbacks = if self.level != original_level {
            self.run_rollback_callbacks().await
        } else {
            Ok(())
        };
        result?;
        release.map_err(|source| DatabaseError::AfterRollback {
            source: Box::new(source),
        })?;
        callbacks?;
        self.fire::<TransactionRolledBack>().await
    }

    async fn run<T>(
        &mut self,
        callback: impl for<'a> FnOnce(&'a mut Self) -> DatabaseFuture<'a, DatabaseResult<T>>,
    ) -> DatabaseResult<T> {
        let level = self.level;
        match callback(self).await {
            Ok(result) if self.level == level => match self.commit().await {
                Ok(()) => Ok(result),
                Err(error @ DatabaseError::AfterCommit { .. }) => Err(error),
                Err(error) => self.abort(level, error).await,
            },
            Ok(_) => {
                self.physical.take();
                self.level = 0;
                self.transactions.discard();
                Err(DatabaseError::TransactionLevelChanged)
            }
            Err(error) => self.abort(level, error).await,
        }
    }

    async fn abort<T>(&mut self, level: u32, error: DatabaseError) -> DatabaseResult<T> {
        if self
            .physical
            .as_ref()
            .is_some_and(PhysicalConnection::interrupted)
        {
            self.physical.take();
            self.level = 0;
            if matches!(&error, DatabaseError::Driver { source, .. } if mysql_deadlock(source)) {
                self.run_rollback_callbacks().await?;
            } else {
                self.transactions.discard();
            }
        } else {
            self.rollBack(Some(level - 1)).await?;
        }
        Err(error)
    }

    /// Executes a nested callback within a savepoint on the same session.
    pub async fn transaction<T>(
        &mut self,
        callback: impl for<'a> FnOnce(&'a mut Self) -> DatabaseFuture<'a, DatabaseResult<T>> + Send,
    ) -> DatabaseResult<T>
    where
        T: Send,
    {
        self.beginTransaction().await?;
        self.run(callback).await
    }

    async fn dispatch_query(
        &mut self,
        sql: &str,
        bindings: Bindings<'_>,
        time: Duration,
    ) -> DatabaseResult<()> {
        if let Some(notification) = self.connection.observe_query(
            sql,
            bindings,
            time,
            self.connection.write_query_role(),
        )? {
            notification
                .dispatch(&mut EventContext::in_transaction(self))
                .await?;
        }
        Ok(())
    }

    /// Reads on this transaction's connection. Exhaust the cursor to continue using the transaction.
    pub fn cursor<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseCursor<'a> {
        let bindings = bindings.into();
        Box::pin(async_stream::try_stream! {
            let mut elapsed = Duration::ZERO;
            {
                let physical = self.physical()?;
                let prepared = bindings.prepare(sql, matches!(physical, PhysicalConnection::Mysql(_)))?;
                let mut rows = std::pin::pin!(physical.cursor(&prepared));
                loop {
                    let start = Instant::now();
                    let next = rows.try_next().await?;
                    elapsed += start.elapsed();
                    let Some(row) = next else { break };
                    yield row;
                }
            }
            self.dispatch_query(sql, bindings, elapsed).await?;
        })
    }

    pub async fn select<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Row>> {
        observed_transaction_query!(self, sql, bindings, select, |_| ())
    }

    /// Returns all rowsets on the transaction's pinned connection.
    pub async fn selectResultSets<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Vec<Row>>> {
        observed_transaction_query!(self, sql, bindings, select_result_sets, |_| ())
    }

    pub async fn selectOne<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Option<Row>> {
        observed_transaction_query!(self, sql, bindings, select_one, |_| ())
    }

    pub async fn scalar<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Value> {
        self.selectOne(sql, bindings)
            .await?
            .map_or(Ok(Value::Null), Row::scalar)
    }

    pub async fn insert<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<bool> {
        self.statement(sql, bindings).await
    }

    pub async fn update<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<u64> {
        self.affectingStatement(sql, bindings).await
    }

    pub async fn delete<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<u64> {
        self.affectingStatement(sql, bindings).await
    }

    pub async fn statement<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<bool> {
        self.connection.setRecordModificationState(true);
        observed_transaction_query!(self, sql, bindings, affecting_statement, |_| ()).map(|_| true)
    }

    pub async fn affectingStatement<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<u64> {
        observed_transaction_query!(self, sql, bindings, affecting_statement, |count: &u64| {
            if *count > 0 {
                self.connection.setRecordModificationState(true);
            }
        })
    }

    pub async fn unprepared(&mut self, sql: &str) -> DatabaseResult<bool> {
        let start = Instant::now();
        let result = self.physical()?.unprepared(sql).await?;
        let elapsed = start.elapsed();
        self.connection.setRecordModificationState(true);
        self.dispatch_query(sql, (&[]).into(), elapsed).await?;
        Ok(result)
    }
}

async fn sqlite_pragma(physical: &mut PhysicalConnection, sql: &str) -> DatabaseResult<bool> {
    let row = physical
        .select_one(sql, &[])
        .await?
        .ok_or(DatabaseError::InvalidSchema("SQLite session setting"))?;
    Ok(matches!(
        row.get_at(0),
        Some(Value::Integer(1) | Value::Bool(true))
    ))
}
