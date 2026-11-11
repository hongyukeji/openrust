use std::sync::Arc;

use indexmap::IndexMap;

use crate::contracts::events::{Listener, ShouldHandleEventsAfterCommit};

use super::{Delivery, EventError, EventResult};

#[derive(Clone)]
pub(super) struct Registration {
    pub listener: Arc<dyn Listener>,
    label: &'static str,
    pub after_commit: bool,
}

#[derive(Default)]
pub struct ListenerRegistry {
    exact: IndexMap<String, Vec<Registration>>,
    wildcard: IndexMap<String, Vec<Registration>>,
}

impl ListenerRegistry {
    pub fn listen(
        &mut self,
        event: &str,
        listener: Arc<dyn Listener>,
        delivery: Delivery,
    ) -> EventResult<()> {
        if event.trim().is_empty() {
            return Err(EventError::InvalidRegistration);
        }
        let after_commit = match delivery {
            Delivery::Sync => false,
            Delivery::AfterCommit => true,
            Delivery::Queued(_) => return Err(EventError::QueueUnavailable),
        };
        let registrations = if event.contains('*') {
            &mut self.wildcard
        } else {
            &mut self.exact
        };
        registrations
            .entry(event.to_owned())
            .or_default()
            .push(Registration {
                label: listener.name(),
                listener,
                after_commit,
            });
        Ok(())
    }

    #[allow(non_snake_case, reason = "Matches Laravel's after-commit terminology")]
    pub fn listenAfterCommit(
        &mut self,
        event: &str,
        listener: impl ShouldHandleEventsAfterCommit + 'static,
    ) -> EventResult<()> {
        self.listen(event, Arc::new(listener), Delivery::AfterCommit)
    }

    pub(super) fn extend(&mut self, other: Self) {
        for (existing, incoming) in [
            (&mut self.exact, other.exact),
            (&mut self.wildcard, other.wildcard),
        ] {
            for (name, listeners) in incoming {
                existing.entry(name).or_default().extend(listeners);
            }
        }
    }

    pub(super) fn forget(&mut self, event: &str) -> Option<Vec<Registration>> {
        if event.contains('*') {
            self.wildcard.shift_remove(event)
        } else {
            self.exact.shift_remove(event)
        }
    }

    pub(super) fn has_listeners(&self, event: &str) -> bool {
        self.exact.contains_key(event)
            || self
                .wildcard
                .keys()
                .any(|pattern| wildcard_matches(pattern, event))
    }

    pub(super) fn selected(&self, event: &str) -> Vec<Registration> {
        self.exact
            .get(event)
            .into_iter()
            .chain(
                self.wildcard
                    .iter()
                    .filter(|(pattern, _)| wildcard_matches(pattern, event))
                    .map(|(_, listeners)| listeners),
            )
            .flatten()
            .cloned()
            .collect()
    }

    pub(super) fn listing(&self) -> Vec<(String, Vec<&'static str>)> {
        self.exact
            .iter()
            .chain(&self.wildcard)
            .map(|(event, listeners)| {
                (
                    event.clone(),
                    listeners
                        .iter()
                        .map(|registration| registration.label)
                        .collect(),
                )
            })
            .collect()
    }
}

// The v0.x matcher uses only '*' as a wildcard; '?' remains a literal.
fn wildcard_matches(pattern: &str, event: &str) -> bool {
    let (pattern, event) = (pattern.as_bytes(), event.as_bytes());
    let (mut cursor, mut position, mut retry) = (0, 0, 0);
    let mut star = None;
    while position < event.len() {
        if cursor < pattern.len() && pattern[cursor] == b'*' {
            star = Some(cursor);
            cursor += 1;
            retry = position;
        } else if cursor < pattern.len() && pattern[cursor] == event[position] {
            cursor += 1;
            position += 1;
        } else if let Some(previous) = star {
            retry += 1;
            position = retry;
            cursor = previous + 1;
        } else {
            return false;
        }
    }
    pattern[cursor..].iter().all(|byte| *byte == b'*')
}
