use std::time::Instant;

use crate::{
    database::{
        Bindings, Connection, DatabaseError, DatabaseResult, Row, Value,
        connection::{PhysicalConnection, TransactionCommand},
    },
    events::EventContext,
};

use super::{Builder, TableInfo, grammars::Table};

/// A leased write session for connection-local schema settings and operations.
pub struct SchemaSession {
    connection: Connection,
    physical: Option<PhysicalConnection>,
    restore_foreign_keys: Option<bool>,
    temporary_tables: Vec<(Table, String)>,
    mysql_case_insensitive: Option<bool>,
}

impl SchemaSession {
    pub(super) async fn new(connection: Connection) -> DatabaseResult<Self> {
        let physical = connection.acquire(false).await?;
        Ok(Self {
            connection,
            physical: Some(physical),
            restore_foreign_keys: None,
            temporary_tables: Vec::new(),
            mysql_case_insensitive: None,
        })
    }

    pub(super) fn connection(&self) -> &Connection {
        &self.connection
    }

    pub(super) fn temporary_index(&self, table: &Table) -> Option<usize> {
        let sqlite = self.connection.is_sqlite();
        let database = self.connection.get_database_name();
        self.temporary_tables.iter().position(|(existing, _)| {
            if sqlite {
                table
                    .schema
                    .as_deref()
                    .is_none_or(|schema| schema.eq_ignore_ascii_case("temp"))
                    && existing.name.eq_ignore_ascii_case(&table.name)
            } else {
                let matches = |left: &str, right: &str| {
                    if self.mysql_case_insensitive == Some(true) {
                        left.eq_ignore_ascii_case(right)
                    } else {
                        left == right
                    }
                };
                matches(
                    existing.schema.as_deref().unwrap_or(database),
                    table.schema.as_deref().unwrap_or(database),
                ) && matches(&existing.name, &table.name)
            }
        })
    }

    pub(super) fn temporary_table(&self, table: &Table) -> Option<&Table> {
        self.temporary_index(table)
            .map(|index| &self.temporary_tables[index].0)
    }

    pub(super) async fn create_temporary(
        &mut self,
        table: Table,
        cleanup: String,
        statements: Vec<String>,
    ) -> DatabaseResult<()> {
        let mut statements = statements.into_iter();
        let sql = statements
            .next()
            .ok_or(DatabaseError::InvalidSchema("temporary table definition"))?;
        self.before_query(&sql, (&[]).into()).await?;
        if !self.connection.is_sqlite() && self.mysql_case_insensitive.is_none() {
            let value = self
                .physical()?
                .select_one("select @@lower_case_table_names", &[])
                .await?
                .and_then(|row| row.get_at(0).cloned());
            self.mysql_case_insensitive = Some(match value {
                Some(Value::Integer(0) | Value::Unsigned(0)) => false,
                Some(Value::Integer(1 | 2) | Value::Unsigned(1 | 2)) => true,
                _ => return Err(DatabaseError::InvalidSchema("MySQL identifier case mode")),
            });
        }
        self.physical()?.retire_on_drop(true);
        let result = self.execute_statement(&sql, (&[]).into()).await;
        let started = match result {
            Ok(start) => start,
            Err(error) => {
                self.release_clean_lease();
                return Err(error);
            }
        };
        self.temporary_tables.push((table, cleanup));
        let result = async {
            self.after_query(&sql, (&[]).into(), started).await?;
            for sql in statements {
                self.statement(&sql, &[]).await?;
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            let index = self.temporary_tables.len() - 1;
            let drop = self.temporary_tables[index].1.clone();
            if self.physical()?.unprepared(&drop).await.is_ok() {
                self.temporary_tables.pop();
                self.release_clean_lease();
            }
        }
        result
    }

    pub(super) async fn drop_temporary(&mut self, index: usize, sql: &str) -> DatabaseResult<()> {
        self.before_query(sql, (&[]).into()).await?;
        let started = self.execute_statement(sql, (&[]).into()).await?;
        self.temporary_tables.remove(index);
        self.release_clean_lease();
        self.after_query(sql, (&[]).into(), started).await
    }

    pub(super) async fn rename_temporary(
        &mut self,
        index: usize,
        target: Table,
        cleanup: String,
        sql: &str,
    ) -> DatabaseResult<()> {
        self.before_query(sql, (&[]).into()).await?;
        let started = self.execute_statement(sql, (&[]).into()).await?;
        self.temporary_tables[index] = (target, cleanup);
        self.after_query(sql, (&[]).into(), started).await
    }

    fn release_clean_lease(&mut self) {
        if self.temporary_tables.is_empty()
            && self.restore_foreign_keys.is_none()
            && let Some(physical) = &mut self.physical
            && !physical.interrupted()
        {
            physical.retire_on_drop(false);
        }
    }

    pub fn get_schema_builder(&mut self) -> Builder<'_> {
        Builder::session_builder(self)
    }

