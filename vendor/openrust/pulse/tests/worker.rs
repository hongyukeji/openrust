use openrust::{
    contracts::queue::JobHandler,
    foundation::Application,
    http::HttpFuture,
    queue::{JobContext, Payload, QueueError, QueueFuture, QueueManager, QueueResult},
    support::{ProviderResult, ServiceProvider},
    testing::TestApp,
};
use openrust_pulse::{PulseServiceProvider, facades::Pulse, storage::DatabaseStorage};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

struct Job(Arc<Mutex<Vec<Value>>>);
impl JobHandler for Job {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            Pulse::record("worker_custom", payload.uuid(), Some(1), None)
                .await
                .map_err(execution)?
                .count();
            Pulse::report("WorkerException", Some("jobs/test.rs:1".into()))
                .await
                .map_err(execution)?;
            self.0.lock().unwrap().push(payload.data()["fail"].clone());
            if payload.data()["fail"] == true {
                return Err(execution(std::io::Error::other("expected failure")));
            }
            Ok(())
        })
    }
}
fn execution(source: impl std::error::Error + Send + Sync + 'static) -> QueueError {
    QueueError::Execution {
        source: Box::new(source),
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
#[tokio::test]
async fn native_worker_records_job_attempts_failures_and_flushes_each_scoped_buffer() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let observed = values.clone();
    let mut app = TestApp::create(move |builder| builder.with_config(|root, _| Ok([
        ("pulse".into(),json!({"ingest":{"driver":"storage","buffer":5000,"trim":{"lottery":[0,1]}},"recorders":{"Queues":{"sample_rate":1},"SlowJobs":{"threshold":0},"Exceptions":{},"Servers":false}})),
        ("database".into(), json!({"default":"test","connections":{"test":{"driver":"sqlite","database":root.join("worker.sqlite"),"max_connections":1}}})),
        ("cache".into(), json!({"default":"array","stores":{"array":{"driver":"array"}}})),
        ("queue".into(), json!({"default":"database","connections":{"database":{"driver":"database","table":"jobs","queue":"default","retry_after":90}},"failed":{"driver":"null"}})),
    ].into())).with_providers(vec![Box::new(PulseServiceProvider), Box::new(Register { values: observed })])).unwrap();
    std::fs::write(app.root().join("worker.sqlite"), []).unwrap();
    app.boot().await.unwrap();
    let connection = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    connection
        .get_schema_builder()
        .create("jobs", |table| {
            table.id();
            table.string("queue", 255).index();
            table.longText("payload");
            table.unsigned_integer("attempts");
            table.unsigned_big_integer("reserved_at").nullable();
            table.unsigned_big_integer("available_at");
            table.unsigned_big_integer("created_at");
        })
        .await
        .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
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
    app.artisan([
        "queue:work",
        "database",
        "--stop-when-empty",
        "--sleep=0",
        "--memory=0",
    ])
    .await
    .assert_successful();
    assert_eq!(
        *values.lock().unwrap(),
        vec![json!(false), json!(true), json!(false), json!(false)]
    );
    assert_eq!(connection.table("jobs").count("*").await.unwrap(), 0);
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "worker_custom")
            .count("*")
            .await
            .unwrap(),
        4
    );
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "exception")
            .count("*")
            .await
            .unwrap(),
        5
    );
    assert_eq!(
        connection
            .table("pulse_entries")
            .r#where("type", "=", "slow_job")
            .count("*")
            .await
            .unwrap(),
        4
    );
    let storage = app.app().make::<DatabaseStorage>().await.unwrap();
    let counts = storage
        .aggregateTotal(
            &["processed", "failed", "processing"].map(str::to_owned),
            "count",
            3600,
        )
        .await
        .unwrap();
    assert_eq!(counts["processed"], Some(3.0));
    assert_eq!(counts["failed"], Some(1.0));
    assert_eq!(counts["processing"], Some(4.0));
}
