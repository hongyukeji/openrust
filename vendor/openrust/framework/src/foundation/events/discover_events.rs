use std::{any::type_name, error::Error};

use super::event_cache::EventCache;
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
    conditional: Vec<(&'static str, DiscoveryCondition, Register)>,
}

impl DiscoverEvents {
    pub const fn new(registrations: &'static [Register]) -> Self {
        Self {
            registrations,
            conditional: Vec::new(),
        }
    }

    /// Adds a listener registration gated by its discovery policy at application boot.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn withListener<L: ShouldBeDiscovered>(mut self, register: Register) -> Self {
        self.conditional
            .push((type_name::<L>(), L::should_be_discovered, register));
        self
    }

    pub(in crate::foundation) fn load(
        &self,
        app: &Application,
    ) -> Result<Vec<bool>, Box<dyn Error + Send + Sync>> {
        if self.conditional.is_empty() {
            return Ok(Vec::new());
        }
        let cached = EventCache::load(app)?
            .and_then(|cache| cache.decisions(self.conditional.iter().map(|(name, _, _)| *name)));
        Ok(match cached {
            Some(decisions) => decisions,
            None => self.evaluate(app)?,
        })
    }

    pub(in crate::foundation) fn cache(
        &self,
        app: &Application,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let decisions = self.evaluate(app)?;
        let entries = self
            .conditional
            .iter()
            .zip(decisions.iter().copied())
            .map(|((name, _, _), enabled)| ((*name).to_owned(), enabled))
            .collect();
        // Verify the selected registrars before replacing a working cache.
        self.register(&decisions, &mut ListenerRegistry::default())?;
        EventCache::new(entries).save(app)?;
        Ok(())
    }

    fn evaluate(&self, app: &Application) -> EventResult<Vec<bool>> {
        self.conditional
            .iter()
            .map(|(_, condition, _)| condition(app))
            .collect()
    }

    pub(in crate::foundation) fn register(
        &self,
        decisions: &[bool],
        registry: &mut ListenerRegistry,
    ) -> EventResult<()> {
        for register in self.registrations {
            register(registry)?;
        }
        for ((_, _, register), enabled) in self.conditional.iter().zip(decisions) {
            if *enabled {
                register(registry)?;
            }
        }
        Ok(())
    }
}
