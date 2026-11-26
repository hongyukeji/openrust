use std::{
    future::Future,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use tokio::sync::{Mutex, OwnedMutexGuard};

use super::{
    Connection, DatabaseError, DatabaseResult,
    connection::{PhysicalConnection, TransactionCommand},
};

#[derive(Clone)]
struct Session {
    connection: Connection,
    physical: Arc<Mutex<Option<PhysicalConnection>>>,
}

impl Session {
    async fn start(connection: Connection) -> DatabaseResult<Self> {
        let mut physical = connection.acquire(false).await?;
        // Keep test-session settings out of the application's pooled connections.
        physical.retire_on_drop(true);
        physical
            .control("begin test transaction", TransactionCommand::Begin)
            .await?;
        Ok(Self {
            connection,
            physical: Arc::new(Mutex::new(Some(physical))),
        })
    }

    async fn rollback(self) -> DatabaseResult<()> {
        let mut physical = self
            .physical
            .try_lock()
            .map_err(|_| DatabaseError::UnsupportedOperation("test database session is in use"))?
            .take()
            .ok_or(DatabaseError::TransactionInterrupted)?;
        physical
            .control("roll back test transaction", TransactionCommand::RollBack)
            .await
    }
}

pub(super) fn same_session(owner: &Connection, connection: &Connection) -> bool {
    owner.same_query_source(connection)
        || (owner.name == connection.name
            && owner.model_container.ptr_eq(&connection.model_container))
}

tokio::task_local! {
    static SESSIONS: Vec<Session>;
}

fn session(connection: &Connection) -> Option<Session> {
    SESSIONS
        .try_with(|sessions| {
            sessions
                .iter()
                .rev()
                .find(|session| same_session(&session.connection, connection))
                .cloned()
        })
        .ok()
        .flatten()
}

pub(crate) fn active(connection: &Connection) -> bool {
    session(connection).is_some()
}

pub(super) fn checkout(connection: &Connection) -> DatabaseResult<Option<QueryPhysical>> {
    let Some(session) = session(connection) else {
        return Ok(None);
    };
    let physical = session
        .physical
        .try_lock_owned()
        .map_err(|_| DatabaseError::UnsupportedOperation("test database session is in use"))?;
    if physical.is_none() {
        return Err(DatabaseError::TransactionInterrupted);
    }
    Ok(Some(QueryPhysical::Scoped {
        physical,
        abandon: false,
    }))
}

pub(crate) fn validate(connections: &[Connection]) -> DatabaseResult<()> {
    for (index, connection) in connections.iter().enumerate() {
        if !connection.grammar.is_mysql()
            || active(connection)
            || connections[..index]
                .iter()
                .any(|other| same_session(connection, other))
        {
            return Err(DatabaseError::UnsupportedOperation(
                "test transactions require distinct unscoped MySQL connections",
            ));
        }
    }
    Ok(())
}

pub(crate) async fn run<T>(
    connections: Vec<Connection>,
    test: impl Future<Output = T>,
) -> DatabaseResult<T> {
    validate(&connections)?;
    let mut started = Vec::with_capacity(connections.len());
    for connection in connections {
        match Session::start(connection).await {
            Ok(session) => started.push(session),
            Err(error) => {
                let _ = rollback(started).await;
                return Err(error);
            }
        }
    }
    let mut sessions = SESSIONS.try_with(Clone::clone).unwrap_or_default();
    sessions.extend(started.iter().cloned());
    let result = SESSIONS.scope(sessions, test).await;
    rollback(started).await?;
    Ok(result)
}

async fn rollback(sessions: Vec<Session>) -> DatabaseResult<()> {
    let mut result = Ok(());
    for session in sessions.into_iter().rev() {
        let rollback = session.rollback().await;
        result = result.and(rollback);
    }
    result
}

pub(super) enum QueryPhysical {
    Pooled(PhysicalConnection),
    Scoped {
        physical: OwnedMutexGuard<Option<PhysicalConnection>>,
        abandon: bool,
    },
}

impl QueryPhysical {
    pub(super) fn begin_nested(&mut self) {
        if let Self::Scoped { abandon, .. } = self {
            *abandon = true;
        }
    }

    pub(super) fn end_nested(&mut self) {
        if let Self::Scoped { abandon, .. } = self {
            *abandon = false;
        }
    }

    pub(super) fn validate(&self, sql: &str, prepared: bool) -> DatabaseResult<()> {
        if matches!(self, Self::Scoped { .. }) {
            let verb = sql.split_ascii_whitespace().next().unwrap_or_default();
            let allowed = [
                "SELECT", "INSERT", "UPDATE", "DELETE", "REPLACE", "WITH", "SHOW", "EXPLAIN",
            ];
            if !prepared
                || !allowed
                    .iter()
                    .any(|allowed| verb.eq_ignore_ascii_case(allowed))
            {
                return Err(DatabaseError::UnsupportedOperation(
                    "test transaction requires a prepared transactional statement",
                ));
            }
        }
        Ok(())
    }
}

impl Deref for QueryPhysical {
    type Target = PhysicalConnection;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Pooled(physical) => physical,
            Self::Scoped { physical, .. } => physical.as_ref().expect("checked-out test session"),
        }
    }
}

impl DerefMut for QueryPhysical {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Pooled(physical) => physical,
            Self::Scoped { physical, .. } => physical.as_mut().expect("checked-out test session"),
        }
    }
}

impl Drop for QueryPhysical {
    fn drop(&mut self) {
        if let Self::Scoped {
            physical,
            abandon: true,
        } = self
        {
            physical.take();
        }
    }
}
