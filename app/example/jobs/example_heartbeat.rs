//! 可由数据库队列执行的应用任务。

use openrust::core::{BoxFuture, Result};
use openrust::queue::{Job, JobContext};
use serde::{Deserialize, Serialize};

/// `example_heartbeat` 后台任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleHeartbeatJob;

impl Job for ExampleHeartbeatJob {
    const NAME: &'static str = "example_heartbeat.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let run = context.service::<super::HeartbeatRun>().await?;
            tracing::info!(
                run_id = run.id,
                attempt = context.attempt(),
                "example heartbeat job processed"
            );
            Ok(())
        })
    }
}
