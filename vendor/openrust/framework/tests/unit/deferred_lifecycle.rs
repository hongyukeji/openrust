#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::{
    container::Container,
    contracts::{
        container::Container as _,
        events::{Dispatcher as _, Event, Listener},
        foundation::Application as _,
        queue::JobHandler,
    },
    database::DatabaseManager,
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::Application,
    queue::{
        JobContext, Payload, QueueError, QueueFuture, QueueManager, QueueResult, WorkResult,
        Worker, WorkerOptions, events::JobAttempted,
    },
};
use serde_json::json;
use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
struct Buffer {
    value: AtomicUsize,
}
struct Probe {
    owner: Weak<Container>,
    seen: Arc<Mutex<Vec<(usize, bool)>>>,
    attempted: Arc<AtomicUsize>,
}
impl JobHandler for Probe {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let owner = self.owner.upgrade().unwrap();
            let buffer = owner.make::<Buffer>().unwrap();
            assert_eq!(
                buffer.value.load(Ordering::SeqCst),
                0,
                "each attempt needs a fresh scoped buffer"
            );
            let kind = payload.data().as_u64().unwrap() as usize;
            for always in [false, true] {
                let buffer = buffer.clone();
                let seen = self.seen.clone();
                let attempted = self.attempted.clone();
                let _ = super::defer(
                    async move {
                        assert_eq!(
                            attempted.load(Ordering::SeqCst),
                            kind + 1,
                            "callbacks follow JobAttempted"
                        );
                        seen.lock()
                            .unwrap()
                            .push((buffer.value.load(Ordering::SeqCst), always));
                        Ok::<_, std::io::Error>(())
                    },
                    None,
                    always,
                )
                .unwrap();
            }
            buffer.value.store(kind + 1, Ordering::SeqCst);
            match kind {
                1 => Err(QueueError::InvalidPayload),
                2 => {
                    context.release(Duration::from_secs(3600));
                    Ok(())
                }
                3 => std::future::pending().await,
                _ => Ok(()),
            }
        })
    }
}
struct Attempted(Arc<AtomicUsize>);
impl Listener for Attempted {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        })
    }
}
#[tokio::test]
async fn worker_flushes_each_attempt_after_event_with_original_scope_and_failure_policy() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("jobs.sqlite"), []).unwrap();
    let mut app = Application::configure(root.path().to_path_buf())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.config().unwrap();
    let database = match std::env::var("OPENRUST_DEFER_DATABASE_DRIVER").as_deref() {
        Ok(driver @ ("mysql" | "pgsql")) => {
            json!({"driver":driver,"url":std::env::var("OPENRUST_DEFER_DATABASE_URL").expect("explicit database URL required")})
        }
        Ok(other) => panic!("unsupported test driver {other}"),
        Err(_) => json!({"driver":"sqlite","database":root.path().join("jobs.sqlite")}),
    };
    config.set("database.default", json!("defer")).unwrap();
    config.set("database.connections.defer", database).unwrap();
    config.set("queue.default", json!("database")).unwrap();
    config
        .set(
            "queue.connections.database",
            json!({"driver":"database","table":"defer_jobs","retry_after":60}),
        )
        .unwrap();
    config.set("queue.failed.driver", json!("null")).unwrap();
    let db = app
        .container()
        .make::<DatabaseManager>()
        .unwrap()
        .connection(None)
        .unwrap();
    db.getSchemaBuilder()
        .create("defer_jobs", |table| {
            table.id();
            table.string("queue", 255);
            table.longText("payload");
            table.unsignedInteger("attempts");
            table.unsignedBigInteger("reserved_at").nullable();
            table.unsignedBigInteger("available_at");
            table.unsignedBigInteger("created_at");
        })
        .await
        .unwrap();
    app.container()
        .scoped::<Buffer, _>(|_| Ok(Arc::new(Buffer::default())))
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let attempted = Arc::new(AtomicUsize::new(0));
    let manager = app.container().make::<QueueManager>().unwrap();
    manager
        .register(
            "defer:probe",
            Arc::new(Probe {
                owner: Arc::downgrade(&app.container_handle()),
                seen: seen.clone(),
                attempted: attempted.clone(),
            }),
        )
        .unwrap();
    app.container()
        .make::<Dispatcher>()
        .unwrap()
        .listen(
            JobAttempted::NAME,
            Arc::new(Attempted(attempted.clone())),
            Delivery::Sync,
        )
        .unwrap();
    let connection = manager.connection(None).unwrap();
    let worker = Worker::new(app.container_handle()).unwrap();
    for (kind, outcome) in [
        (0, WorkResult::Processed),
        (1, WorkResult::Failed),
        (2, WorkResult::Released),
        (3, WorkResult::Failed),
    ] {
        connection
            .push(&Payload::new("defer:probe", json!(kind)).unwrap(), None)
            .await
            .unwrap();
        let result = worker
            .runNextJob(
                connection.as_ref(),
                None,
                &WorkerOptions {
                    timeout: Duration::from_millis(50),
                    kill_on_timeout: false,
                    ..WorkerOptions::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(result, outcome);
        assert_eq!(attempted.load(Ordering::SeqCst), kind + 1);
    }
    assert_eq!(
        *seen.lock().unwrap(),
        [
            (1, false),
            (1, true),
            (2, true),
            (3, false),
            (3, true),
            (4, true)
        ]
    );
    db.getSchemaBuilder().drop("defer_jobs").await.unwrap();
}
