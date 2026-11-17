use std::sync::Arc;

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Row, Value,
    query::Record,
};

use super::{MigrationRecord, MigrationRepositoryInterface};

/// Stores migration history in the configured migration table on the writer.
pub struct DatabaseMigrationRepository {
    manager: Arc<DatabaseManager>,
    source: Option<String>,
    table: String,
}

impl DatabaseMigrationRepository {
    pub fn new(manager: Arc<DatabaseManager>, source: Option<&str>, table: &str) -> Self {
        Self {
            manager,
            source: source.map(str::to_owned),
            table: table.to_owned(),
        }
    }

    fn connection(&self) -> DatabaseResult<Connection> {
        self.manager.connection(self.source.as_deref())
    }

    pub(super) async fn records(&self) -> DatabaseResult<Vec<MigrationRecord>> {
        self.connection()?
            .table(&self.table)
            .useWritePdo()
            .orderByDesc("batch")
            .orderByDesc("migration")
            .get()
            .await?
            .iter()
            .map(record)
            .collect()
    }
}

impl MigrationRepositoryInterface for DatabaseMigrationRepository {
    fn set_source(&mut self, name: Option<&str>) {
        self.source = name.map(str::to_owned);
    }

    fn get_ran(&mut self) -> DatabaseFuture<'_, DatabaseResult<Vec<String>>> {
        Box::pin(async move {
            self.connection()?
                .table(&self.table)
                .useWritePdo()
                .orderBy("batch", "asc")
                .orderBy("migration", "asc")
                .pluck("migration")
                .await?
                .iter()
                .map(migration_name)
                .collect()
        })
    }

    fn get_migrations(
        &mut self,
        steps: u64,
    ) -> DatabaseFuture<'_, DatabaseResult<Vec<MigrationRecord>>> {
        Box::pin(async move {
            self.connection()?
                .table(&self.table)
                .useWritePdo()
                .r#where("batch", ">=", 1)
                .orderByDesc("batch")
                .orderByDesc("migration")
                .limit(steps)
                .get()
                .await?
                .iter()
                .map(record)
                .collect()
        })
    }

    fn get_migrations_by_batch(
        &mut self,
        batch: u64,
    ) -> DatabaseFuture<'_, DatabaseResult<Vec<MigrationRecord>>> {
        Box::pin(async move {
            self.connection()?
                .table(&self.table)
                .useWritePdo()
                .r#where("batch", "=", Value::Unsigned(batch))
                .orderByDesc("migration")
                .get()
                .await?
                .iter()
                .map(record)
                .collect()
        })
    }

    fn get_last(&mut self) -> DatabaseFuture<'_, DatabaseResult<Vec<MigrationRecord>>> {
        Box::pin(async move {
            let connection = self.connection()?;
            let latest = connection
                .table(&self.table)
                .selectRaw("max(batch)", vec![]);
            connection
                .table(&self.table)
                .useWritePdo()
                .select(["id", "migration", "batch"])
                .whereSub("batch", "=", &latest)?
                .orderByDesc("migration")
                .get()
                .await?
                .iter()
                .map(record)
                .collect()
        })
    }

    fn get_migration_batches(&mut self) -> DatabaseFuture<'_, DatabaseResult<Vec<(String, u64)>>> {
        Box::pin(async move {
            self.connection()?
                .table(&self.table)
                .useWritePdo()
                .select(["migration", "batch"])
                .orderBy("batch", "asc")
                .orderBy("migration", "asc")
                .get()
                .await?
                .iter()
                .map(|row| {
                    Ok((
                        migration_name(ledger_column(row, "migration")?)?,
                        unsigned(ledger_column(row, "batch")?)?,
                    ))
                })
                .collect()
        })
    }

    fn get_next_batch_number(&mut self) -> DatabaseFuture<'_, DatabaseResult<u64>> {
        Box::pin(async move {
            let batch = self
                .connection()?
                .table(&self.table)
                .useWritePdo()
                .max("batch")
                .await?;
            let batch = if batch == Value::Null {
                0
            } else {
                unsigned(&batch)?
            };
            batch
                .checked_add(1)
                .ok_or(DatabaseError::InvalidMigration("batch overflow"))
        })
    }

    fn create_repository(&mut self) -> DatabaseFuture<'_, DatabaseResult<()>> {
        Box::pin(async move {
            self.connection()?
                .getSchemaBuilder()
                .create(&self.table, |table| {
                    table.increments("id");
                    table.string("migration", 255);
                    table.integer("batch");
                })
                .await
        })
    }

    fn repository_exists(&mut self) -> DatabaseFuture<'_, DatabaseResult<bool>> {
        Box::pin(async move {
            self.connection()?
                .getSchemaBuilder()
                .hasTable(&self.table)
                .await
        })
    }

    fn delete_repository(&mut self) -> DatabaseFuture<'_, DatabaseResult<()>> {
        Box::pin(async move {
            self.connection()?
                .getSchemaBuilder()
                .drop(&self.table)
                .await
        })
    }

    fn log<'a>(&'a mut self, file: &'a str, batch: u64) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            self.connection()?
                .table(&self.table)
                .insert(&[Record::from([
                    ("migration".into(), file.into()),
                    ("batch".into(), Value::Unsigned(batch)),
                ])])
                .await?;
            Ok(())
        })
    }

    fn delete<'a>(
        &'a mut self,
        migration: &'a MigrationRecord,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            self.connection()?
                .table(&self.table)
                .r#where("id", "=", Value::Unsigned(migration.id))
                .delete()
                .await?;
            Ok(())
        })
    }
}

fn unsigned(value: &Value) -> DatabaseResult<u64> {
    match value {
        Value::Unsigned(value) => Ok(*value),
        Value::Integer(value) => {
            u64::try_from(*value).map_err(|_| DatabaseError::InvalidMigration("ledger number"))
        }
        _ => Err(DatabaseError::InvalidMigration("ledger number")),
    }
}

fn ledger_column<'a>(row: &'a Row, key: &str) -> DatabaseResult<&'a Value> {
    row.get(key)
        .ok_or(DatabaseError::InvalidMigration("ledger column"))
}

fn migration_name(value: &Value) -> DatabaseResult<String> {
    let Value::Text(migration) = value else {
        return Err(DatabaseError::InvalidMigration("ledger name"));
    };
    Ok(migration.clone())
}

fn record(row: &Row) -> DatabaseResult<MigrationRecord> {
    Ok(MigrationRecord {
        id: unsigned(ledger_column(row, "id")?)?,
        migration: migration_name(ledger_column(row, "migration")?)?,
        batch: unsigned(ledger_column(row, "batch")?)?,
    })
}
