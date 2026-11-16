//! Events emitted by database connections and transactions.

use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::{
    container::Container,
    contracts::events::{Dispatcher as _, Event, Listener},
    events::{Dispatcher, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    support::facades::application_scope,
};

use super::{
    Bindings, Connection, DatabaseError, DatabaseResult, QueryLogEntry, Value,
    bindings::render_query,
};

/// An application connection has been resolved for use.
pub struct ConnectionEstablished {
    pub connection: Connection,
}

impl ConnectionEstablished {
    pub const NAME: &'static str = r"OpenRust\Database\Events\ConnectionEstablished";
}

impl Event for ConnectionEstablished {
    fn name(&self) -> &str {
        Self::NAME
    }
}

/// The pool selected for an executed query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryRole {
    Read,
    Write,
}

/// The original bindings supplied by the caller.
#[derive(Clone)]
pub enum QueryBindings {
    Positional(Vec<Value>),
    Named(Vec<(String, Value)>),
}

impl From<Bindings<'_>> for QueryBindings {
    fn from(bindings: Bindings<'_>) -> Self {
        match bindings {
            Bindings::Positional(values) => Self::Positional(values.to_vec()),
            Bindings::Named(values) => Self::Named(
                values
                    .iter()
                    .map(|(name, value)| ((*name).into(), value.clone()))
                    .collect(),
            ),
        }
    }
}

/// A successful database query and its execution duration.
pub struct QueryExecuted {
    pub connection: Connection,
    pub sql: String,
    pub bindings: QueryBindings,
    pub time: Duration,
    pub read_write_type: QueryRole,
}

impl QueryExecuted {
    pub const NAME: &'static str = r"OpenRust\Database\Events\QueryExecuted";

    /// Renders a diagnostic SQL string with the original bindings embedded.
    #[allow(non_snake_case, reason = "Matches Laravel's QueryExecuted API")]
    pub fn toRawSql(&self) -> DatabaseResult<String> {
        render_query(&self.sql, &self.bindings, !self.connection.is_sqlite())
    }
}

impl Event for QueryExecuted {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub(super) struct QueryNotification {
    pub owner: Arc<Container>,
    pub dispatcher: Arc<Dispatcher>,
    pub event: Arc<QueryExecuted>,
}

impl QueryNotification {
    pub async fn dispatch(self, context: &mut EventContext<'_>) -> DatabaseResult<()> {
        application_scope(
            self.owner,
            self.dispatcher.dispatch(self.event.clone(), context),
        )
        .await
        .map_err(|source| DatabaseError::ConnectionEvent(Box::new(source)))?;
        self.event.connection.query_log.push(|| QueryLogEntry {
            query: self.event.sql.clone(),
            bindings: self.event.bindings.clone(),
            time: self.event.time,
            read_write_type: self.event.read_write_type,
        });
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct QueryDuration {
    nanos: AtomicU64,
    generation: AtomicU64,
}

impl QueryDuration {
    pub(super) fn total(&self) -> Duration {
        Duration::from_nanos(self.nanos.load(Ordering::Relaxed))
    }

    pub(super) fn add(&self, duration: Duration) {
        let nanos = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        let _ = self
            .nanos
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                Some(total.saturating_add(nanos))
            });
    }

    pub(super) fn reset(&self) {
        self.nanos.store(0, Ordering::Relaxed);
    }

    pub(super) fn rearm(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) struct QueryDurationHandler {
    connection: Weak<QueryDuration>,
    threshold: Duration,
    listener: Arc<dyn Listener>,
    claimed: Mutex<Option<u64>>,
}

impl QueryDurationHandler {
    pub(super) fn new(
        connection: &Arc<QueryDuration>,
        threshold: Duration,
        listener: Arc<dyn Listener>,
    ) -> Self {
        Self {
            connection: Arc::downgrade(connection),
            threshold,
            listener,
            claimed: Mutex::default(),
        }
    }
}

struct DurationClaim<'a> {
    claimed: &'a Mutex<Option<u64>>,
    generation: u64,
    completed: bool,
}

impl Drop for DurationClaim<'_> {
    fn drop(&mut self) {
        if !self.completed {
            let mut claimed = self
                .claimed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *claimed == Some(self.generation) {
                *claimed = None;
            }
        }
    }
}

impl Listener for QueryDurationHandler {
    fn handle<'a>(
        &'a self,
        payload: &'a dyn Event,
        context: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = payload
                .downcast_ref::<QueryExecuted>()
                .ok_or(EventError::InvalidPayload)?;
            let duration = &event.connection.query_duration;
            if !std::ptr::eq(self.connection.as_ptr(), Arc::as_ptr(duration))
                || duration.total() <= self.threshold
            {
                return Ok(None);
            }
            let generation = duration.generation.load(Ordering::Relaxed);
            {
                let mut claimed = self
                    .claimed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if claimed.is_some_and(|claimed| claimed >= generation) {
                    return Ok(None);
                }
                *claimed = Some(generation);
            }
            let mut claim = DurationClaim {
                claimed: &self.claimed,
                generation,
                completed: false,
            };
            let result = self.listener.handle(payload, context).await;
            claim.completed = result.is_ok();
            result
        })
    }
}

pub(super) trait TransactionEvent: Event {
    const NAME: &'static str;

    fn new(connection: Connection, level: u32) -> Self;
}

macro_rules! transaction_event {
    ($event:ident, $description:literal) => {
        #[doc = $description]
        pub struct $event {
            pub connection: Connection,
            pub level: u32,
        }

        impl $event {
            pub const NAME: &'static str =
                concat!(r"OpenRust\Database\Events\", stringify!($event));
        }

        impl Event for $event {
            fn name(&self) -> &str {
                Self::NAME
            }
        }

        impl TransactionEvent for $event {
            const NAME: &'static str = Self::NAME;

            fn new(connection: Connection, level: u32) -> Self {
                Self { connection, level }
            }
        }
    };
}

transaction_event!(
    TransactionBeginning,
    "A transaction or savepoint has started."
);
transaction_event!(
    TransactionCommitting,
    "The outer transaction is about to commit."
);
transaction_event!(TransactionCommitted, "The transaction level has committed.");
transaction_event!(
    TransactionRolledBack,
    "A rollback has restored the indicated transaction level."
);
