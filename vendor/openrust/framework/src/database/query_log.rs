use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use super::events::{QueryBindings, QueryRole};

/// A successful query captured by a connection's optional query log.
#[derive(Clone)]
pub struct QueryLogEntry {
    pub query: String,
    pub bindings: QueryBindings,
    pub time: Duration,
    pub read_write_type: QueryRole,
}

/// A logged query with its bindings rendered as SQL literals.
pub struct RawQueryLogEntry {
    pub raw_query: String,
    pub time: Duration,
}

#[derive(Default)]
pub(super) struct QueryLog {
    enabled: AtomicBool,
    entries: Mutex<Vec<QueryLogEntry>>,
}

impl QueryLog {
    pub(super) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub(super) fn set_enabled(&self, enabled: bool) {
        let _entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub(super) fn push(&self, entry: impl FnOnce() -> QueryLogEntry) {
        if self.enabled() {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if self.enabled() {
                entries.push(entry());
            }
        }
    }

    pub(super) fn snapshot(&self) -> Vec<QueryLogEntry> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}
