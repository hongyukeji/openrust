use std::sync::Arc;

use crate::database::{Connection, DatabaseError, DatabaseResult, Row, Transaction, Value};

use super::{Blueprint, grammars::Grammar};

mod catalog;
mod sqlite;

enum Executor<'connection> {
    Connection(Connection),
    Transaction(&'connection mut Transaction),
    Pretend(&'connection mut Vec<String>),
}

/// Inspects and changes the schema on the write connection.
pub struct Builder<'connection> {
    executor: Executor<'connection>,
    grammar: Grammar,
}

#[allow(non_snake_case, reason = "Matches Laravel's Schema Builder API")]
impl<'connection> Builder<'connection> {
    pub(crate) fn connection(connection: Connection) -> Builder<'static> {
        let grammar = Grammar::new(
            connection.grammar.clone(),
            connection.prefix_indexes,
            connection.getDatabaseName(),
        );
        Builder {
            executor: Executor::Connection(connection),
            grammar,
        }
    }

    pub(crate) fn pretend(
        connection: &Connection,
        statements: &'connection mut Vec<String>,
    ) -> Self {
        let grammar = Grammar::new(
            connection.grammar.clone(),
            connection.prefix_indexes,
            connection.getDatabaseName(),
        );
        Self {
            executor: Executor::Pretend(statements),
            grammar,
        }
    }

    pub(crate) fn transaction(transaction: &'connection mut Transaction) -> Self {
        let connection = transaction.query_connection();
        let grammar = Grammar::new(
            connection.grammar.clone(),
            connection.prefix_indexes,
            connection.getDatabaseName(),
        );
        Self {
            executor: Executor::Transaction(transaction),
            grammar,
        }
    }

    pub async fn create(
        &mut self,
        table: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        let table = self.grammar.table(table)?;
        let mut blueprint = Blueprint::default();
        configure(&mut blueprint);
        let sql = self.grammar.create(&table, blueprint)?;
        self.execute(sql).await
    }

    pub async fn table(
        &mut self,
        table: &str,
        configure: impl FnOnce(&mut Blueprint),
    ) -> DatabaseResult<()> {
        let table = self.grammar.table(table)?;
        let mut blueprint = Blueprint::default();
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
        let table = self.grammar.qualified(&self.grammar.table(table)?)?;
        let guard = if if_exists { " if exists" } else { "" };
        self.execute(vec![format!("drop table{guard} {table}")])
            .await
    }

    pub async fn rename(&mut self, from: &str, to: &str) -> DatabaseResult<()> {
        let from = self.grammar.table(from)?;
        let to = self.grammar.table(to)?;
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
        self.execute(vec![format!("alter table {source} rename to {target}")])
            .await
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
