use crate::events::{EventResult, ListenerRegistry};

/// Registers a group of event handlers together.
pub trait Subscriber: Send + Sync {
    fn subscribe(&self, registry: &mut ListenerRegistry) -> EventResult<()>;
}
