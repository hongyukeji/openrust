use crate::database::ConnectionInterface;

/// Builds a query on the connection that owns its execution state.
pub struct Builder<'connection> {
    _connection: &'connection mut dyn ConnectionInterface,
}
