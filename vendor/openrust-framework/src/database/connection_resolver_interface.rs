use super::{Connection, DatabaseResult};

/// Resolves named connections and controls the default connection name.
pub trait ConnectionResolverInterface: Send + Sync {
    fn connection(&self, name: Option<&str>) -> DatabaseResult<Connection>;
    fn get_default_connection(&self) -> DatabaseResult<String>;
    fn set_default_connection(&self, name: &str) -> DatabaseResult<()>;
}
