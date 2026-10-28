//! 后台任务。
use std::time::Duration;

pub mod example_heartbeat;

use openrust::core::{RegisterContext, Result, ServiceProvider};
use openrust::queue::{Job, QueueContributions};

/// 使用共享队列贡献目录登记示例任务与调度。
pub struct JobsServiceProvider(pub std::sync::Arc<QueueContributions>);

impl ServiceProvider for JobsServiceProvider {
    fn register(&self, _: &mut RegisterContext<'_>) -> Result<()> {
        self.0
            .register_job::<crate::app::example::jobs::example_heartbeat::ExampleHeartbeatJob>()?;
        self.0.schedule_interval(
            "example-heartbeat",
            "example",
            crate::app::example::jobs::example_heartbeat::ExampleHeartbeatJob::NAME,
            &crate::app::example::jobs::example_heartbeat::ExampleHeartbeatJob,
            Duration::from_secs(60),
        )
    }
}
