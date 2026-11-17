use std::sync::{Arc, RwLock};

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Row, Transaction, Value,
};

use super::{
    Blueprint, KeySelector, SchemaDefaults, SchemaSession,
    grammars::{Grammar, Table},
};

mod catalog;
mod mariadb;
mod sqlite;

enum Executor<'connection> {
    Connection(Connection),
    Transaction(&'connection mut Transaction),
    Session(&'connection mut SchemaSession),
    Pretend(&'connection mut Vec<String>),
}

type BlueprintResolver = Box<dyn Fn(&mut Blueprint) + Send + Sync>;

/// Inspects and changes the schema on the write connection.
pub struct Builder<'connection> {
    executor: Executor<'connection>,
    grammar: Grammar,
    schema_defaults: Arc<RwLock<SchemaDefaults>>,
    blueprint_resolver: Option<BlueprintResolver>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Schema Builder API")]
impl<'connection> Builder<'connection> {
    pub(crate) fn connection(connection: Connection) -> Builder<'static> {
        let grammar = Grammar::new(&connection);
        Builder {
            schema_defaults: connection.schema_defaults.clone(),
            executor: Executor::Connection(connection),
            grammar,
            blueprint_resolver: None,
        }
    }

    pub(crate) fn pretend(
        connection: &Connection,
        statements: &'connection mut Vec<String>,
    ) -> Self {
        let grammar = Grammar::new(connection);
        Self {
            executor: Executor::Pretend(statements),
            grammar,
            blueprint_resolver: None,
            schema_defaults: connection.schema_defaults.clone(),
        }
    }

    pub(crate) fn transaction(transaction: &'connection mut Transaction) -> Self {
        let connection = transaction.query_connection();
        let grammar = Grammar::new(connection);
        let schema_defaults = connection.schema_defaults.clone();
        Self {
            executor: Executor::Transaction(transaction),
            grammar,
            blueprint_resolver: None,
            schema_defaults,
        }
    }

    pub(super) fn session_builder(session: &'connection mut SchemaSession) -> Self {
        let connection = session.connection();
        let grammar = Grammar::new(connection);
        let schema_defaults = connection.schema_defaults.clone();
        Self {
            executor: Executor::Session(session),
            grammar,
            blueprint_resolver: None,
            schema_defaults,
        }
    }

    pub async fn session(&self) -> DatabaseResult<SchemaSession> {
        let Executor::Connection(connection) = &self.executor else {
            return Err(DatabaseError::UnsupportedSchema(
                "schema session requires a connection builder",
            ));
        };
        SchemaSession::new(connection.clone()).await
    }

    pub async fn withoutForeignKeyConstraints<T>(
        &self,
        callback: impl for<'a> FnOnce(&'a mut SchemaSession) -> DatabaseFuture<'a, DatabaseResult<T>>
        + Send,
    ) -> DatabaseResult<T> {
        let mut session = self.session().await?;
        session.disableForeignKeyConstraints().await?;
        let result = callback(&mut session).await;
        let cleanup = session.close().await;
        let output = result?;
        cleanup?;
        Ok(output)
    }

    /// Sets the initializer for blueprints produced by this schema builder.
    pub fn blueprintResolver(&mut self, resolver: impl Fn(&mut Blueprint) + Send + Sync + 'static) {
        self.blueprint_resolver = Some(Box::new(resolver));
    }

    fn blueprint(&self, table: &str, creating: bool) -> Blueprint {
        let mut blueprint = Blueprint {
            table: table.to_owned(),
            prefix: Arc::clone(self.grammar.table_prefix()),
            creating,
            defaults: *self
                .schema_defaults
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            ..Blueprint::default()
        };
        if let Some(resolve) = &self.blueprint_resolver {
            resolve(&mut blueprint);
        }
        blueprint
    }

    /// Creates a database on the selected connection's server or a new SQLite file.
    pub async fn createDatabase(&mut self, name: &str) -> DatabaseResult<bool> {
        if self.grammar.mysql() {
            self.execute(vec![self.grammar.create_database(name)?])
                .await?;
        } else {
            self.validate_sqlite_database_path(name)?;
            tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(name)
                .await
                .map_err(DatabaseError::SchemaIo)?;
        }
        Ok(true)
    }

    /// Drops a database on the selected connection's server or removes a SQLite file.
    pub async fn dropDatabaseIfExists(&mut self, name: &str) -> DatabaseResult<bool> {
        if self.grammar.mysql() {
            let name = self.grammar.identifier(name)?;
            self.execute(vec![format!("drop database if exists {name}")])
                .await?;
        } else {
            self.validate_sqlite_database_path(name)?;
            if let Err(error) = tokio::fs::remove_file(name).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                return Err(DatabaseError::SchemaIo(error));
            }
        }
        Ok(true)
    }

    fn validate_sqlite_database_path(&self, name: &str) -> DatabaseResult<()> {
        if !matches!(self.executor, Executor::Connection(_)) {
            return Err(DatabaseError::UnsupportedSchema(
                "SQLite database files require a connection builder",
            ));
        }
        if name.is_empty() || name.contains('\0') || name == ":memory:" || name.starts_with("file:")
        {
            return Err(DatabaseError::InvalidSchema("SQLite database file path"));
        }
        Ok(())
    }

    pub async fn create(
        &mut self,
        table: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        let mut resolved_table = self.grammar.table(table)?;
        let mut blueprint = self.blueprint(table, true);
        configure(&mut blueprint);
        self.mariadb_version_for(&blueprint).await?;
        if blueprint.temporary {
            if !matches!(self.executor, Executor::Session(_) | Executor::Pretend(_)) {
                return Err(DatabaseError::UnsupportedSchema(
                    "temporary table requires a schema session",
                ));
            }
            self.temporary_schema(&mut resolved_table)?;
            let sql = self.grammar.create(&resolved_table, blueprint)?;
            let cleanup = self.temporary_drop(&resolved_table, true)?;
            if let Executor::Session(session) = &mut self.executor {
                return session.create_temporary(resolved_table, cleanup, sql).await;
            }
            return self.execute(sql).await;
        }
        let sql = self.grammar.create(&resolved_table, blueprint)?;
        self.execute(sql).await
    }

    fn resolve_table(&self, name: &str) -> DatabaseResult<Table> {
        let table = self.grammar.table(name)?;
        Ok(match &self.executor {
            Executor::Session(session) => session.temporary_table(&table).cloned().unwrap_or(table),
            _ => table,
        })
    }

    fn temporary_schema(&self, table: &mut Table) -> DatabaseResult<()> {
        if !self.grammar.mysql() {
            if table
                .schema
                .as_deref()
                .is_some_and(|schema| !schema.eq_ignore_ascii_case("temp"))
            {
                return Err(DatabaseError::UnsupportedSchema(
                    "SQLite temporary table schema",
                ));
            }
            table.schema = Some("temp".into());
        }
        Ok(())
    }

    fn temporary_drop(&self, table: &Table, if_exists: bool) -> DatabaseResult<String> {
        let table = self.grammar.qualified(table)?;
        let temporary = if self.grammar.mysql() {
            " temporary"
        } else {
            ""
        };
        let guard = if if_exists { " if exists" } else { "" };
        Ok(format!("drop{temporary} table{guard} {table}"))
    }

    pub async fn table(
        &mut self,
        table: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        let resolved_table = self.resolve_table(table)?;
        let mut blueprint = self.blueprint(table, false);
        let table = resolved_table;
        configure(&mut blueprint);
        let version = self.mariadb_version_for(&blueprint).await?;
        if version.is_some_and(|version| version < (10, 5, 2))
            && mariadb::has_rename(&blueprint)
            && !blueprint.temporary
        {
            return self.mariadb_legacy_alter(&table, blueprint).await;
        }
        if !self.grammar.mysql() && blueprint.commands.iter().any(sqlite::requires_catalog) {
            if matches!(self.executor, Executor::Pretend(_)) {
                return Err(DatabaseError::UnsupportedSchema(
                    "SQLite rebuild needs live catalog metadata",
                ));
            }
            let Executor::Connection(connection) = &self.executor else {
                return Err(DatabaseError::UnsupportedSchema(
                    "SQLite rebuild inside an active transaction",
                ));
            };
            let grammar = self.grammar.clone();
            return connection
                .sqlite_schema_transaction(move |transaction, foreign_keys| {
                    Box::pin(sqlite::alter(
                        transaction,
                        grammar,
                        table,
                        blueprint,
                        foreign_keys,
                    ))
                })
                .await;
        }
        let sql = self.grammar.alter(&table, blueprint)?;
        self.execute(sql).await
    }

    pub async fn whenTableHasColumn(
        &mut self,
        table: &str,
        column: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        if self.hasColumn(table, column).await? {
            self.table(table, configure).await?;
        }
        Ok(())
    }

    pub async fn whenTableDoesntHaveColumn(
        &mut self,
        table: &str,
        column: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        if !self.hasColumn(table, column).await? {
            self.table(table, configure).await?;
        }
        Ok(())
    }

    pub async fn whenTableHasIndex(
        &mut self,
        table: &str,
        index: KeySelector<'_>,
        configure: impl FnOnce(&mut Blueprint),
        index_type: Option<&str>,
    ) -> DatabaseResult<()> {
        if self.hasIndexWith(table, index, index_type).await? {
            self.table(table, configure).await?;
        }
        Ok(())
    }

    pub async fn whenTableDoesntHaveIndex(
        &mut self,
        table: &str,
        index: KeySelector<'_>,
        configure: impl FnOnce(&mut Blueprint),
        index_type: Option<&str>,
    ) -> DatabaseResult<()> {
        if !self.hasIndexWith(table, index, index_type).await? {
            self.table(table, configure).await?;
        }
        Ok(())
    }

    /// Drops permanent tables in the selected database.
    pub async fn dropAllTables(&mut self) -> DatabaseResult<()> {
        self.drop_schema_objects(true, false).await
    }

    /// Drops views in the selected database.
    pub async fn dropAllViews(&mut self) -> DatabaseResult<()> {
        self.drop_schema_objects(false, true).await
    }

    pub(in crate::database) async fn drop_schema_objects(
        &mut self,
        drop_tables: bool,
        drop_views: bool,
    ) -> DatabaseResult<()> {
        let Executor::Connection(connection) = &self.executor else {
            return Err(DatabaseError::UnsupportedSchema(
                "schema cleanup requires a connection builder",
            ));
        };
        if self.grammar.mysql() {
            return connection
                .drop_mysql_schema_objects(drop_tables, drop_views)
                .await;
        }
        let grammar = connection.grammar.clone();
        connection.sqlite_schema_transaction(move |transaction, _| Box::pin(async move {
            let rows = transaction.select(
                "select name, type from pragma_table_list where schema = 'main' and type in ('table','virtual','view')                  and substr(name,1,7) <> 'sqlite_' order by type = 'view' desc, name",
                &[],
            ).await?;
            for row in &rows {
                let Some(Value::Text(name)) = row.get("name") else {
                    return Err(DatabaseError::InvalidSchema("catalog table name"));
                };
                let kind = match row.get("type") {
                    Some(Value::Text(kind)) if kind == "view" && drop_views => "view",
                    Some(Value::Text(kind)) if drop_tables && matches!(kind.as_str(), "table" | "virtual") => "table",
                    _ => continue,
                };
                transaction.statement(&format!("drop {kind} main.{}", grammar.quote(name)?), &[]).await?;
            }
            Ok(())
        })).await
    }

    pub async fn dropColumns(&mut self, table: &str, columns: &[&str]) -> DatabaseResult<()> {
        self.table(table, |blueprint| {
            blueprint.dropColumn(columns);
        })
        .await
    }

    pub async fn drop(&mut self, table: &str) -> DatabaseResult<()> {
        self.drop_table(table, false).await
    }

    pub async fn dropIfExists(&mut self, table: &str) -> DatabaseResult<()> {
        self.drop_table(table, true).await
    }

    async fn drop_table(&mut self, table: &str, if_exists: bool) -> DatabaseResult<()> {
        let table = self.resolve_table(table)?;
        let temporary = self.temporary_drop(&table, if_exists)?;
        if let Executor::Session(session) = &mut self.executor
            && let Some(index) = session.temporary_index(&table)
        {
            return session.drop_temporary(index, &temporary).await;
        }
        let table = self.grammar.qualified(&table)?;
        let guard = if if_exists { " if exists" } else { "" };
        self.execute(vec![format!("drop table{guard} {table}")])
            .await
    }

    pub async fn rename(&mut self, from: &str, to: &str) -> DatabaseResult<()> {
        let from = self.resolve_table(from)?;
        let mut to = self.grammar.table(to)?;
        let temporary = match &self.executor {
            Executor::Session(session) => session.temporary_index(&from),
            _ => None,
        };
        if temporary.is_some() {
            self.temporary_schema(&mut to)?;
        }
        let target = if self.grammar.mysql() {
            self.grammar.qualified(&to)?
        } else {
            if to.schema.is_some() && from.schema != to.schema {
                return Err(DatabaseError::UnsupportedSchema(
                    "SQLite cross-schema table rename",
                ));
            }
            self.grammar.identifier(&to.name)?
        };
        let source = self.grammar.qualified(&from)?;
        let sql = format!("alter table {source} rename to {target}");
        if let Some(index) = temporary {
            let cleanup = self.temporary_drop(&to, true)?;
            if let Executor::Session(session) = &mut self.executor {
                return session.rename_temporary(index, to, cleanup, &sql).await;
            }
        }
        self.execute(vec![sql]).await
    }

    async fn execute(&mut self, statements: Vec<String>) -> DatabaseResult<()> {
        if statements.is_empty() {
            return Ok(());
        }
        match &mut self.executor {
            Executor::Pretend(output) => {
                output.extend(statements);
                Ok(())
            }
            Executor::Connection(connection) if self.grammar.mysql() => {
                for sql in statements {
                    connection.statement(&sql, &[]).await?;
                }
                Ok(())
            }
            Executor::Session(session) => session.execute_schema(statements).await,
            Executor::Transaction(_) if self.grammar.mysql() => Err(
                DatabaseError::UnsupportedSchema("MySQL DDL implicitly commits"),
            ),
            Executor::Connection(connection) => {
                let statements: Arc<[String]> = statements.into();
                connection
                    .transaction(
                        |transaction| Box::pin(run_statements(transaction, statements.clone())),
                        1,
                    )
                    .await
            }
            Executor::Transaction(transaction) => {
                let statements: Arc<[String]> = statements.into();
                transaction
                    .transaction(|transaction| Box::pin(run_statements(transaction, statements)))
                    .await
            }
        }
    }

    async fn select(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<Vec<Row>> {
        match &mut self.executor {
            Executor::Pretend(_) => Ok(Vec::new()),
            Executor::Connection(connection) => {
                connection.selectFromWriteConnection(sql, bindings).await
            }
            Executor::Transaction(transaction) => transaction.select(sql, bindings).await,
            Executor::Session(session) => session.select(sql, bindings).await,
        }
    }
}

async fn run_statements(
    transaction: &mut Transaction,
    statements: Arc<[String]>,
) -> DatabaseResult<()> {
    for sql in statements.iter() {
        transaction.statement(sql, &[]).await?;
    }
    Ok(())
}
