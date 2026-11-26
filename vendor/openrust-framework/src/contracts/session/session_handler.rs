use std::time::Duration;

use crate::session::{SessionFuture, SessionResult};

/// Stores encoded session payloads for a configured lifetime.
pub trait SessionHandler: Send + Sync {
    fn read<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>>;

    fn write<'a>(
        &'a mut self,
        id: &'a str,
        payload: &'a [u8],
    ) -> SessionFuture<'a, SessionResult<()>>;

    fn destroy<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<()>>;
    fn gc(&mut self, max_lifetime: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>>;
}
