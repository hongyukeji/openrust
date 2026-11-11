use sqlx::error::DatabaseError as _;

use super::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, DatabaseTransactionsManager, Row,
    Value,
    connection::{PhysicalConnection, TransactionCommand},
};

/// An exclusive database transaction on the write connection.
///
/// Dropping an active transaction rolls it back. Use this handle for all SQL
/// that must participate in the transaction.
pub struct Transaction {
    connection: Connection,
    physical: Option<PhysicalConnection>,
    level: u32,
    retain_physical: bool,
    transactions: DatabaseTransactionsManager,
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
    pub async fn beginTransaction(&self) -> DatabaseResult<Transaction> {
        let mut transaction = Transaction {
            connection: self.clone(),
            physical: Some(self.pool.acquire().await?),
            level: 0,
            retain_physical: false,
            transactions: DatabaseTransactionsManager::default(),
        };
        transaction.beginTransaction().await?;
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
        let connection = self.clone();
        // Let the owning task restore its session state even when the caller is cancelled.
        let result = tokio::spawn(async move {
            let mut physical = connection.pool.acquire().await?;
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
                retain_physical: true,
                transactions: DatabaseTransactionsManager::default(),
            };
            let result = match transaction
                .physical()?
                .control(
                    "begin schema transaction",
                    TransactionCommand::BeginImmediate,
                )
                .await
            {
                Ok(()) => {
                    transaction.level = 1;
                    transaction
                        .run(|transaction| callback(transaction, foreign_keys))
                        .await
                }
                Err(error) => Err(error),
            };
            if transaction.level == 0
                && let Some(physical) = &mut transaction.physical
            {
                physical
                    .unprepared(if legacy_alter {
                        "pragma legacy_alter_table = on"
                    } else {
                        "pragma legacy_alter_table = off"
                    })
                    .await?;
                physical
                    .unprepared(if foreign_keys {
                        "pragma foreign_keys = on"
                    } else {
                        "pragma foreign_keys = off"
                    })
                    .await?;
                physical.retire_on_drop(false);
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
        self.physical()?
            .execute_statement(sql, bindings)
            .await
            .map(|result| result.insert_id)
    }

    fn physical(&mut self) -> DatabaseResult<&mut PhysicalConnection> {
        self.physical
            .as_mut()
            .ok_or(DatabaseError::TransactionClosed)
    }

    pub fn transactionLevel(&self) -> u32 {
        self.level
    }

    pub async fn beginTransaction(&mut self) -> DatabaseResult<()> {
        self.physical()?
            .control("begin transaction", TransactionCommand::Begin)
            .await?;
        self.level += 1;
        Ok(())
    }

    /// Runs an owned callback after the outermost successful commit.
    pub fn afterCommit(
        &mut self,
        callback: impl FnOnce() -> DatabaseFuture<'static, DatabaseResult<()>> + Send + 'static,
    ) -> DatabaseResult<()> {
        self.transactions.add(self.level, Box::new(callback))
    }

    pub async fn commit(&mut self) -> DatabaseResult<()> {
        if self.level == 0 {
            return Err(DatabaseError::TransactionClosed);
        }
        self.physical()?
            .control("commit", TransactionCommand::Commit)
            .await?;
        self.level -= 1;
        if self.level == 0 && !self.retain_physical {
            self.physical.take();
        }
        self.transactions.commit(self.level + 1).await
    }

    pub async fn rollBack(&mut self, toLevel: Option<u32>) -> DatabaseResult<()> {
        let toLevel = toLevel.unwrap_or_else(|| self.level.saturating_sub(1));
        while self.level > toLevel {
            self.physical()?
                .control("roll back", TransactionCommand::RollBack)
                .await?;
            self.level -= 1;
            self.transactions.rollback(self.level);
        }
        if self.level == 0 && !self.retain_physical {
            self.physical.take();
        }
        Ok(())
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
                self.transactions.rollback(0);
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
            self.transactions.rollback(0);
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

    pub async fn select(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<Vec<Row>> {
        self.physical()?.select(sql, bindings).await
    }

    pub async fn selectOne(
        &mut self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<Option<Row>> {
        self.physical()?.select_one(sql, bindings).await
    }

    pub async fn scalar(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<Value> {
        self.selectOne(sql, bindings)
            .await?
            .map_or(Ok(Value::Null), Row::scalar)
    }

    pub async fn insert(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<bool> {
        self.statement(sql, bindings).await
    }

    pub async fn update(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<u64> {
        self.affectingStatement(sql, bindings).await
    }

    pub async fn delete(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<u64> {
        self.affectingStatement(sql, bindings).await
    }

    pub async fn statement(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<bool> {
        self.connection.setRecordModificationState(true);
        self.physical()?
            .affecting_statement(sql, bindings)
            .await
            .map(|_| true)
    }

    pub async fn affectingStatement(
        &mut self,
        sql: &str,
        bindings: &[Value],
    ) -> DatabaseResult<u64> {
        let count = self.physical()?.affecting_statement(sql, bindings).await?;
        if count > 0 {
            self.connection.setRecordModificationState(true);
        }
        Ok(count)
    }

    pub async fn unprepared(&mut self, sql: &str) -> DatabaseResult<bool> {
        let result = self.physical()?.unprepared(sql).await?;
        self.connection.setRecordModificationState(true);
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
