use std::{fmt::Write as _, time::Duration};

use crate::contracts::cache::Lock as LockContract;

use super::{
    CacheError, CacheFuture, CacheResult, Value, backend,
    engine::{Change, Engine, Record, expires},
};

/// Coordinates acquisition and ownership for an atomic cache lock.
pub struct Lock {
    engine: Engine,
    key: String,
    owner: String,
    lease: Option<Duration>,
    database: bool,
}

impl Lock {
    pub(super) fn new(
        engine: Engine,
        key: String,
        lease: Option<Duration>,
        owner: Option<&str>,
        database: bool,
    ) -> CacheResult<Self> {
        let owner = match owner {
            Some(owner) if !owner.is_empty() && owner.len() <= 255 => owner.to_owned(),
            Some(_) => return Err(CacheError::InvalidOwner),
            None => {
                let mut bytes = [0; 20];
                aws_lc_rs::rand::fill(&mut bytes)
                    .map_err(|_| backend(std::io::Error::other("lock owner generation failed")))?;
                let mut owner = String::with_capacity(40);
                for byte in bytes {
                    let _ = write!(owner, "{byte:02x}");
                }
                owner
            }
        };
        Ok(Self {
            engine,
            key,
            owner,
            lease,
            database,
        })
    }

    fn effective_lease(&self, lease: Option<Duration>) -> Option<Duration> {
        let lease = lease.or(self.lease).filter(|lease| !lease.is_zero());
        lease.or_else(|| self.database.then_some(Duration::from_secs(86_400)))
    }
}

impl LockContract for Lock {
    fn get(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        let lease = self.effective_lease(None);
        let owner = self.owner.clone();
        let database = self.database;
        Box::pin(self.engine.change(self.key.clone(), move |entry| {
            if entry.is_some_and(|entry| !database || entry.value.as_str() != Some(owner.as_str()))
            {
                return Ok((Change::Keep, false));
            }
            Ok((
                Change::Write(Record {
                    value: Value::from(owner.as_str()),
                    expires: expires(lease)?,
                }),
                true,
            ))
        }))
    }

    fn block(&mut self, wait: Duration) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(async move {
            let deadline = tokio::time::Instant::now()
                .checked_add(wait)
                .ok_or(CacheError::InvalidValue)?;
            loop {
                if self.get().await? {
                    return Ok(());
                }
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    return Err(CacheError::LockTimeout);
                }
                tokio::time::sleep(remaining.min(Duration::from_millis(250))).await;
                if tokio::time::Instant::now() >= deadline {
                    return Err(CacheError::LockTimeout);
                }
            }
        })
    }

    fn release(&mut self) -> CacheFuture<'_, CacheResult<bool>> {
        let owner = self.owner.clone();
        Box::pin(self.engine.change(self.key.clone(), move |entry| {
            if entry.is_some_and(|entry| entry.value.as_str() == Some(owner.as_str())) {
                Ok((Change::Remove, true))
            } else {
                Ok((Change::Keep, false))
            }
        }))
    }

    fn refresh(&mut self, lease: Option<Duration>) -> CacheFuture<'_, CacheResult<bool>> {
        let owner = self.owner.clone();
        let lease = self.effective_lease(lease);
        Box::pin(self.engine.change(self.key.clone(), move |entry| {
            if let Some(mut entry) = entry
                && entry.value.as_str() == Some(owner.as_str())
            {
                entry.expires = expires(lease)?;
                return Ok((Change::Write(entry), true));
            }
            Ok((Change::Keep, false))
        }))
    }

    fn force_release(&mut self) -> CacheFuture<'_, CacheResult<()>> {
        Box::pin(
            self.engine
                .change(self.key.clone(), |_| Ok((Change::Remove, ()))),
        )
    }

    fn owner(&self) -> &str {
        &self.owner
    }
}
