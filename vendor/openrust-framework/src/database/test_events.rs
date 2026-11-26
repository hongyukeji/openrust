use std::future::Future;

use super::{Connection, test_transaction::same_session};

tokio::task_local! {
    static MUTED_CONNECTIONS: Vec<Connection>;
}

pub(crate) async fn without_connections<T>(
    connections: Vec<Connection>,
    work: impl Future<Output = T>,
) -> T {
    if connections.is_empty() {
        return work.await;
    }
    MUTED_CONNECTIONS.scope(connections, work).await
}

pub(super) fn muted(connection: &Connection) -> bool {
    MUTED_CONNECTIONS
        .try_with(|connections| {
            connections
                .iter()
                .any(|selected| same_session(selected, connection))
        })
        .unwrap_or(false)
}
