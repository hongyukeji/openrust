use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use super::events::{QueryBindings, QueryRole};

/// A query captured by a connection log or a preview.
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

#[derive(Clone)]
pub(super) struct PreviewScope {
    pub(super) connection: usize,
    pub(super) dry: bool,
    pub(super) log: Arc<QueryLog>,
}

tokio::task_local! {
    static QUERY_PREVIEWS: Vec<PreviewScope>;
}

impl PreviewScope {
    pub(super) fn current(connection: usize) -> Option<Self> {
        QUERY_PREVIEWS
            .try_with(|scopes| {
                scopes
                    .iter()
                    .rev()
                    .find(|scope| scope.connection == connection)
                    .cloned()
            })
            .ok()
            .flatten()
    }

    pub(super) async fn run<T>(self, work: impl Future<Output = T>) -> T {
        let mut scopes = QUERY_PREVIEWS.try_with(Clone::clone).unwrap_or_default();
        scopes.push(self);
        QUERY_PREVIEWS.scope(scopes, work).await
    }
}
