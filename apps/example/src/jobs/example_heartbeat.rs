//! 可由数据库队列执行的应用任务。

use openrust::core::{BoxFuture, Result};
use openrust::queue::{Job, JobContext};
use serde::{Deserialize, Serialize};

/// `example_heartbeat` 后台任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleHeartbeatJob;

impl Job for ExampleHeartbeatJob {
    const NAME: &'static str = "example_heartbeat.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            tracing::info!("example heartbeat job processed");
            Ok(())
        })
    }
}
