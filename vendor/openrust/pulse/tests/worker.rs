use openrust::{
    contracts::events::{Dispatcher as _, Event, Listener},
    contracts::queue::JobHandler,
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
    foundation::Application,
    http::HttpFuture,
    queue::events::{JobFailed, JobProcessing, JobTimedOut},
    queue::{JobContext, JobOptions, Payload, QueueError, QueueFuture, QueueManager, QueueResult},
    support::{ProviderResult, ServiceProvider},
    testing::TestApp,
};
use openrust_pulse::{PulseServiceProvider, facades::Pulse, storage::DatabaseStorage};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

struct Job(Arc<Mutex<Vec<Value>>>);
impl JobHandler for Job {
    fn handle<'a>(
        &'a self,
        context: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            if payload.data()["timeout"] == true
                || (payload.data()["timeout_once"] == true && context.attempts() == 1)
            {
                return std::future::pending().await;
            }
            Pulse::record("worker_custom", payload.uuid(), Some(1), None)
                .await
                .map_err(execution)?
                .count();
            Pulse::report("WorkerException", Some("jobs/test.rs:1".into()))
                .await
                .map_err(execution)?;
            let fails = payload.data()["fail"] == true
                || (payload.data()["retry"] == true && context.attempts() == 1);
            self.0.lock().unwrap().push(json!(fails));
            if fails {
                return Err(execution(std::io::Error::other("expected failure")));
            }
            Ok(())
        })
    }
}
fn execution(source: impl std::error::Error + Send + Sync + 'static) -> QueueError {
    QueueError::Execution {
        source: std::sync::Arc::new(source),
    }
}
struct Register {
    values: Arc<Mutex<Vec<Value>>>,
}
impl ServiceProvider for Register {
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.make::<QueueManager>()
                .await?
                .register("pulse-scope", Arc::new(Job(self.values.clone())))?;
            Ok(())
        })
    }
}

struct JobPreflight;

impl Listener for JobPreflight {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            Ok(None)
        })
    }
}

async fn worker_application(
    values: Arc<Mutex<Vec<Value>>>,
) -> (TestApp, openrust::database::Connection, &'static str) {
    let driver = worker_queue_driver();
    let mut app = TestApp::create(move |builder| builder.with_config(|root, _| {
        let driver = worker_queue_driver();
        let mut database = json!({"default":"test","connections":{"test":worker_database(root)}});
        if driver == "redis" {
            database["redis"] = json!({
                "client":"redis",
                "options":{"prefix":format!("pulse_worker_{:x}:", fastrand::u64(..))},
                "default":{"url":std::env::var("OPENRUST_TEST_REDIS_URL").expect("独占 Redis 连接")}
            });
        }
        Ok([
        ("pulse".into(),json!({"ingest":{"driver":"storage","buffer":5000,"trim":{"lottery":[0,1]}},"recorders":{"Queues":{"sample_rate":1},"SlowJobs":{"threshold":0},"Exceptions":{},"Servers":false}})),
        ("database".into(), database),
        ("cache".into(), json!({"default":"array","stores":{"array":{"driver":"array"}}})),
        ("queue".into(), json!({"default":driver,"connections":{
            "database":{"driver":"database","table":"jobs","queue":"default","retry_after":90},
            "redis":{"driver":"redis","connection":"default","queue":"default","retry_after":90}
        },"failed":{"driver":"null"}})),
    ].into())}).with_providers(vec![Box::new(PulseServiceProvider), Box::new(Register { values })])).unwrap();
    std::fs::write(app.root().join("worker.sqlite"), []).unwrap();
    app.boot().await.unwrap();
    app.app()
        .make::<Dispatcher>()
        .await
        .unwrap()
        .listen(JobProcessing::NAME, Arc::new(JobPreflight), Delivery::Sync)
        .unwrap();
    let connection = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    if driver == "database" {
        connection
            .get_schema_builder()
            .create("jobs", |table| {
                table.id();
                table.string("queue", 255).index();
                table.long_text("payload");
                table.unsigned_integer("attempts");
                table.unsigned_big_integer("reserved_at").nullable();
                table.unsigned_big_integer("available_at");
                table.unsigned_big_integer("created_at");
            })
            .await
            .unwrap();
    }
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    (app, connection, driver)
}

fn worker_queue_driver() -> &'static str {
    if std::env::var("PULSE_TEST_WORKER_QUEUE").as_deref() == Ok("redis") {
        assert_eq!(std::env::var("OPENRUST_REQUIRE_REDIS").as_deref(), Ok("1"));
        "redis"
    } else {
        "database"
    }
}

fn worker_database(root: &std::path::Path) -> Value {
    let driver = std::env::var("PULSE_TEST_DRIVER").unwrap_or_else(|_| "sqlite".into());
    let (required, url) = match driver.as_str() {
        "sqlite" => {
            return json!({"driver":"sqlite","database":root.join("worker.sqlite"),"max_connections":1});
        }
        "pgsql" => ("OPENRUST_REQUIRE_POSTGRES", "OPENRUST_TEST_POSTGRES_URL"),
        "mysql" => ("OPENRUST_REQUIRE_MYSQL", "OPENRUST_TEST_MYSQL_URL"),
        driver => panic!("无对应的独占工作进程验收驱动：{driver}"),
    };
    assert_eq!(std::env::var(required).as_deref(), Ok("1"));
    json!({
        "driver":driver,
        "url":std::env::var(url).unwrap(),
        "prefix":format!("pulse_{:x}_", fastrand::u64(..)),
        "prefix_indexes":driver == "pgsql",
        "max_connections":1
    })
}

