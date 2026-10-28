//! 后台任务。
use std::time::Duration;

pub mod example_heartbeat;

use openrust::core::{AppBuilder, Plugin, Result};
use openrust::queue::{Job, QueuePlugin};

/// 注册应用的后台任务。
pub struct JobsPlugin;

impl Plugin for JobsPlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let mut queue = QueuePlugin::default();
        queue.register_job::<crate::jobs::example_heartbeat::ExampleHeartbeatJob>()?;
        queue.schedule_interval(
            "example-heartbeat",
            "example",
            crate::jobs::example_heartbeat::ExampleHeartbeatJob::NAME,
            &crate::jobs::example_heartbeat::ExampleHeartbeatJob,
            Duration::from_secs(60),
        )?;
        app.plugin(queue);
        Ok(())
    }
}
