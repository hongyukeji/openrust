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

tokio::task_local! {
    static SESSIONS: Vec<Session>;
}

fn session(connection: &Connection) -> Option<Session> {
    SESSIONS
        .try_with(|sessions| {
            sessions
                .iter()
                .rev()
                .find(|session| {
                    let owner = &session.connection;
                    owner.same_query_source(connection)
                        || (owner.name == connection.name
                            && owner.model_container.ptr_eq(&connection.model_container))
                })
                .cloned()
        })
        .ok()
        .flatten()
}

pub(super) fn active(connection: &Connection) -> bool {
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

pub(crate) async fn run<T>(
    connection: Connection,
    test: impl Future<Output = T>,
) -> DatabaseResult<T> {
    if !connection.grammar.is_mysql() || active(&connection) {
        return Err(DatabaseError::UnsupportedOperation(
            "a test transaction requires an unscoped MySQL connection",
        ));
    }
    let mut physical = connection.acquire(false).await?;
    // Keep test-session settings out of the application's pooled connections.
    physical.retire_on_drop(true);
    physical
        .control("begin test transaction", TransactionCommand::Begin)
        .await?;
    let physical = Arc::new(Mutex::new(Some(physical)));
    let mut sessions = SESSIONS.try_with(Clone::clone).unwrap_or_default();
    sessions.push(Session {
        connection,
        physical: physical.clone(),
    });
    let result = SESSIONS.scope(sessions, test).await;
    let mut physical = physical
        .try_lock()
        .map_err(|_| DatabaseError::UnsupportedOperation("test database session is in use"))?
        .take()
        .ok_or(DatabaseError::TransactionInterrupted)?;
    physical
        .control("roll back test transaction", TransactionCommand::RollBack)
        .await?;
    Ok(result)
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
