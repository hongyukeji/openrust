use std::any::Any;

/// A typed event with a name shared by exact and wildcard listeners.
pub trait Event: Any + Send + Sync {
    fn name(&self) -> &str;
}

impl dyn Event {
    /// Reads a payload registered by its Rust type.
    pub fn downcast_ref<T: Event>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }
}
