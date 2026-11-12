use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use crate::contracts::events::Event;

#[derive(Default)]
struct RecordedEvents {
    excluded: HashSet<String>,
    events: HashMap<String, Vec<Arc<dyn Event>>>,
}

/// Records events selected by an application's fake dispatcher.
pub struct EventFake {
    selected: HashSet<String>,
    recorded: Mutex<RecordedEvents>,
}

#[allow(non_snake_case, reason = "Matches Laravel's EventFake assertions")]
impl EventFake {
    pub(super) fn new(events: &[&str]) -> Self {
        Self {
            selected: events.iter().map(|&name| name.to_owned()).collect(),
            recorded: Mutex::default(),
        }
    }

    /// Lets the given events reach their normal listeners.
    pub fn except(&self, events: &[&str]) -> &Self {
        self.recorded()
            .excluded
            .extend(events.iter().map(|&name| name.to_owned()));
        self
    }

    /// Returns recorded payloads in dispatch order for this event name.
    pub fn dispatched(&self, event: &str) -> Vec<Arc<dyn Event>> {
        self.recorded()
            .events
            .get(event)
            .cloned()
            .unwrap_or_default()
    }

    /// Asserts that the event was dispatched at least once.
    pub fn assertDispatched(&self, event: &str) -> &Self {
        assert!(self.count(event) > 0, "Event [{event}] was not dispatched.");
        self
    }

    /// Asserts that the event was dispatched exactly the given number of times.
    pub fn assertDispatchedTimes(&self, event: &str, times: usize) -> &Self {
        assert_eq!(self.count(event), times, "Dispatch count for [{event}].");
        self
    }

    /// Asserts that the event was never dispatched.
    pub fn assertNotDispatched(&self, event: &str) -> &Self {
        self.assertDispatchedTimes(event, 0)
    }

    /// Asserts that no faked events were dispatched.
    pub fn assertNothingDispatched(&self) -> &Self {
        let count: usize = self.recorded().events.values().map(Vec::len).sum();
        assert_eq!(count, 0, "Unexpected faked events were dispatched.");
        self
    }

    pub(super) fn matches(&self, event: &str) -> bool {
        (self.selected.is_empty() || self.selected.contains(event))
            && !self.recorded().excluded.contains(event)
    }

    pub(super) fn record(&self, event: Arc<dyn Event>) {
        self.recorded()
            .events
            .entry(event.name().to_owned())
            .or_default()
            .push(event);
    }

    fn count(&self, event: &str) -> usize {
        self.recorded().events.get(event).map_or(0, Vec::len)
    }

    fn recorded(&self) -> MutexGuard<'_, RecordedEvents> {
        self.recorded.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
