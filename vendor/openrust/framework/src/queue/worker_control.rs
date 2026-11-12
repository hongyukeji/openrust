use std::sync::Arc;

use crate::{
    cache::{Repository, Ttl, Value},
    contracts::cache::Repository as _,
};

use super::{QueueError, QueueResult, backend, new_id};

const RESTART: &str = "openrust:queue:restart";
const PAUSE_ALL: &str = "openrust:queues:paused";

pub(super) struct WorkerControl(Arc<Repository>);

impl WorkerControl {
    pub(super) fn new(cache: Arc<Repository>) -> Self {
        Self(cache)
    }

    pub(super) async fn restart_generation(&self) -> QueueResult<Option<Value>> {
        self.0.get(RESTART).await.map_err(backend)
    }

    pub(super) async fn restart(&self) -> QueueResult<()> {
        if !self
            .0
            .forever(RESTART, &Value::from(new_id()?.as_str()))
            .await
            .map_err(backend)?
        {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(())
    }

    pub(super) async fn pause(
        &self,
        target: Option<(&str, &str)>,
        ttl: Option<Ttl>,
    ) -> QueueResult<()> {
        let key = target.map_or_else(|| PAUSE_ALL.to_owned(), pause_key);
        let paused = Value::Json(serde_json::Value::Bool(true));
        let stored = match ttl {
            Some(ttl) => self.0.put(&key, &paused, Some(ttl)).await,
            None => self.0.forever(&key, &paused).await,
        };
        if !stored.map_err(backend)? {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(())
    }

    pub(super) async fn resume(&self, target: Option<(&str, &str)>) -> QueueResult<()> {
        let key = target.map_or_else(|| PAUSE_ALL.to_owned(), pause_key);
        self.0.forget(&key).await.map_err(backend)?;
        Ok(())
    }

    pub(super) async fn all_paused(&self) -> QueueResult<bool> {
        self.paused(PAUSE_ALL).await
    }

    pub(super) async fn queue_paused(&self, connection: &str, queue: &str) -> QueueResult<bool> {
        self.paused(&pause_key((connection, queue))).await
    }

    pub(super) async fn paused_queues<'a>(
        &self,
        connection: &str,
        queues: &[&'a str],
    ) -> QueueResult<Vec<&'a str>> {
        if self.all_paused().await? {
            return Ok(queues.to_vec());
        }
        let mut paused = Vec::new();
        for &queue in queues {
            if self.queue_paused(connection, queue).await? {
                paused.push(queue);
            }
        }
        Ok(paused)
    }

    async fn paused(&self, key: &str) -> QueueResult<bool> {
        Ok(self.0.get(key).await.map_err(backend)?
            == Some(Value::Json(serde_json::Value::Bool(true))))
    }
}

fn pause_key((connection, queue): (&str, &str)) -> String {
    format!("openrust:queue:paused:{connection}:{queue}")
}
