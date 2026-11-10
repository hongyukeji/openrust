use std::any::Any;

/// A typed event with a name shared by exact and wildcard listeners.
pub trait Event: Any + Send + Sync {
    fn name(&self) -> &str;
}
