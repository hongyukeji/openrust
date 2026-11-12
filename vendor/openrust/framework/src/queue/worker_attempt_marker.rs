use std::{sync::Arc, time::Duration};

use crate::{
    cache::{Repository, Ttl, Value, rate_limiting::counters},
    contracts::cache::{Lock, Repository as _},
};

use super::{QueueError, QueueResult, backend, new_id};

pub(super) struct AttemptMarker {
    cache: Arc<Repository>,
    key: String,
    owner: Value,
}

impl AttemptMarker {
    pub(super) async fn begin(cache: Arc<Repository>, uuid: &str) -> QueueResult<(Self, bool)> {
        let marker = Self {
            cache,
            key: counters::key("queue-job-processing", "", uuid),
            owner: Value::from(new_id()?.as_str()),
        };
        let mut lock = marker.lock().await?;
        let previous = marker.replace().await;
        let released = lock.release().await.map_err(backend);
        let previous = previous?;
        if !released? {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok((marker, previous))
    }

    async fn replace(&self) -> QueueResult<bool> {
        let previous = self.cache.get(&self.key).await.map_err(backend)?.is_some();
        if !self
            .cache
            .put(&self.key, &self.owner, Some(Ttl::Seconds(86_400)))
            .await
            .map_err(backend)?
        {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(previous)
    }

    pub(super) async fn finish(self) -> QueueResult<()> {
        let mut lock = self.lock().await?;
        let result: QueueResult<()> = async {
            if self.cache.get(&self.key).await.map_err(backend)? == Some(self.owner) {
                self.cache.forget(&self.key).await.map_err(backend)?;
            }
            Ok(())
        }
        .await;
        let released = lock.release().await.map_err(backend);
        result?;
        if !released? {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(())
    }

    async fn lock(&self) -> QueueResult<Box<dyn Lock>> {
        let provider = self
            .cache
            .get_store()
            .lock_provider()
            .ok_or(QueueError::InvalidConfiguration)?;
        let mut lock = provider
            .lock(
                &format!("{}:lock", self.key),
                Some(Duration::from_secs(15)),
                None,
            )
            .map_err(backend)?;
        lock.block(Duration::from_secs(5)).await.map_err(backend)?;
        Ok(lock)
    }
}
