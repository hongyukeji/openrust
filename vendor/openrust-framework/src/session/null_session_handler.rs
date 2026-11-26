use std::time::Duration;

use crate::contracts::session::SessionHandler;

use super::{SessionFuture, SessionResult};

/// Accepts session operations without persisting data.
pub struct NullSessionHandler;

impl SessionHandler for NullSessionHandler {
    fn read<'a>(&'a mut self, _: &'a str) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>> {
        Box::pin(async { Ok(None) })
    }

    fn write<'a>(&'a mut self, _: &'a str, _: &'a [u8]) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async { Ok(()) })
    }

    fn destroy<'a>(&'a mut self, _: &'a str) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async { Ok(()) })
    }

    fn gc(&mut self, _: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>> {
        Box::pin(async { Ok(Some(0)) })
    }
}
