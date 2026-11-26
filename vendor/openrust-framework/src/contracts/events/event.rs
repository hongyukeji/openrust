use std::any::Any;

/// A typed event with a name shared by exact and wildcard listeners.
pub trait Event: Any + Send + Sync {
    fn name(&self) -> &str;

    /// The concrete Rust type, even when the event is observed through a trait object.
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Explicit serializable properties for diagnostics; opaque Rust values remain unavailable.
    fn properties(&self) -> Option<serde_json::Value> {
        None
    }

    /// Additional event contracts whose exact listeners receive this payload.
    ///
    /// Contract names must be distinct and differ from the event name. The
    /// returned order controls delivery after the exact and wildcard listeners.
    fn interfaces(&self) -> &[&str] {
        &[]
    }
}

impl dyn Event {
    /// Reads a payload registered by its Rust type.
    pub fn downcast_ref<T: Event>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }
}
