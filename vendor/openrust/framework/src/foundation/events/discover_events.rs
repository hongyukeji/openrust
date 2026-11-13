use crate::{
    contracts::events::ShouldBeDiscovered,
    events::{EventResult, ListenerRegistry},
    foundation::Application,
};

type Register = fn(&mut ListenerRegistry) -> EventResult<()>;
type DiscoveryCondition = fn(&Application) -> EventResult<bool>;

/// A compiled list of application listener registration functions.
pub struct DiscoverEvents {
    registrations: &'static [Register],
    conditional: Vec<(DiscoveryCondition, Register)>,
}

impl DiscoverEvents {
    pub const fn new(registrations: &'static [Register]) -> Self {
        Self {
            registrations,
            conditional: Vec::new(),
        }
    }

    /// Adds a listener registration gated by its discovery policy at application boot.
    #[allow(non_snake_case, reason = "Matches Laravel's listener discovery naming")]
    pub fn withListener<L: ShouldBeDiscovered>(mut self, register: Register) -> Self {
        self.conditional.push((L::shouldBeDiscovered, register));
        self
    }

    pub(crate) fn register(
        &self,
        app: &Application,
        registry: &mut ListenerRegistry,
    ) -> EventResult<()> {
        for register in self.registrations {
            register(registry)?;
        }
        for (should_discover, register) in &self.conditional {
            if should_discover(app)? {
                register(registry)?;
            }
        }
        Ok(())
    }
}
