use std::{sync::Arc, time::Duration};

use crate::{
    cache::{Ttl, Value},
    contracts::{cache::Repository, session::SessionHandler},
};

use super::{SessionError, SessionFuture, SessionResult, store::MAX_PAYLOAD_BYTES};

/// Stores sessions using a configured cache store.
#[derive(Clone)]
pub struct CacheBasedSessionHandler {
    cache: Arc<dyn Repository>,
    lifetime: Duration,
}

impl CacheBasedSessionHandler {
    pub fn new(cache: Arc<dyn Repository>, lifetime: Duration) -> Self {
        Self { cache, lifetime }
    }

    /// Returns the cache repository used for session payloads.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    pub fn getCache(&self) -> Arc<dyn Repository> {
        self.cache.clone()
    }
}

impl SessionHandler for CacheBasedSessionHandler {
    fn read<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<Option<Vec<u8>>>> {
        Box::pin(async move {
            match self
                .cache
                .get(id)
                .await
                .map_err(|_| SessionError::Storage)?
            {
                None => Ok(None),
                Some(Value::Bytes(bytes)) if bytes.len() <= MAX_PAYLOAD_BYTES => Ok(Some(bytes)),
                Some(Value::Bytes(_)) => Err(SessionError::PayloadTooLarge),
                Some(_) => Err(SessionError::Encoding),
            }
        })
    }

    fn write<'a>(
        &'a mut self,
        id: &'a str,
        payload: &'a [u8],
    ) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            if payload.len() > MAX_PAYLOAD_BYTES {
                return Err(SessionError::PayloadTooLarge);
            }
            if self.lifetime.is_zero() {
                return self.destroy(id).await;
            }
            let seconds =
                i64::try_from(self.lifetime.as_secs()).map_err(|_| SessionError::Configuration)?;
            let stored = self
                .cache
                .put(
                    id,
                    &Value::Bytes(payload.to_vec()),
                    Some(Ttl::Seconds(seconds)),
                )
                .await
                .map_err(|_| SessionError::Storage)?;
            if stored {
                Ok(())
            } else {
                Err(SessionError::Storage)
            }
        })
    }

    fn destroy<'a>(&'a mut self, id: &'a str) -> SessionFuture<'a, SessionResult<()>> {
        Box::pin(async move {
            self.cache
                .forget(id)
                .await
                .map_err(|_| SessionError::Storage)?;
            Ok(())
        })
    }

    fn gc(&mut self, _: Duration) -> SessionFuture<'_, SessionResult<Option<u64>>> {
        Box::pin(async { Ok(Some(0)) })
    }
}
