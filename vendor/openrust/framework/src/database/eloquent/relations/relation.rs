use crate::database::{ConnectionInterface, DatabaseFuture, DatabaseResult};

/// The result contract shared by model relationships.
pub trait Relation: Send + Sync {
    type Output: Send;

    fn get_results<'a>(
        &'a self,
        connection: &'a mut dyn ConnectionInterface,
    ) -> DatabaseFuture<'a, DatabaseResult<Self::Output>>;
}
