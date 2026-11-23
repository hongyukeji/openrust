use openrust::{
    contracts::queue::JobHandler,
    foundation::Application,
    http::HttpFuture,
    queue::{JobContext, Payload, QueueError, QueueFuture, QueueManager, QueueResult},
    support::{ProviderResult, ServiceProvider},
    testing::TestApp,
};
use openrust_pennant::{Feature, FeatureManager, PennantServiceProvider};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Job(Arc<Mutex<Vec<Value>>>);
impl JobHandler for Job {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let first = Feature::value("counter").await.map_err(execution)?;
            assert_eq!(Feature::value("counter").await.map_err(execution)?, first);
            self.0.lock().unwrap().push(first);
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
            let counter = Arc::new(AtomicUsize::new(0));
            app.make::<FeatureManager>().await?.store(None)?.define(
                "counter",
                Arc::new(move |_| {
                    let value = counter.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async move { Ok(json!(value)) })
                }),
            )?;
            app.make::<QueueManager>()
                .await?
                .register("pennant-scope", Arc::new(Job(self.values.clone())))?;
            Ok(())
        })
    }
}
#[tokio::test]
async fn native_worker_isolates_feature_values_across_success_and_failed_jobs() {
    let values = Arc::new(Mutex::new(Vec::new()));
    let observed = values.clone();
    let mut app = TestApp::create(move |builder| builder.with_config(|root, _| Ok([
        ("pennant".into(), json!({"default":"array"})),
        ("database".into(), json!({"default":"test","connections":{"test":{"driver":"sqlite","database":root.join("worker.sqlite"),"max_connections":1}}})),
        ("cache".into(), json!({"default":"array","stores":{"array":{"driver":"array"}}})),
        ("queue".into(), json!({"default":"database","connections":{"database":{"driver":"database","table":"jobs","queue":"default","retry_after":90}},"failed":{"driver":"null"}})),
    ].into())).with_providers(vec![Box::new(PennantServiceProvider), Box::new(Register { values: observed })])).unwrap();
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
            table.long_text("payload");
            table.unsigned_integer("attempts");
            table.unsigned_big_integer("reserved_at").nullable();
            table.unsigned_big_integer("available_at");
            table.unsigned_big_integer("created_at");
        })
        .await
        .unwrap();
    let queue = app.app().make::<QueueManager>().await.unwrap();
    for fail in [false, true, false, false] {
        queue
            .dispatch(
                Payload::new("pennant-scope", json!({"fail":fail})).unwrap(),
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
        vec![json!(0), json!(1), json!(2), json!(3)]
    );
    assert_eq!(connection.table("jobs").count("*").await.unwrap(), 0);
}
