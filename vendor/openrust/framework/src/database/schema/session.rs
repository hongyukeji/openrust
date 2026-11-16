use std::time::Instant;

use crate::{
    database::{
        Bindings, Connection, DatabaseError, DatabaseResult, Row, Value,
        connection::{PhysicalConnection, TransactionCommand},
    },
    events::EventContext,
};

use super::Builder;

/// A leased write session for connection-local schema settings and operations.
pub struct SchemaSession {
    connection: Connection,
    physical: Option<PhysicalConnection>,
    restore_foreign_keys: Option<bool>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Schema Builder API")]
impl SchemaSession {
    pub(super) async fn new(connection: Connection) -> DatabaseResult<Self> {
        let physical = connection.acquire(false).await?;
        Ok(Self {
            connection,
            physical: Some(physical),
            restore_foreign_keys: None,
        })
    }

    pub(super) fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn getSchemaBuilder(&mut self) -> Builder<'_> {
        Builder::session_builder(self)
    }

    pub async fn enableForeignKeyConstraints(&mut self) -> DatabaseResult<bool> {
        self.set_foreign_keys(true).await
    }

    pub async fn disableForeignKeyConstraints(&mut self) -> DatabaseResult<bool> {
        self.set_foreign_keys(false).await
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
        apply_foreign_keys(self.physical()?, sqlite, enabled).await?;
        Ok(true)
    }

    /// Restores the original constraint setting before releasing the write session.
    pub async fn close(mut self) -> DatabaseResult<()> {
        if let Some(original) = self.restore_foreign_keys {
            let sqlite = self.connection.is_sqlite();
            restore(self.physical()?, sqlite, original).await?;
            self.restore_foreign_keys = None;
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
        let start = Instant::now();
        self.physical()?.affecting_statement(sql, bindings).await?;
        self.connection.setRecordModificationState(true);
        self.after_query(sql, bindings, start).await?;
        Ok(true)
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
        if let (Some(original), Some(mut physical), Ok(runtime)) = (
            self.restore_foreign_keys.take(),
            self.physical.take(),
            tokio::runtime::Handle::try_current(),
        ) {
            let sqlite = self.connection.is_sqlite();
            runtime.spawn(async move {
                let _ = restore(&mut physical, sqlite, original).await;
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

async fn restore(
    physical: &mut PhysicalConnection,
    sqlite: bool,
    enabled: bool,
) -> DatabaseResult<()> {
    if sqlite && physical.in_transaction() {
        physical
            .control("roll back schema", TransactionCommand::RollBack)
            .await?;
    }
    apply_foreign_keys(physical, sqlite, enabled).await?;
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
