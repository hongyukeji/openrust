use std::sync::Arc;

use indexmap::IndexMap;

use crate::{
    container::Container,
    contracts::{
        container::Container as _,
        events::{Event, Listener, ShouldHandleEventsAfterCommit},
        queue::JobHandler,
    },
};

use super::{
    CallQueuedListener, Delivery, EventError, EventFuture, EventResult, QueuedListenerOptions,
};

#[derive(Clone)]
pub(super) struct Registration {
    pub source: ListenerSource,
    pub(super) label: &'static str,
    pub after_commit: bool,
}

type ListenerResolver = fn(&Container) -> EventFuture<'_, EventResult<Arc<dyn Listener>>>;

#[derive(Clone)]
pub(super) enum ListenerSource {
    Instance(Arc<dyn Listener>),
    Container(ListenerResolver),
    Queued(Arc<QueuedRegistration>),
}

pub(super) struct QueuedRegistration {
    pub name: String,
    pub options: QueuedListenerOptions,
    pub encode: fn(&dyn Event) -> EventResult<serde_json::Value>,
    handler: Arc<dyn JobHandler>,
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
        self.add_listener(
            event,
            listener.name(),
            ListenerSource::Instance(listener),
            delivery,
        )
    }

    /// Resolves the listener from the dispatcher's application container on delivery.
    #[allow(non_snake_case, reason = "Matches Laravel's listener naming")]
    pub fn listenClass<L: Listener + 'static>(
        &mut self,
        event: &str,
        delivery: Delivery,
    ) -> EventResult<()> {
        self.add_listener(
            event,
            std::any::type_name::<L>(),
            ListenerSource::Container(|container| {
                Box::pin(async move {
                    let listener = container
                        .makeAsync::<L>()
                        .await
                        .map_err(EventError::Unavailable)?;
                    Ok(listener as Arc<dyn Listener>)
                })
            }),
            delivery,
        )
    }

    fn add_listener(
        &mut self,
        event: &str,
        label: &'static str,
        source: ListenerSource,
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
        self.add(
            event,
            Registration {
                label,
                source,
                after_commit,
            },
        );
        Ok(())
    }

    fn add(&mut self, event: &str, registration: Registration) {
        let registrations = if event.contains('*') {
            &mut self.wildcard
        } else {
            &mut self.exact
        };
        registrations
            .entry(event.to_owned())
            .or_default()
            .push(registration);
    }

    #[allow(non_snake_case, reason = "Matches Laravel's queued listener naming")]
    pub fn listenQueued<E: Event + serde::Serialize + serde::de::DeserializeOwned>(
        &mut self,
        event: &str,
        listener: Arc<dyn Listener>,
        options: QueuedListenerOptions,
    ) -> EventResult<()> {
        if event.trim().is_empty() || options.listener_id.trim().is_empty() {
            return Err(EventError::InvalidRegistration);
        }
        let queued = QueuedRegistration {
            name: format!("events.listener:{}", options.listener_id),
            options,
            encode: |event| {
                let event = event
                    .downcast_ref::<E>()
                    .ok_or(EventError::InvalidPayload)?;
                serde_json::to_value(event).map_err(|_| EventError::InvalidPayload)
            },
            handler: Arc::new(CallQueuedListener::<E>::new(listener.clone())),
        };
        self.add(
            event,
            Registration {
                label: listener.name(),
                source: ListenerSource::Queued(Arc::new(queued)),
                after_commit: false,
            },
        );
        Ok(())
    }

    pub(super) fn queued_handlers(&self) -> Vec<(String, Arc<dyn JobHandler>)> {
        self.exact
            .values()
            .chain(self.wildcard.values())
            .flatten()
            .filter_map(|registration| match &registration.source {
                ListenerSource::Queued(queued) => {
                    Some((queued.name.clone(), queued.handler.clone()))
                }
                _ => None,
            })
            .collect()
    }

    #[allow(non_snake_case, reason = "Matches Laravel's after-commit terminology")]
    pub fn listenAfterCommit(
        &mut self,
        event: &str,
        listener: impl Listener + ShouldHandleEventsAfterCommit + 'static,
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

    pub(super) fn selected(&self, name: &str, interfaces: &[&str]) -> Vec<Registration> {
        self.exact
            .get(name)
            .into_iter()
            .chain(
                self.wildcard
                    .iter()
                    .filter(|(pattern, _)| wildcard_matches(pattern, name))
                    .map(|(_, listeners)| listeners),
            )
            .chain(
                interfaces
                    .iter()
                    .enumerate()
                    .filter(|(index, interface)| {
                        **interface != name && !interfaces[..*index].contains(interface)
                    })
                    .filter_map(|(_, interface)| self.exact.get(*interface)),
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
