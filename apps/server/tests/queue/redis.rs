//! 验证应用可显式选择 Redis 队列，且不需要数据库服务。
#![cfg(feature = "queue-redis")]

use figment::providers::Format;
use openrust::{
    BoxFuture, Error, Result,
    config::FrameworkConfig,
    core::{AppBuilder, lifecycle::finalize},
    queue::{Job, JobContext, Queue, QueuePlugin},
};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
struct ProbeJob;

impl Job for ProbeJob {
    const NAME: &'static str = "tests.server.redis.probe.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

fn namespace() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("server-{}", nanos % 1_000_000_000_000_000)
}

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_owned())
}

fn builder(namespace: &str) -> Result<AppBuilder> {
    let url = redis_url();
    let figment = figment::Figment::from(figment::providers::Toml::string(&format!(
        "[queue]\ndriver = 'redis'\nredis_url = {url:?}\nredis_namespace = '{namespace}'\n"
    )));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    let mut plugin = QueuePlugin::default();
    plugin.register_job::<ProbeJob>()?;
    builder.plugin(plugin);
    Ok(builder)
}

#[tokio::test]
async fn queue_plugin_selects_redis_without_a_database_plugin() -> Result<()> {
    let namespace = namespace();
    let app = finalize(builder(&namespace)?).await?;
    let queue = app.state().try_get::<Queue>()?;
    queue.dispatch("default", ProbeJob::NAME, &()).await?;
    assert!(queue.run_next("default").await?);
    assert!(!queue.run_next("default").await?);

    let client = redis::Client::open(redis_url())
        .map_err(|_| Error::custom("test_redis_url_invalid", "Redis test URL is invalid"))?;
    let mut connection = client
        .get_multiplexed_async_connection()
        .await
        .map_err(Error::internal)?;
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg(format!("{{openrust:{namespace}}}:*"))
        .query_async(&mut connection)
        .await
        .map_err(Error::internal)?;
    if !keys.is_empty() {
        let _: usize = redis::cmd("DEL")
            .arg(keys)
            .query_async(&mut connection)
            .await
            .map_err(Error::internal)?;
    }
    Ok(())
}

#[tokio::test]
async fn redis_queue_rejects_database_transactional_schedules_at_build() -> Result<()> {
    let namespace = namespace();
    let url = redis_url();
    let figment = figment::Figment::from(figment::providers::Toml::string(&format!(
        "[queue]\ndriver = 'redis'\nredis_url = {url:?}\nredis_namespace = '{namespace}'\n"
    )));
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment);
    let mut plugin = QueuePlugin::default();
    plugin.register_job::<ProbeJob>()?;
    plugin.schedule_interval(
        "probe",
        "default",
        ProbeJob::NAME,
        &(),
        std::time::Duration::from_secs(60),
    )?;
    builder.plugin(plugin);
    let error = finalize(builder).await.err().ok_or_else(|| {
        Error::custom("test_expected_error", "Redis scheduler unexpectedly built")
    })?;
    assert_eq!(error.code(), "redis_queue_schedule_unsupported");
    Ok(())
}
