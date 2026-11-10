use crate::database::ConnectionInterface;

/// Inspects and changes the schema on its bound connection.
pub struct Builder<'connection> {
    _connection: &'connection mut dyn ConnectionInterface,
}
