use crate::database::{DatabaseFuture, DatabaseResult};

/// A recorded migration and its batch number.
pub struct MigrationRecord {
    pub id: u64,
    pub migration: String,
    pub batch: u64,
}

/// Persists the history of migrations on the selected database.
pub trait MigrationRepositoryInterface: Send {
    fn set_source(&mut self, name: Option<&str>);
    fn get_ran(&mut self) -> DatabaseFuture<'_, DatabaseResult<Vec<String>>>;
    fn get_migrations(
        &mut self,
        steps: u64,
    ) -> DatabaseFuture<'_, DatabaseResult<Vec<MigrationRecord>>>;
    fn get_migrations_by_batch(
        &mut self,
        batch: u64,
    ) -> DatabaseFuture<'_, DatabaseResult<Vec<MigrationRecord>>>;
    fn get_last(&mut self) -> DatabaseFuture<'_, DatabaseResult<Vec<MigrationRecord>>>;
    fn get_migration_batches(&mut self) -> DatabaseFuture<'_, DatabaseResult<Vec<(String, u64)>>>;
    fn get_next_batch_number(&mut self) -> DatabaseFuture<'_, DatabaseResult<u64>>;
    fn create_repository(&mut self) -> DatabaseFuture<'_, DatabaseResult<()>>;
    fn repository_exists(&mut self) -> DatabaseFuture<'_, DatabaseResult<bool>>;
    fn delete_repository(&mut self) -> DatabaseFuture<'_, DatabaseResult<()>>;
    fn log<'a>(&'a mut self, file: &'a str, batch: u64) -> DatabaseFuture<'a, DatabaseResult<()>>;
    fn delete<'a>(
        &'a mut self,
        migration: &'a MigrationRecord,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}