#[tokio::test]
async fn native_worker_records_job_attempts_failures_and_flushes_each_scoped_buffer() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let (mut app, connection, driver) = worker_application(values.clone()).await;
    let queue = app.app().make::<QueueManager>().await.unwrap();
    for fail in [false, true, false, false] {
        queue
            .dispatch(
                Payload::new("pulse-scope", json!({"fail":fail})).unwrap(),
                Default::default(),
                None,
            )
            .await
            .unwrap();
    }
    let retry = Payload::new("pulse-scope", json!({"retry": true}))
        .unwrap()
        .with_options(JobOptions {
            max_tries: Some(2),
            ..Default::default()
        });
    queue
        .dispatch(retry, Default::default(), None)
        .await
        .unwrap();
    app.artisan([
        "queue:work",
        driver,
        "--stop-when-empty",
        "--sleep=0",
        "--memory=0",
    ])
    .await
    .assert_successful();
    assert_eq!(
        *values.lock().unwrap(),
        [false, true, false, false, true, false].map(|fails| json!(fails))
    );
    let transport = queue.connection(Some(driver)).unwrap();
    assert_eq!(transport.size(None).await.unwrap(), 0);
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "worker_custom")
            .count("*")
            .await
            .unwrap(),
        6
    );
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "exception")
            .count("*")
            .await
            .unwrap(),
        8
    );
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "slow_job")
            .count("*")
            .await
            .unwrap(),
        6
    );
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "slow_job")
            .r#where("value", ">=", 40)
            .count("*")
            .await
            .unwrap(),
        6
    );
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let counts = storage
        .aggregate_total(
            &["processed", "failed", "processing", "released"].map(str::to_owned),
            "count",
            3600,
        )
        .await
        .unwrap();
    assert_eq!(counts["processed"], Some(4.0));
    assert_eq!(counts["failed"], Some(1.0));
    assert_eq!(counts["processing"], Some(6.0));
    assert_eq!(counts["released"], Some(1.0));
}

#[derive(Default)]
struct TimeoutProbe(Mutex<Option<JobTimedOut>>);

impl Listener for TimeoutProbe {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            *self.0.lock().unwrap() = event.downcast_ref::<JobTimedOut>().cloned();
            Ok(None)
        })
    }
}

#[tokio::test]
async fn embedded_worker_timeout_clears_unsettled_slow_job_attempt() {
    use openrust::contracts::foundation::Application as _;
    let (mut app, connection, driver) = worker_application(Arc::default()).await;
    let probe = Arc::new(TimeoutProbe::default());
    app.app()
        .make::<Dispatcher>()
        .await
        .unwrap()
        .listen(JobTimedOut::NAME, probe.clone(), Delivery::Sync)
        .unwrap();
    let queue = app.app().make::<QueueManager>().await.unwrap();
    queue
        .dispatch(
            Payload::new("pulse-scope", json!({"timeout":true})).unwrap(),
            Default::default(),
            None,
        )
        .await
        .unwrap();
    app.artisan([
        "queue:work",
        driver,
        "--timeout=1",
        "--tries=3",
        "--sleep=0",
        "--memory=0",
    ])
    .await
    .assert_exit_code(1);
    let timed_out = probe
        .0
        .lock()
        .unwrap()
        .take()
        .expect("真实 Worker 未派发超时事件");
    let transport = queue.connection(Some(driver)).unwrap();
    assert_eq!(transport.size(None).await.unwrap(), 1);
    let pulse = app.app().make::<openrust_pulse::Pulse>().await.unwrap();
    let slow = pulse
        .recorders()
        .into_iter()
        .find(|recorder| recorder.name() == "SlowJobs")
        .unwrap();
    slow.record(
        &JobFailed {
            connection_name: timed_out.connection_name,
            job: timed_out.job,
            exception: "旧工作进程的延迟事件".into(),
        },
        &pulse,
    )
    .await
    .unwrap();
    pulse.ingest().await;
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "slow_job")
            .count("*")
            .await
            .unwrap(),
        0
    );

    app.app()
        .config()
        .unwrap()
        .set("queue.kill_on_timeout", json!(false))
        .unwrap();
    queue
        .dispatch(
            Payload::new("pulse-scope", json!({"timeout_once":true})).unwrap(),
            Default::default(),
            None,
        )
        .await
        .unwrap();
    app.artisan([
        "queue:work",
        driver,
        "--timeout=1",
        "--tries=3",
        "--sleep=0",
        "--memory=0",
        "--stop-when-empty",
    ])
    .await
    .assert_successful();
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "slow_job")
            .count("*")
            .await
            .unwrap(),
        2
    );
    assert_eq!(transport.size(None).await.unwrap(), 1);
}
