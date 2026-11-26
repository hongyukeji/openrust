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
    test_events,
    test_transaction::QueryPhysical,
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
    physical: Option<QueryPhysical>,
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
    if let DatabaseError::SqlServer { source, .. } = error {
        return source.is_deadlock();
    }
    let DatabaseError::Driver { source, .. } = error else {
        return false;
    };
    mysql_deadlock(source)
        || source.as_database_error().is_some_and(|error| {
            matches!(error.code().as_deref(), Some("40001" | "40P01"))
                || error
                    .try_downcast_ref::<sqlx::mysql::MySqlDatabaseError>()
                    .is_some_and(|error| error.number() == 1205)
                || error
                    .try_downcast_ref::<sqlx::sqlite::SqliteError>()
                    .and_then(|error| error.code()?.parse::<u32>().ok())
                    .is_some_and(|code| matches!(code & 0xff, 5 | 6))
        })
}

/// 为业务服务复用指定连接上的事务执行。实现者负责选择连接及业务规则。
pub trait TransactionalService: Sync {
    fn connection(&self) -> DatabaseResult<Connection>;

    #[track_caller]
    fn transaction<T: Send>(
        &self,
        callback: impl for<'a> FnMut(&'a mut Transaction) -> DatabaseFuture<'a, DatabaseResult<T>>
        + Send,
        attempts: u32,
    ) -> impl std::future::Future<Output = DatabaseResult<T>> + Send {
        super::events::with_query_caller(std::panic::Location::caller(), async move {
            self.connection()?.transaction(callback, attempts).await
        })
    }
}

impl Connection {
    /// Registers a hook before starting a transaction or savepoint.
    pub fn before_starting_transaction(
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

    pub async fn begin_transaction(&self) -> DatabaseResult<Transaction> {
        self.ensure_real_operation()?;
        self.run_before_transaction(None).await?;
        let mut replayed = false;
        loop {
            let mut lease = self.acquire_query(false).await?;
            lease.physical.begin_nested();
            let mut transaction = Transaction {
                connection: self.clone(),
                physical: Some(lease.physical),
                level: 0,
                sqlite_schema_session: None,
                transactions: DatabaseTransactionsManager::default(),
            };
            match transaction
                .begin("begin transaction", TransactionCommand::Begin)
                .await
            {
                Ok(()) => return Ok(transaction),
                Err(error)
                    if transaction.level == 0
                        && self.retry_lost_connection(
                            lease.generation.as_ref(),
                            &error,
                            &mut replayed,
                        ) => {}
                Err(error) => return Err(error),
            }
        }
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
        let result = tokio::spawn(super::events::with_inherited_query_scope(async move {
            let foreign_keys = sqlite_pragma(&mut physical, "pragma foreign_keys").await?;
            let legacy_alter = sqlite_pragma(&mut physical, "pragma legacy_alter_table").await?;
            physical.retire_on_drop(true);
            physical.unprepared("pragma foreign_keys = off").await?;
            physical
                .unprepared("pragma legacy_alter_table = on")
                .await?;
            let mut transaction = Transaction {
                connection,
                physical: Some(QueryPhysical::Pooled(physical)),
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
        }))
        .await
        .map_err(|_| DatabaseError::InvalidSchema("SQLite schema task interrupted"))?;
        if result.is_ok() {
            self.set_record_modification_state(true);
        }
        result
    }

    #[track_caller]
    pub fn transaction<T>(
        &self,
        mut callback: impl for<'a> FnMut(&'a mut Transaction) -> DatabaseFuture<'a, DatabaseResult<T>>
        + Send,
        attempts: u32,
    ) -> impl std::future::Future<Output = DatabaseResult<T>>
    where
        T: Send,
    {
        super::events::with_query_caller(std::panic::Location::caller(), async move {
            if attempts == 0 {
                return Err(DatabaseError::InvalidTransactionAttempts);
            }
            let mut remaining = attempts;
            loop {
                let result = self.begin_transaction().await?.run(&mut callback).await;
                remaining -= 1;
                match result {
                    Err(ref error) if remaining > 0 && concurrency_error(error) => continue,
                    result => return result,
                }
            }
        })
    }
}

macro_rules! observed_transaction_query {
    ($transaction:ident, $sql:ident, $bindings:expr, $operation:ident, $preview:expr, $before:expr, $after:expr) => {{
        let bindings: Bindings<'_> = $bindings.into();
        $transaction.run_before_executing($sql, bindings).await?;
        if $transaction.connection.pretending() {
            $transaction
                .dispatch_query($sql, bindings, Duration::ZERO)
                .await?;
            return Ok($preview);
        }
        use crate::foundation::observability::{self, Outcome};
        observability::begin_sql(
            Connection::model_container(Some(&$transaction.connection)),
            $transaction.connection.get_driver_name(),
            $sql,
        )
        .run_boxed(
            async {
                let start = Instant::now();
                $transaction.physical()?.validate($sql, true)?;
                $before();
                let result = $transaction.physical()?.$operation($sql, bindings).await?;
                let elapsed = start.elapsed();
                $after(&result);
                $transaction.dispatch_query($sql, bindings, elapsed).await?;
                Ok::<_, DatabaseError>(result)
            },
            |result| Outcome::Sql {
                failed: result.is_err(),
            },
        )
        .await
    }};
}

impl Transaction {
    pub fn get_schema_builder(&mut self) -> super::schema::Builder<'_> {
        super::schema::Builder::transaction(self)
    }

