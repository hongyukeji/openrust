use crate::database::{ConnectionInterface, DatabaseFuture, DatabaseResult};

/// A compiled migration registered with the application.
pub trait Migration: Send + Sync {
    fn get_connection(&self) -> Option<&str>;
    fn within_transaction(&self) -> bool;

    fn should_run<'a>(
        &'a self,
        connection: &'a mut dyn ConnectionInterface,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>>;

    fn up<'a>(
        &'a self,
        connection: &'a mut dyn ConnectionInterface,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;

    fn down<'a>(
        &'a self,
        connection: &'a mut dyn ConnectionInterface,
    ) -> DatabaseFuture<'a, DatabaseResult<()>>;
}
