//! 后台任务。
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub mod example_heartbeat;

use openrust::core::{RegisterContext, Result, ServiceProvider};
use openrust::queue::{Job, QueueContributions};

/// 使用共享队列贡献目录登记示例任务与调度。
pub struct JobsServiceProvider(pub std::sync::Arc<QueueContributions>);

struct HeartbeatSequence(AtomicU64);

/// 为单次 heartbeat 任务尝试分配的关联号；重试会取得新号。
pub(crate) struct HeartbeatRun {
    pub(crate) id: u64,
}

impl ServiceProvider for JobsServiceProvider {
    fn register(&self, app: &mut RegisterContext<'_>) -> Result<()> {
        app.instance(Arc::new(HeartbeatSequence(AtomicU64::new(0))));
        app.scoped::<HeartbeatRun, _>(|resolver| {
            Box::pin(async move {
                let sequence = resolver.resolve::<HeartbeatSequence>().await?;
                let id = sequence.0.fetch_add(1, Ordering::Relaxed) + 1;
                resolver.resources()?.on_shutdown(move || {
                    Box::pin(async move {
                        tracing::info!(run_id = id, "example heartbeat attempt closed");
                        Ok(())
                    })
                })?;
                Ok(Arc::new(HeartbeatRun { id }))
            })
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use openrust::config::FrameworkConfig;
    use openrust::core::{App, Cancellation, ProviderBuilder};
    use openrust::queue::{JobContext, JobRegistry};

    #[tokio::test]
    async fn heartbeat_uses_a_new_scoped_run_for_each_attempt() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(JobsServiceProvider(Arc::new(QueueContributions::default())));
        let runtime = builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(FrameworkConfig::default()),
            ))
            .await?;
        let app = App::from_provider_runtime(runtime)?;
        let mut jobs = JobRegistry::default();
        jobs.register_job::<example_heartbeat::ExampleHeartbeatJob>()?;
        for attempt in 1..=2 {
            let context = JobContext::new("example-heartbeat", attempt, 2, Cancellation::new())?
                .with_app_state(app.state().clone());
            jobs.execute(
                example_heartbeat::ExampleHeartbeatJob::NAME,
                serde_json::json!(null),
                context,
            )
            .await?;
            assert_eq!(
                app.state()
                    .resolve::<HeartbeatSequence>()
                    .await?
                    .0
                    .load(Ordering::Relaxed),
                attempt
            );
        }
        app.shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await?;
        Ok(())
    }
}
