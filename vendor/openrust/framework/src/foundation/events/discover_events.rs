use crate::events::{EventResult, ListenerRegistry};

type Register = fn(&mut ListenerRegistry) -> EventResult<()>;

/// A compiled list of application listener registration functions.
pub struct DiscoverEvents {
    registrations: &'static [Register],
}

impl DiscoverEvents {
    pub const fn new(registrations: &'static [Register]) -> Self {
        Self { registrations }
    }

    pub(crate) fn register(&self, registry: &mut ListenerRegistry) -> EventResult<()> {
        for register in self.registrations {
            register(registry)?;
        }
        Ok(())
    }
}
