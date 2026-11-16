use std::sync::{Arc, RwLock};

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Row, Transaction, Value,
};

use super::{
    Blueprint, MorphKeyType, SchemaSession,
    grammars::{Grammar, Table},
};

mod catalog;
mod sqlite;

enum Executor<'connection> {
    Connection(Connection),
    Transaction(&'connection mut Transaction),
    Session(&'connection mut SchemaSession),
    Pretend(&'connection mut Vec<String>),
}

/// Inspects and changes the schema on the write connection.
pub struct Builder<'connection> {
    executor: Executor<'connection>,
    grammar: Grammar,
    morph_key_type: Arc<RwLock<MorphKeyType>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Schema Builder API")]
impl<'connection> Builder<'connection> {
    pub(crate) fn connection(connection: Connection) -> Builder<'static> {
        let grammar = Grammar::new(&connection);
        Builder {
            morph_key_type: connection.morph_key_type.clone(),
            executor: Executor::Connection(connection),
            grammar,
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
            morph_key_type: connection.morph_key_type.clone(),
        }
    }

    pub(crate) fn transaction(transaction: &'connection mut Transaction) -> Self {
        let connection = transaction.query_connection();
        let grammar = Grammar::new(connection);
        let morph_key_type = connection.morph_key_type.clone();
        Self {
            executor: Executor::Transaction(transaction),
            grammar,
            morph_key_type,
        }
    }

    pub(super) fn session_builder(session: &'connection mut SchemaSession) -> Self {
        let connection = session.connection();
        let grammar = Grammar::new(connection);
        let morph_key_type = connection.morph_key_type.clone();
        Self {
            executor: Executor::Session(session),
            grammar,
            morph_key_type,
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

    fn blueprint(&self) -> Blueprint {
        Blueprint {
            morph_key_type: *self
                .morph_key_type
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            ..Blueprint::default()
        }
    }

    pub async fn create(
        &mut self,
        table: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        let mut table = self.grammar.table(table)?;
        let mut blueprint = self.blueprint();
        configure(&mut blueprint);
        if blueprint.temporary {
            if !matches!(self.executor, Executor::Session(_) | Executor::Pretend(_)) {
                return Err(DatabaseError::UnsupportedSchema(
                    "temporary table requires a schema session",
                ));
            }
            self.temporary_schema(&mut table)?;
            let sql = self.grammar.create(&table, blueprint)?;
            let cleanup = self.temporary_drop(&table, true)?;
            if let Executor::Session(session) = &mut self.executor {
                return session.create_temporary(table, cleanup, sql).await;
            }
            return self.execute(sql).await;
        }
        let sql = self.grammar.create(&table, blueprint)?;
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
        let table = self.resolve_table(table)?;
        let mut blueprint = self.blueprint();
        configure(&mut blueprint);
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
