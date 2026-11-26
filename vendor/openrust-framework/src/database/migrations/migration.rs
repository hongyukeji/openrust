use std::sync::Arc;

use crate::contracts::config::Repository;
use crate::database::{Connection, DatabaseFuture, DatabaseResult, Transaction, Value, schema};

/// A compiled migration registered with the application.
pub trait Migration: Send + Sync {
    /// Resolves a potentially configuration-derived name from the owning application.
    fn get_connection(&self, _config: &dyn Repository) -> DatabaseResult<Option<String>> {
        Ok(None)
    }

    /// Allows the migrator to run the whole migration in a schema transaction.
    fn within_transaction(&self) -> bool {
        true
    }

    fn should_run<'a>(
        &'a self,
        _connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async { Ok(true) })
    }

    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}

/// Executes migration operations on the selected connection, or records SQL in pretend mode.
pub struct MigrationContext {
    connection: Connection,
    config: Arc<dyn Repository>,
    transaction: Option<Transaction>,
    statements: Option<Vec<String>>,
}

impl MigrationContext {
    pub(super) fn new(connection: Connection, config: Arc<dyn Repository>, pretend: bool) -> Self {
        Self {
            connection,
            config,
            transaction: None,
            statements: pretend.then(Vec::new),
        }
    }

    /// 返回当前迁移所属应用的配置。
    pub fn configuration(&self) -> &dyn Repository {
        self.config.as_ref()
    }

    pub fn get_schema_builder(&mut self) -> schema::Builder<'_> {
        match &mut self.statements {
            Some(statements) => schema::Builder::pretend(&self.connection, statements),
            None => match &mut self.transaction {
                Some(transaction) => transaction.get_schema_builder(),
                None => self.connection.get_schema_builder(),
            },
        }
    }

    /// 真实迁移从当前事务或写连接读取，预演时跳过数据查询。
    pub fn query_table(&mut self, table: &str) -> Option<super::super::query::Builder<'_>> {
        if self.statements.is_some() {
            return None;
        }
        Some(match &mut self.transaction {
            Some(transaction) => transaction.table(table),
            None => self.connection.table(table).use_write_pdo(),
        })
    }

    pub async fn statement(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<bool> {
        if let Some(statements) = &mut self.statements {
            statements.push(sql.to_owned());
            return Ok(true);
        }
        match &mut self.transaction {
            Some(transaction) => transaction.statement(sql, bindings).await,
            None => self.connection.statement(sql, bindings).await,
        }
    }

    /// Returns the driver used by this migration, including pretend mode.
    pub fn get_driver_name(&self) -> &'static str {
        self.connection.get_driver_name()
    }

    pub fn get_name(&self) -> &str {
        self.connection.get_name()
    }

    pub(super) fn take_statements(self) -> Vec<String> {
        self.statements.unwrap_or_default()
    }

    pub(super) async fn begin_transaction(&mut self, enabled: bool) -> DatabaseResult<()> {
        if enabled && (self.connection.is_postgres() || self.connection.is_sql_server()) {
            self.transaction = Some(self.connection.begin_transaction().await?);
        }
        Ok(())
    }

    pub(super) fn transaction(&mut self) -> Option<&mut Transaction> {
        self.transaction.as_mut()
    }

    pub(super) async fn finish(&mut self, result: DatabaseResult<()>) -> DatabaseResult<()> {
        match self.transaction.take() {
            Some(mut transaction) => transaction.finish(1, result).await,
            None => result,
        }
    }
}

/// A migration and its source path in the compiled application.
pub struct MigrationEntry {
    pub path: &'static str,
    pub migration: &'static dyn Migration,
}

impl MigrationEntry {
    pub const fn new(path: &'static str, migration: &'static dyn Migration) -> Self {
        Self { path, migration }
    }
}

/// The statically registered migration files for an application.
#[derive(Default)]
pub struct MigrationRegistry(pub &'static [MigrationEntry]);
