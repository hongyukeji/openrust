use sqlx::error::DatabaseError as _;

use super::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Row, Value,
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
        };
        transaction.beginTransaction().await?;
        Ok(transaction)
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
    pub fn table(&mut self, table: &str) -> super::query::Builder<'_> {
        let grammar = self.connection.grammar.clone();
        super::query::Builder::transaction(self, grammar, table)
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

    pub async fn commit(&mut self) -> DatabaseResult<()> {
        if self.level == 0 {
            return Err(DatabaseError::TransactionClosed);
        }
        self.physical()?
            .control("commit", TransactionCommand::Commit)
            .await?;
        self.level -= 1;
        if self.level == 0 {
            self.physical.take();
        }
        Ok(())
    }

    pub async fn rollBack(&mut self, toLevel: Option<u32>) -> DatabaseResult<()> {
        let toLevel = toLevel.unwrap_or_else(|| self.level.saturating_sub(1));
        while self.level > toLevel {
            self.physical()?
                .control("roll back", TransactionCommand::RollBack)
                .await?;
            self.level -= 1;
        }
        if self.level == 0 {
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
                Err(error) => self.abort(level, error).await,
            },
            Ok(_) => {
                self.physical.take();
                self.level = 0;
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
