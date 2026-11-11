use crate::database::{Connection, DatabaseFuture, DatabaseResult, Value, schema};

/// A compiled migration registered with the application.
#[allow(non_snake_case, reason = "Matches Laravel's Migration API")]
pub trait Migration: Send + Sync {
    fn getConnection(&self) -> Option<&str> {
        None
    }

    fn shouldRun<'a>(
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
    statements: Option<Vec<String>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's connection API")]
impl MigrationContext {
    pub(super) fn new(connection: Connection, pretend: bool) -> Self {
        Self {
            connection,
            statements: pretend.then(Vec::new),
        }
    }

    pub fn getSchemaBuilder(&mut self) -> schema::Builder<'_> {
        match &mut self.statements {
            Some(statements) => schema::Builder::pretend(&self.connection, statements),
            None => self.connection.getSchemaBuilder(),
        }
    }

    pub async fn statement(&mut self, sql: &str, bindings: &[Value]) -> DatabaseResult<bool> {
        if let Some(statements) = &mut self.statements {
            statements.push(sql.to_owned());
            return Ok(true);
        }
        self.connection.statement(sql, bindings).await
    }

    pub fn getName(&self) -> &str {
        self.connection.getName()
    }

    pub(super) fn take_statements(self) -> Vec<String> {
        self.statements.unwrap_or_default()
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