    pub(crate) async fn truncate_nonempty_catalog_table(
        &mut self,
        table: &TableInfo,
    ) -> DatabaseResult<()> {
        if !self.connection.grammar.is_mysql()
            || table.schema != self.connection.get_database_name()
        {
            return Err(DatabaseError::InvalidSchema(
                "expected a table in the current MySQL schema",
            ));
        }
        let database = self.connection.grammar.quote(&table.schema)?;
        let name = self.connection.grammar.quote(&table.name)?;
        let target = format!("{database}.{name}");
        if !self
            .select(&format!("select 1 from {target} limit 1"), &[])
            .await?
            .is_empty()
        {
            self.statement(&format!("truncate table {target}"), &[])
                .await?;
        }
        Ok(())
    }

    query_callers! {
    pub async fn enable_foreign_key_constraints(&mut self) -> DatabaseResult<bool> {
        self.set_foreign_keys(true).await
    }

    pub async fn disable_foreign_key_constraints(&mut self) -> DatabaseResult<bool> {
        self.set_foreign_keys(false).await
    }
    }

    async fn set_foreign_keys(&mut self, enabled: bool) -> DatabaseResult<bool> {
        self.connection.ensure_real_operation()?;
        let sqlite = self.connection.is_sqlite();
        if self.restore_foreign_keys.is_none() {
            let original = foreign_key_state(self.physical()?, sqlite).await?;
            if enabled == original {
                return Ok(true);
            }
            self.restore_foreign_keys = Some(original);
            self.physical()?.retire_on_drop(true);
        }
        let sql = foreign_key_sql(sqlite, enabled);
        let bindings = (&[]).into();
        self.before_query(sql, bindings).await?;
        let started = Instant::now();
        apply_foreign_keys(self.physical()?, sqlite, enabled).await?;
        self.after_query(sql, bindings, started).await?;
        Ok(true)
    }

    query_callers! {
    /// Drops managed temporary tables and restores the session settings before release.
    pub async fn close(mut self) -> DatabaseResult<()> {
        if self.temporary_tables.is_empty() && self.restore_foreign_keys.is_none() {
            return Ok(());
        }
        let sqlite = self.connection.is_sqlite();
        let restore = self.restore_foreign_keys.map(|enabled| foreign_key_sql(sqlite, enabled));
        let started = Instant::now();
        let physical = self
            .physical
            .as_mut()
            .ok_or(DatabaseError::TransactionClosed)?;
        cleanup(
            physical,
            sqlite,
            &self.temporary_tables,
            self.restore_foreign_keys,
        )
        .await?;
        self.temporary_tables.clear();
        self.restore_foreign_keys = None;
        if let Some(sql) = restore {
            self.after_query(sql, (&[]).into(), started).await?;
        }
        Ok(())
    }

    pub async fn select<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<Vec<Row>> {
        let bindings = bindings.into();
        self.before_query(sql, bindings).await?;
        let start = Instant::now();
        let rows = self.physical()?.select(sql, bindings).await?;
        self.after_query(sql, bindings, start).await?;
        Ok(rows)
    }

    pub async fn statement<'a>(
        &mut self,
        sql: &str,
        bindings: impl Into<Bindings<'a>>,
    ) -> DatabaseResult<bool> {
        let bindings = bindings.into();
        self.before_query(sql, bindings).await?;
        let start = self.execute_statement(sql, bindings).await?;
        self.after_query(sql, bindings, start).await?;
        Ok(true)
    }
    }

    async fn execute_statement(
        &mut self,
        sql: &str,
        bindings: Bindings<'_>,
    ) -> DatabaseResult<Instant> {
        let start = Instant::now();
        self.physical()?.affecting_statement(sql, bindings).await?;
        self.connection.set_record_modification_state(true);
        Ok(start)
    }

    pub(super) async fn execute_database_ddl(&mut self, sql: &str) -> DatabaseResult<()> {
        let bindings = (&[]).into();
        self.before_query(sql, bindings).await?;
        let start = Instant::now();
        self.physical()?.unprepared(sql).await?;
        self.connection.set_record_modification_state(true);
        self.after_query(sql, bindings, start).await
    }

    pub(super) fn in_transaction(&mut self) -> DatabaseResult<bool> {
        Ok(self.physical()?.in_transaction())
    }

    pub(super) async fn execute_schema(&mut self, statements: Vec<String>) -> DatabaseResult<()> {
        self.connection.ensure_real_operation()?;
        if !self.connection.is_sqlite() {
            return self.execute_statements(statements).await;
        }
        self.physical()?
            .control("begin schema", TransactionCommand::Begin)
            .await?;
        let result = self.execute_statements(statements).await;
        let command = if result.is_ok() {
            TransactionCommand::Commit
        } else {
            TransactionCommand::RollBack
        };
        let finish = self.physical()?.control("end schema", command).await;
        result.and(finish)
    }

    async fn execute_statements(&mut self, statements: Vec<String>) -> DatabaseResult<()> {
        for sql in statements {
            self.statement(&sql, &[]).await?;
        }
        Ok(())
    }

    fn physical(&mut self) -> DatabaseResult<&mut PhysicalConnection> {
        self.physical
            .as_mut()
            .ok_or(DatabaseError::TransactionClosed)
    }

    async fn before_query(&self, sql: &str, bindings: Bindings<'_>) -> DatabaseResult<()> {
        self.connection.ensure_real_operation()?;
        self.connection
            .run_before_executing(sql, bindings, None)
            .await
    }

    async fn after_query(
        &self,
        sql: &str,
        bindings: Bindings<'_>,
        start: Instant,
    ) -> DatabaseResult<()> {
        if let Some(notification) = self.connection.observe_query(
            sql,
            bindings,
            start.elapsed(),
            self.connection.write_query_role(),
        )? {
            notification.dispatch(&mut EventContext::new()).await?;
        }
        Ok(())
    }
}

