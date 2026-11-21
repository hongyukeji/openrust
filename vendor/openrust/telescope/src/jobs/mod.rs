use crate::EntryUpdate;
use openrust::{
    contracts::queue::JobHandler,
    queue::{DispatchOptions, JobContext, Payload, QueueError, QueueFuture, QueueResult},
    support::facades::{Config, Facade},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct ProcessPendingUpdates {
    pub pendingUpdates: Vec<EntryUpdate>,
    pub attempt: u32,
}
impl ProcessPendingUpdates {
    pub const NAME: &str = "openrust_telescope::jobs::ProcessPendingUpdates";
    pub fn new(pendingUpdates: Vec<EntryUpdate>, attempt: u32) -> Self {
        Self {
            pendingUpdates,
            attempt,
        }
    }
    pub async fn dispatch(self) -> QueueResult<()> {
        let name = |key| -> QueueResult<Option<String>> {
            Ok(Config::get(key)
                .map_err(QueueError::Unavailable)?
                .and_then(|value| value.as_str().map(str::to_owned)))
        };
        let delay = Config::get("telescope.queue.delay")
            .map_err(QueueError::Unavailable)?
            .and_then(|value| value.as_u64())
            .filter(|value| *value > 0)
            .map(|value| std::time::Duration::from_secs(value).into());
        openrust::queue::QueueFacade::dispatch(
            Payload::new(Self::NAME, &self)?,
            DispatchOptions {
                connection: name("telescope.queue.connection")?,
                queue: name("telescope.queue.queue")?,
                delay,
                ..Default::default()
            },
            None,
        )
        .await?;
        Ok(())
    }
}
pub(crate) struct Handler;
impl JobHandler for Handler {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let mut pending: ProcessPendingUpdates = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            pending.attempt = pending
                .attempt
                .checked_add(1)
                .ok_or(QueueError::InvalidPayload)?;
            let telescope =
                crate::facades::Telescope::getFacadeRoot().map_err(QueueError::Unavailable)?;
            let failed = telescope
                .withoutRecording(telescope.repository.update(&pending.pendingUpdates))
                .await
                .map_err(|source| QueueError::Execution {
                    source: Box::new(source),
                })?;
            if !failed.is_empty() && pending.attempt < 3 {
                ProcessPendingUpdates::new(failed, pending.attempt)
                    .dispatch()
                    .await?;
            }
            Ok(())
        })
    }
}
