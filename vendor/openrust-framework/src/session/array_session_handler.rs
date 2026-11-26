use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::contracts::session::SessionHandler;

use super::{SessionError, SessionFuture, SessionResult};

type Records = Arc<Mutex<HashMap<String, (Instant, Vec<u8>)>>>;

/// Stores session bytes in the application's memory.
#[derive(Clone)]
pub struct ArraySessionHandler {
    records: Records,
    lifetime: Duration,
}

impl ArraySessionHandler {
    pub fn new(lifetime: Duration) -> Self {
        Self {
            records: Records::default(),
            lifetime,
        }
    }
}

impl SessionHandler for ArraySessionHandler {
    fn read<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>> {
        Box::pin(async move {
            let records = self.records.lock().map_err(|_| SessionError::Storage)?;
            Ok(records
                .get(id)
                .filter(|(written, _)| written.elapsed() <= self.lifetime)
                .map(|(_, payload)| payload.clone()))
        })
    }

    fn write<'a>(
        &'a mut self,
        id: &'a str,
        payload: &'a [u8],
    ) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            if payload.len() > super::store::MAX_PAYLOAD_BYTES {
                return Err(SessionError::PayloadTooLarge);
            }
            let mut records = self.records.lock().map_err(|_| SessionError::Storage)?;
            records.insert(id.to_owned(), (Instant::now(), payload.to_vec()));
            Ok(())
        })
    }

    fn destroy<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            self.records
                .lock()
                .map_err(|_| SessionError::Storage)?
                .remove(id);
            Ok(())
        })
    }

    fn gc(&mut self, lifetime: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>> {
        Box::pin(async move {
            let mut records = self.records.lock().map_err(|_| SessionError::Storage)?;
            let previous = records.len();
            records.retain(|_, (written, _)| written.elapsed() < lifetime);
            Ok(Some((previous - records.len()) as u64))
        })
    }
}
