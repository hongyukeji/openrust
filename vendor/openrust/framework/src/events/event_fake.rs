use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use crate::contracts::events::Event;

#[derive(Default)]
struct RecordedEvents {
    excluded: HashSet<String>,
    excluded_predicates: Vec<Predicate>,
    events: HashMap<String, Vec<Arc<dyn Event>>>,
}

type Predicate = Arc<dyn Fn(&dyn Event) -> bool + Send + Sync>;

/// Records events selected by an application's fake dispatcher.
pub struct EventFake {
    selected: HashSet<String>,
    predicate: Option<Predicate>,
    recorded: Mutex<RecordedEvents>,
}

#[allow(non_snake_case, reason = "Matches Laravel's EventFake assertions")]
impl EventFake {
    pub(crate) fn new(events: &[&str], except: &[&str]) -> Arc<Self> {
        Arc::new(Self {
            selected: events.iter().map(|&name| name.to_owned()).collect(),
            predicate: None,
            recorded: Mutex::new(RecordedEvents {
                excluded: except.iter().map(|&name| name.to_owned()).collect(),
                ..RecordedEvents::default()
            }),
        })
    }

    pub(crate) fn matching(
        events: &[&str],
        predicate: impl Fn(&dyn Event) -> bool + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            selected: events.iter().map(|&name| name.to_owned()).collect(),
            predicate: Some(Arc::new(predicate)),
            recorded: Mutex::default(),
        })
    }

    /// Lets the given events reach their normal listeners.
    pub fn except(&self, events: &[&str]) -> &Self {
        self.recorded()
            .excluded
            .extend(events.iter().map(|&name| name.to_owned()));
        self
    }

    /// Lets events that match the predicate reach their normal listeners.
    pub fn exceptWhen(
        &self,
        predicate: impl Fn(&dyn Event) -> bool + Send + Sync + 'static,
    ) -> &Self {
        self.recorded()
            .excluded_predicates
            .push(Arc::new(predicate));
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

    /// Filters recorded payloads for the given name, retaining dispatch order.
    pub fn dispatchedWhere(
        &self,
        event: &str,
        mut predicate: impl FnMut(&dyn Event) -> bool,
    ) -> Vec<Arc<dyn Event>> {
        self.dispatched(event)
            .into_iter()
            .filter(|payload| predicate(payload.as_ref()))
            .collect()
    }

    /// Asserts that the event was dispatched at least once.
    pub fn assertDispatched(&self, event: &str) -> &Self {
        assert!(self.count(event) > 0, "Event [{event}] was not dispatched.");
        self
    }

    /// Asserts that at least one recorded payload satisfies the predicate.
    pub fn assertDispatchedWhere(
        &self,
        event: &str,
        predicate: impl FnMut(&dyn Event) -> bool,
    ) -> &Self {
        assert!(
            !self.dispatchedWhere(event, predicate).is_empty(),
            "Event [{event}] was not dispatched with a matching payload."
        );
        self
    }

    /// Asserts that the event was dispatched exactly once.
    pub fn assertDispatchedOnce(&self, event: &str) -> &Self {
        self.assertDispatchedTimes(event, 1)
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

    /// Asserts that no recorded payload satisfies the predicate.
    pub fn assertNotDispatchedWhere(
        &self,
        event: &str,
        predicate: impl FnMut(&dyn Event) -> bool,
    ) -> &Self {
        assert!(
            self.dispatchedWhere(event, predicate).is_empty(),
            "Event [{event}] was dispatched with a matching payload."
        );
        self
    }

    /// Asserts that no faked events were dispatched.
    pub fn assertNothingDispatched(&self) -> &Self {
        let count: usize = self.recorded().events.values().map(Vec::len).sum();
        assert_eq!(count, 0, "Unexpected faked events were dispatched.");
        self
    }

    pub(super) fn may_match_name(&self, event: &str) -> bool {
        self.may_select_name(event) && !self.recorded().excluded.contains(event)
    }

    pub(super) fn matches(&self, event: &dyn Event) -> bool {
        let name = event.name();
        if !self.may_select_name(name) {
            return false;
        }
        let exclusions = {
            let recorded = self.recorded();
            if recorded.excluded.contains(name) {
                return false;
            }
            recorded.excluded_predicates.clone()
        };
        !exclusions.iter().any(|predicate| predicate(event))
            && (self.selected.contains(name)
                || self
                    .predicate
                    .as_ref()
                    .is_none_or(|predicate| predicate(event)))
    }

    fn may_select_name(&self, name: &str) -> bool {
        self.predicate.is_some() || self.selected.is_empty() || self.selected.contains(name)
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