    pub fn table(&mut self, table: &str) -> super::query::Builder<'_> {
        let grammar = self.connection.grammar.clone();
        super::query::Builder::transaction(self, grammar, table)
    }

    /// Returns the connection whose transaction this handle borrows.
    pub fn get_connection(&self) -> &Connection {
        &self.connection
    }

    pub(super) fn query_connection(&self) -> &Connection {
        &self.connection
    }

    /// 核对事务的实际查询来源，包含相同名称下不同作用域的连接。
    pub fn belongs_to(&self, connection: &Connection) -> bool {
        self.connection.same_query_source(connection)
    }

    pub(crate) async fn insert_get_id(
        &mut self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Value> {
        if self.connection.grammar.is_postgres() || self.connection.grammar.is_sql_server() {
            return observed_transaction_query!(
                self,
                sql,
                bindings,
                select_one,
                Value::Null,
                || self.connection.set_record_modification_state(true),
                |_| ()
            )
            .and_then(|row| row.map_or(Ok(Value::Null), Row::scalar));
        }
        observed_transaction_query!(
            self,
            sql,
            bindings,
            execute_statement,
            Value::Null,
            || self.connection.set_record_modification_state(true),
            |_| ()
        )
        .map(|result| result.insert_id)
    }

    fn physical(&mut self) -> DatabaseResult<&mut QueryPhysical> {
        self.physical
            .as_mut()
            .ok_or(DatabaseError::TransactionClosed)
    }

    async fn release_session(&mut self) -> DatabaseResult<()> {
        let physical = self.physical.take().map(|mut physical| {
            physical.end_nested();
            physical
        });
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

    pub fn transaction_level(&self) -> u32 {
        self.level
    }

    pub async fn begin_transaction(&mut self) -> DatabaseResult<()> {
        self.connection.ensure_real_operation()?;
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
        if test_events::muted(&self.connection) {
            return Ok(());
        }
        let Some(owner) =
            Connection::model_container(Some(&self.connection)).filter(|owner| owner.has("events"))
        else {
            return Ok(());
        };
        let dispatcher = owner.get::<Dispatcher>("events")?;
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
    pub fn after_commit(
        &mut self,
        callback: impl FnOnce() -> DatabaseFuture<'static, DatabaseResult<()>> + Send + 'static,
    ) -> DatabaseResult<()> {
        self.transactions.add(self.level, Box::new(callback))
    }

    /// Registers a callback on an active ancestor transaction level.
    ///
    /// A callback registered on level 1 retains registration order with other
    /// level-1 callbacks. Pair it with a current-level commit marker when work
    /// must be discarded on savepoint rollback but saved in its original order.
    pub fn after_commit_at_level(
        &mut self,
        level: u32,
        callback: impl FnOnce() -> DatabaseFuture<'static, DatabaseResult<()>> + Send + 'static,
    ) -> DatabaseResult<()> {
        if level == 0 || level > self.level {
            return Err(DatabaseError::TransactionClosed);
        }
        self.transactions.add_at_level(level, Box::new(callback));
        Ok(())
    }

    /// Runs an owned callback if the current transaction level is rolled back.
    pub fn after_roll_back(
        &mut self,
        callback: impl FnOnce() -> DatabaseFuture<'static, DatabaseResult<()>> + Send + 'static,
    ) -> DatabaseResult<()> {
        self.transactions
            .add_rollback(self.level, Box::new(callback))
    }

    pub async fn commit(&mut self) -> DatabaseResult<()> {
        self.connection.ensure_real_operation()?;
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

    pub async fn roll_back(&mut self, to_level: Option<u32>) -> DatabaseResult<()> {
        self.connection.ensure_real_operation()?;
        let to_level = to_level.unwrap_or_else(|| self.level.saturating_sub(1));
        if self.level <= to_level {
            return Ok(());
        }
        let original_level = self.level;
        let sql_server = self.connection.grammar.is_sql_server();
        let result = async {
            while self.level > to_level {
                let physical = self.physical()?;
                physical
                    .control("roll back", TransactionCommand::RollBack)
                    .await?;
                if sql_server && !physical.in_transaction() {
                    self.level = 0;
                    // A scoped test session cannot be used after its outer transaction is gone.
                    self.physical.take();
                    break;
                }
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
        let result = callback(self).await;
        self.finish(level, result).await
    }

    pub(super) async fn finish<T>(
        &mut self,
        level: u32,
        result: DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        match result {
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
            .is_some_and(|physical| physical.interrupted())
        {
            self.physical.take();
            self.level = 0;
            if matches!(&error, DatabaseError::Driver { source, .. } if mysql_deadlock(source)) {
                self.run_rollback_callbacks().await?;
            } else {
                self.transactions.discard();
            }
        } else {
            self.roll_back(Some(level - 1)).await?;
        }
        Err(error)
    }

    /// Executes a nested callback within a savepoint on the same session.
    #[track_caller]
    pub fn transaction<T>(
        &mut self,
        callback: impl for<'a> FnOnce(&'a mut Self) -> DatabaseFuture<'a, DatabaseResult<T>> + Send,
    ) -> impl std::future::Future<Output = DatabaseResult<T>>
    where
        T: Send,
    {
        super::events::with_query_caller(std::panic::Location::caller(), async move {
            self.begin_transaction().await?;
            self.run(callback).await
        })
    }

    async fn run_before_executing(
        &mut self,
        sql: &str,
        bindings: Bindings<'_>,
    ) -> DatabaseResult<()> {
        self.physical()?;
        self.connection
            .clone()
            .run_before_executing(sql, bindings, Some(self))
            .await
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
    #[track_caller]
    pub fn cursor<'a>(
        &'a mut self,
        sql: &'a str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseCursor<'a> {
        let bindings = bindings.into();
        use crate::foundation::observability::{self, Execution};
        let connection = self.connection.clone();
        let start = move || {
            if connection.pretending() {
                return Execution::default();
            }
            observability::begin_sql(
                Connection::model_container(Some(&connection)),
                connection.get_driver_name(),
                sql,
            )
        };
        Box::pin(super::events::stream_with_query_caller(
            std::panic::Location::caller(),
            observability::sql_stream(
                start,
                async_stream::try_stream! {
                    self.run_before_executing(sql, bindings).await?;
                    if self.connection.pretending() {
                        self.dispatch_query(sql, bindings, Duration::ZERO).await?;
                        return;
                    }
                    let mut elapsed = Duration::ZERO;
                    {
                        let physical = self.physical()?;
                        physical.validate(sql, true)?;
                        let prepared = physical.prepare(sql, bindings)?;
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
                },
            ),
        ))
    }

    #[track_caller]
    pub fn select<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<Vec<Row>>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            observed_transaction_query!(self, sql, bindings, select, Vec::new(), || (), |_| ())
        })
    }

    pub(super) async fn insert_returning(
        &mut self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Vec<Row>> {
        observed_transaction_query!(
            self,
            sql,
            bindings,
            select,
            Vec::new(),
            || (),
            |rows: &Vec<Row>| {
                if !rows.is_empty() {
                    self.connection.set_record_modification_state(true);
                }
            }
        )
    }

    /// Returns all rowsets on the transaction's pinned connection.
    #[track_caller]
    pub fn select_result_sets<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<Vec<Vec<Row>>>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            observed_transaction_query!(
                self,
                sql,
                bindings,
                select_result_sets,
                Vec::new(),
                || (),
                |_| ()
            )
        })
    }

    #[track_caller]
    pub fn select_one<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<Option<Row>>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            observed_transaction_query!(self, sql, bindings, select_one, None, || (), |_| ())
        })
    }

    #[track_caller]
    pub fn scalar<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<Value>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.select_one(sql, bindings)
                .await?
                .map_or(Ok(Value::Null), Row::scalar)
        })
    }

    #[track_caller]
    pub fn insert<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<bool>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.statement(sql, bindings).await
        })
    }

    #[track_caller]
    pub fn update<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.affecting_statement(sql, bindings).await
        })
    }

    #[track_caller]
    pub fn delete<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            self.affecting_statement(sql, bindings).await
        })
    }

    #[track_caller]
    pub fn statement<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<bool>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            observed_transaction_query!(
                self,
                sql,
                bindings,
                affecting_statement,
                true,
                || self.connection.set_record_modification_state(true),
                |_| ()
            )
            .map(|_| true)
        })
    }

    #[track_caller]
    pub fn affecting_statement<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> impl Future<Output = DatabaseResult<u64>> {
        crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
            observed_transaction_query!(
                self,
                sql,
                bindings,
                affecting_statement,
                0,
                || (),
                |count: &u64| {
                    if *count > 0 {
                        self.connection.set_record_modification_state(true);
                    }
                }
            )
        })
    }

    pub async fn unprepared(&mut self, sql: &str) -> DatabaseResult<bool> {
        self.run_before_executing(sql, (&[]).into()).await?;
        if self.connection.pretending() {
            self.dispatch_query(sql, (&[]).into(), Duration::ZERO)
                .await?;
            return Ok(true);
        }
        use crate::foundation::observability::{self, Outcome};
        observability::begin_sql(
            Connection::model_container(Some(&self.connection)),
            self.connection.get_driver_name(),
            sql,
        )
        .run(
            async {
                let start = Instant::now();
                self.physical()?.validate(sql, false)?;
                let result = self.physical()?.unprepared(sql).await?;
                let elapsed = start.elapsed();
                self.connection.set_record_modification_state(true);
                self.dispatch_query(sql, (&[]).into(), elapsed).await?;
                Ok(result)
            },
            |result: &DatabaseResult<bool>| Outcome::Sql {
                failed: result.is_err(),
            },
        )
        .await
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