impl Drop for SchemaSession {
    fn drop(&mut self) {
        if self.temporary_tables.is_empty() && self.restore_foreign_keys.is_none() {
            return;
        }
        if let (Some(mut physical), Ok(runtime)) =
            (self.physical.take(), tokio::runtime::Handle::try_current())
        {
            let sqlite = self.connection.is_sqlite();
            let tables = std::mem::take(&mut self.temporary_tables);
            let original = self.restore_foreign_keys.take();
            runtime.spawn(async move {
                let _ = cleanup(&mut physical, sqlite, &tables, original).await;
            });
        }
    }
}

fn foreign_key_sql(sqlite: bool, enabled: bool) -> &'static str {
    match (sqlite, enabled) {
        (true, true) => "pragma foreign_keys = on",
        (true, false) => "pragma foreign_keys = off",
        (false, true) => "set session foreign_key_checks = 1",
        (false, false) => "set session foreign_key_checks = 0",
    }
}

async fn cleanup(
    physical: &mut PhysicalConnection,
    sqlite: bool,
    tables: &[(Table, String)],
    foreign_keys: Option<bool>,
) -> DatabaseResult<()> {
    if sqlite && physical.in_transaction() {
        physical
            .control("roll back schema", TransactionCommand::RollBack)
            .await?;
    }
    for (_, statement) in tables.iter().rev() {
        physical.unprepared(statement).await?;
    }
    if let Some(enabled) = foreign_keys {
        apply_foreign_keys(physical, sqlite, enabled).await?;
    }
    physical.retire_on_drop(false);
    Ok(())
}

async fn foreign_key_state(
    physical: &mut PhysicalConnection,
    sqlite: bool,
) -> DatabaseResult<bool> {
    let sql = if sqlite {
        "pragma foreign_keys"
    } else {
        "select @@foreign_key_checks"
    };
    match physical
        .select_one(sql, &[])
        .await?
        .and_then(|row| row.get_at(0).cloned())
    {
        Some(Value::Integer(1) | Value::Unsigned(1) | Value::Bool(true)) => Ok(true),
        Some(Value::Integer(0) | Value::Unsigned(0) | Value::Bool(false)) => Ok(false),
        _ => Err(DatabaseError::InvalidSchema("foreign key check state")),
    }
}

async fn apply_foreign_keys(
    physical: &mut PhysicalConnection,
    sqlite: bool,
    enabled: bool,
) -> DatabaseResult<()> {
    physical
        .unprepared(foreign_key_sql(sqlite, enabled))
        .await?;
    if foreign_key_state(physical, sqlite).await? != enabled {
        return Err(DatabaseError::UnsupportedSchema(
            "cannot toggle foreign key constraints in this session state",
        ));
    }
    Ok(())
}
