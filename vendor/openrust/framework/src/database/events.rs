//! Events emitted by database connections and transactions.

use std::{
    cell::RefCell,
    panic::Location,
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

pub struct ModelPruningStarting {
    pub models: Vec<&'static str>,
}

impl ModelPruningStarting {
    pub const NAME: &'static str = r"OpenRust\Database\Events\ModelPruningStarting";
}

impl Event for ModelPruningStarting {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct ModelsPruned {
    pub model: &'static str,
    pub count: u64,
}

impl ModelsPruned {
    pub const NAME: &'static str = r"OpenRust\Database\Events\ModelsPruned";
}

impl Event for ModelsPruned {
    fn name(&self) -> &str {
        Self::NAME
    }
}

pub struct ModelPruningFinished {
    pub models: Vec<&'static str>,
}

impl ModelPruningFinished {
    pub const NAME: &'static str = r"OpenRust\Database\Events\ModelPruningFinished";
}

impl Event for ModelPruningFinished {
    fn name(&self) -> &str {
        Self::NAME
    }
}

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
    Direct,
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
    /// 已捕获的查询入口调用位置，最近的入口在前。
    pub callers: Vec<&'static Location<'static>>,
}

thread_local! {
    static QUERY_CALLERS: RefCell<Vec<&'static Location<'static>>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn query_callers() -> Vec<&'static Location<'static>> {
    QUERY_CALLERS.with_borrow(|callers| callers.iter().rev().copied().collect())
}

fn poll_query_callers<T>(
    callers: impl IntoIterator<Item = &'static Location<'static>>,
    poll: impl FnOnce() -> T,
) -> T {
    struct RestoreCaller(usize);
    impl Drop for RestoreCaller {
        fn drop(&mut self) {
            QUERY_CALLERS.with_borrow_mut(|callers| {
                callers.truncate(self.0);
            });
        }
    }
    // 只在 poll 期间登记，挂起、跨线程恢复和取消都不会留下位置。
    let depth = QUERY_CALLERS.with_borrow_mut(|stack| {
        let depth = stack.len();
        stack.extend(callers);
        depth
    });
    let _restore = RestoreCaller(depth);
    poll()
}

pub(in crate::database) fn with_query_caller<F: Future>(
    caller: &'static Location<'static>,
    future: F,
) -> impl Future<Output = F::Output> {
    let mut future = Box::pin(future);
    std::future::poll_fn(move |context| {
        poll_query_callers([caller], || future.as_mut().poll(context))
    })
}

/// 将 SQLite 重建任务的查询位置和仍有效的应用作用域传给新任务。
pub(super) fn with_inherited_query_scope<F: Future>(future: F) -> impl Future<Output = F::Output> {
    let callers = QUERY_CALLERS.with_borrow(Clone::clone);
    let facade = crate::support::facades::context_snapshot();
    let scope = crate::container::ScopeSnapshot::capture();
    let mut future = Box::pin(future);
    std::future::poll_fn(move |context| {
        poll_query_callers(callers.iter().copied(), || {
            if facade.is_closed() {
                // 请求取消后仍须完成重建与会话恢复。
                future.as_mut().poll(context)
            } else {
                facade.run(|| scope.run(|| future.as_mut().poll(context)))
            }
        })
    })
}

pub(in crate::database) fn stream_with_query_caller<S: futures_util::Stream>(
    caller: &'static Location<'static>,
    stream: S,
) -> impl futures_util::Stream<Item = S::Item> {
    let mut stream = Box::pin(stream);
    futures_util::stream::poll_fn(move |context| {
        poll_query_callers([caller], || stream.as_mut().poll_next(context))
    })
}

impl QueryExecuted {
    pub const NAME: &'static str = r"OpenRust\Database\Events\QueryExecuted";

    /// Renders a diagnostic SQL string with the original bindings embedded.
    pub fn to_raw_sql(&self) -> DatabaseResult<String> {
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
        self.event.connection.record_query(|| QueryLogEntry {
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
        let mut total = self.nanos.load(Ordering::Relaxed);
        loop {
            match self.nanos.compare_exchange_weak(
                total,
                total.saturating_add(nanos),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => total = current,
            }
        }
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

/// The direction of a migration operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationMethod {
    Up,
    Down,
}

/// The options passed to one migration batch.
#[derive(Clone, Copy, Debug)]
pub enum MigrationOptions {
    Up(super::migrations::RunOptions),
    Down(super::migrations::RollbackOptions),
}

macro_rules! database_event {
    ($(#[$meta:meta])* $name:ident { $($field:ident: $ty:ty),+ } $(, $contract:expr)?) => {
        $(#[$meta])*
        pub struct $name {
            $(pub $field: $ty),+
        }

        impl $name {
            pub const NAME: &'static str = concat!("OpenRust\\Database\\Events\\", stringify!($name));
        }

        impl Event for $name {
            fn name(&self) -> &str {
                Self::NAME
            }

            $(
                fn interfaces(&self) -> &[&str] {
                    &[$contract]
                }
            )?
        }
    };
}

use super::migrations::Migration;
use crate::contracts::database::events::MigrationEvent;

database_event!(
    /// A nonempty batch is about to run.
    MigrationsStarted { method: MigrationMethod, options: MigrationOptions },
    MigrationEvent::NAME
);
database_event!(
    /// All migrations in the batch have been processed.
    MigrationsEnded { method: MigrationMethod, options: MigrationOptions },
    MigrationEvent::NAME
);
database_event!(
    /// A compiled migration is about to execute.
    MigrationStarted { migration: &'static dyn Migration, method: MigrationMethod, name: String },
    MigrationEvent::NAME
);
database_event!(
    /// A migration method succeeded, before its ledger is updated.
    MigrationEnded { migration: &'static dyn Migration, method: MigrationMethod, name: String },
    MigrationEvent::NAME
);
database_event!(
    /// The command found no pending migrations in this direction.
    NoPendingMigrations { method: MigrationMethod },
    MigrationEvent::NAME
);
database_event!(
    /// 迁移通过 `should_run` 钩子选择不执行。
        MigrationSkipped { migration_name: String },
    MigrationEvent::NAME
);
database_event!(
    /// A schema snapshot was persisted at the given path.
        SchemaDumped { connection: Connection, connection_name: String, path: std::path::PathBuf }
);
database_event!(
    /// A schema snapshot was loaded before remaining migrations.
        SchemaLoaded { connection: Connection, connection_name: String, path: std::path::PathBuf }
);
database_event!(
    /// Application migration sources were pruned after a dump.
        MigrationsPruned { connection: Connection, connection_name: String, path: std::path::PathBuf }
);
database_event!(
    /// A fresh or refreshed schema is ready, before optional seeding.
    DatabaseRefreshed { database: Option<String>, seeding: bool },
    MigrationEvent::NAME
);
