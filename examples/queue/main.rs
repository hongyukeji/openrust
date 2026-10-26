//! API、worker 与 scheduler 共用同一应用和数据库的队列示例。

use std::time::Duration;

use openrust::config::{QueueConfig, QueueDriverKind};
use openrust::core::{AppBuilder, BoxFuture, Plugin, Result};
use openrust::db::{DatabasePlugin, MigrationPlugin, sea_orm_migration::prelude::*};
use openrust::http::{AppBuilderHttpExt, HttpPlugin, Inject, Json};
use openrust::prelude::{Application, NoConfig};
use openrust::queue::{Job, JobContext, Queue, QueueMigration, QueuePlugin};
use serde::{Deserialize, Serialize};

struct Migrator;

#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(QueueMigration)]
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DemoJob {
    message: String,
}

impl Job for DemoJob {
    const NAME: &'static str = "demo.message.v1";

    fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            tracing::info!(message = %self.message, "处理队列消息");
            Ok(())
        })
    }
}

struct DemoQueuePlugin;

impl Plugin for DemoQueuePlugin {
    fn build(&self, app: &mut AppBuilder) -> Result<()> {
        let mut queue = QueuePlugin::default();
        queue.register_job::<DemoJob>()?;
        if app
            .section::<QueueConfig>("queue")
            .map_or(true, |config| config.driver == QueueDriverKind::Database)
        {
            queue.schedule_interval(
                "demo-heartbeat",
                "demo",
                DemoJob::NAME,
                &DemoJob {
                    message: "scheduler heartbeat".to_owned(),
                },
                Duration::from_secs(60),
            )?;
        }
        app.plugin(queue);
        Ok(())
    }
}

struct QueueDemo;

impl Application for QueueDemo {
    type Config = NoConfig;

    fn build(app: &mut AppBuilder) {
        app.plugin(HttpPlugin);
        if app
            .section::<QueueConfig>("queue")
            .map_or(true, |config| config.driver == QueueDriverKind::Database)
        {
            app.plugin(DatabasePlugin)
                .plugin(MigrationPlugin::<Migrator>::new());
        }
        app.plugin(DemoQueuePlugin)
            .routes(|routes| routes.post("/jobs", enqueue));
    }
}

async fn enqueue(Inject(queue): Inject<Queue>) -> Result<Json<String>> {
    let id = queue
        .dispatch(
            "demo",
            DemoJob::NAME,
            &DemoJob {
                message: "dispatched by API".to_owned(),
            },
        )
        .await?;
    Ok(Json(id))
}

fn main() -> std::process::ExitCode {
    openrust::run::<QueueDemo>()
}
