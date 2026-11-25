use openrust::{
    config::{ConfigError, Environment, Value},
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request},
    testing::TestApp,
};
use openrust_horizon::{Horizon, HorizonServiceProvider, JobRecord, Repository};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

fn routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::routes()
}
fn authorize(request: &mut Request) -> HttpFuture<'_, Result<bool, HttpError>> {
    let allowed = request
        .headers()
        .get("x-test-operator")
        .is_some_and(|v| v == "operator");
    Box::pin(async move { Ok(allowed) })
}
fn authorized_routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::auth(authorize).unwrap();
    Horizon::routes()
}
fn app(authorized: bool) -> TestApp {
    TestApp::create(|builder| {
        builder
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(if authorized {
                authorized_routes
            } else {
                routes
            }))
    })
    .unwrap()
}
#[tokio::test]
async fn dashboard_denies_by_default_and_authorization_is_application_local_and_lazy() {
    let mut denied = app(false);
    let mut authorized = app(true);
    denied
        .get("/api/horizon")
        .await
        .unwrap()
        .assert_status(http::StatusCode::FORBIDDEN);
    authorized
        .get("/api/horizon/api/stats")
        .await
        .unwrap()
        .assert_status(http::StatusCode::FORBIDDEN);
    let request = http::Request::builder()
        .uri("/api/horizon")
        .header("x-test-operator", "operator")
        .body(openrust::http::Body::empty())
        .unwrap();
    let response = authorized.call(request).await.unwrap();
    response
        .assert_ok()
        .assert_header("cache-control", "no-store");
    assert!(String::from_utf8_lossy(response.body()).contains("Recent attempts"));
    denied
        .get("/api/horizon")
        .await
        .unwrap()
        .assert_status(http::StatusCode::FORBIDDEN);
    denied
        .artisan(["help", "horizon"])
        .await
        .assert_successful();
    denied
        .artisan(["vendor:publish", "--tag=horizon-config"])
        .await
        .assert_successful();
    let path = denied.root().join("config/horizon.rs");
    assert!(path.exists());
    std::fs::write(&path, "// customization").unwrap();
    denied
        .artisan(["vendor:publish", "--tag=horizon-config"])
        .await
        .assert_successful();
    assert_eq!(std::fs::read_to_string(path).unwrap(), "// customization");
}

fn url() -> String {
    assert_eq!(std::env::var("HORIZON_REQUIRE_REDIS").as_deref(), Ok("1"));
    assert!(
        std::env::var("HORIZON_ACCEPTANCE_REDIS_CONTAINER")
            .unwrap()
            .starts_with("openrust-horizon-")
    );
    std::env::var("HORIZON_TEST_REDIS_URL").unwrap()
}
fn record() -> JobRecord {
    JobRecord {
        id: uuid::Uuid::now_v7().to_string(),
        name: "probe".into(),
        connection: "redis".into(),
        queue: "default".into(),
        attempts: 1,
        status: String::new(),
        duration: 12.5,
        updated_at: 0,
    }
}
#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_metadata_is_atomic_bounded_isolated_and_uses_server_time() {
    let config = Arc::new(openrust::config::Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"redis":{"options":{"prefix":format!("test-{}:",uuid::Uuid::now_v7())},"default":{"url":url()}}}),
    )])));
    let connection = openrust::redis::RedisManager::new(config)
        .connection(None)
        .unwrap();
    let store = Arc::new(Repository::new(connection.clone(), "horizon:".into(), 60).unwrap());
    let item = record();
    let mut tasks = Vec::new();
    for _ in 0..24 {
        let store = store.clone();
        let item = item.clone();
        tasks.push(tokio::spawn(async move {
            store.record(item, "finalize", false).await.unwrap()
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let metrics = store.metrics("redis", "default").await.unwrap();
    assert_eq!(metrics.attempts, 1);
    assert_eq!(metrics.completed, 1);
    assert_eq!(metrics.runtime_ms, 12.5);
    let recent = store.recent(100).await.unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].status, "completed");
    assert!(recent[0].updated_at > 0);
    let mut second = item.clone();
    second.attempts = 2;
    store
        .record(second.clone(), "processing", false)
        .await
        .unwrap();
    store
        .record(second.clone(), "released", false)
        .await
        .unwrap();
    store.record(second, "finalize", false).await.unwrap();
    store.record(item.clone(), "finalize", false).await.unwrap();
    let metrics = store.metrics("redis", "default").await.unwrap();
    assert_eq!(metrics.attempts, 2);
    assert_eq!(metrics.released, 1);
    // Manual queue:retry resets attempts even after a later attempt finalized.
    store
        .record(item.clone(), "processing", false)
        .await
        .unwrap();
    store.record(item.clone(), "finalize", false).await.unwrap();
    store.record(item.clone(), "finalize", false).await.unwrap();
    let metrics = store.metrics("redis", "default").await.unwrap();
    assert_eq!(metrics.attempts, 3);
    assert_eq!(metrics.completed, 2);
    let mut failed = record();
    failed.duration = 0.;
    store.record(failed.clone(), "failed", true).await.unwrap();
    store.record(failed, "finalize", true).await.unwrap();
    assert_eq!(store.metrics("redis", "default").await.unwrap().failed, 1);
    let metrics = store.metrics("redis", "default").await.unwrap();
    assert_eq!(metrics.runtime_ms, 25.);
    assert_eq!(metrics.runtime_for_queue(), 12.5);
    let other = Repository::new(connection.clone(), "another:".into(), 60).unwrap();
    assert!(other.recent(10).await.unwrap().is_empty());
    store.pause().await.unwrap();
    assert!(store.paused().await.unwrap());
    assert!(!other.paused().await.unwrap());
    store.r#continue().await.unwrap();
    assert!(!store.paused().await.unwrap());
    let mut other_queue = item;
    other_queue.queue = "another".into();
    store.record(other_queue, "finalize", false).await.unwrap();
    assert_eq!(
        store.metrics("redis", "another").await.unwrap().completed,
        1
    );
    assert!(store.recent(101).await.is_err());
    let mut invalid = record();
    invalid.duration = f64::NAN;
    assert!(store.record(invalid, "finalize", false).await.is_err());
    let mut long = record();
    long.queue = "long".into();
    long.duration = 7_200_000.;
    store.record(long, "finalize", false).await.unwrap();
    assert_eq!(
        store
            .metrics("redis", "long")
            .await
            .unwrap()
            .runtime_for_queue(),
        7_200_000.
    );
    // Dedicated metric:job:* counters persist until a snapshot; only legacy
    // attempt metadata and rolling minute buckets use the configured retention.
    let script = "for _,pattern in ipairs(ARGV) do for _,key in ipairs(redis.call('KEYS',pattern)) do local ttl=redis.call('TTL',key); if ttl<1 or ttl>60 then return 0 end end end; return 1";
    let jobs = connection.key("horizon:{horizon}:job:*");
    let metrics = connection.key("horizon:{horizon}:metrics:*");
    assert_eq!(
        connection
            .command(
                "EVAL",
                &[script.as_bytes(), b"0", jobs.as_bytes(), metrics.as_bytes()]
            )
            .await
            .unwrap(),
        openrust::redis::Value::Int(1)
    );
}
fn config(base: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":"testing"})),
        ("horizon".into(), json!({"environments":{"testing":{}}})),
        (
            "database".into(),
            json!({"redis":{"options":{"prefix":format!("horizon-{}:",base.file_name().unwrap().to_str().unwrap())},"default":{"url":url()}}}),
        ),
        (
            "cache".into(),
            json!({"default":"file","stores":{"file":{"driver":"file","path":base.join("cache")}}}),
        ),
        (
            "queue".into(),
            json!({"default":"redis","connections":{"redis":{"driver":"redis","connection":"default","queue":"default","retry_after":90}},"failed":{"driver":"null"}}),
        ),
    ]))
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn artisan_supervisor_commands_select_the_local_host_and_report_master_status() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
    })
    .unwrap();
    app.boot().await.unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let host = hostname::get().unwrap().into_string().unwrap();
    let peer = format!("{host}-peer");

    let inactive = app.artisan(["horizon:status"]).await;
    inactive.assert_exit_code(2);
    assert!(inactive.stdout.contains("ERROR") && inactive.stdout.contains("Horizon is inactive."));

    let masters = repository.master_supervisors();
    masters
        .update(
            "local-master",
            &json!({"name":"local-master","host":host,"status":"running"}),
        )
        .await
        .unwrap();
    for index in 0..100 {
        let name = format!("local-master-{index:03}");
        masters
            .update(&name, &json!({"name": name,"host":host,"status":"running"}))
            .await
            .unwrap();
    }
    masters
        .update(
            "remote-master",
            &json!({"name":"remote-master","host":peer,"status":"paused"}),
        )
        .await
        .unwrap();
    assert!(!repository.paused().await.unwrap());
    let paused = app.artisan(["horizon:status"]).await;
    paused.assert_exit_code(1);
    assert!(paused.stdout.contains("WARN") && paused.stdout.contains("Horizon is paused."));
    let json_status = app.artisan(["horizon:status", "--json"]).await;
    json_status.assert_exit_code(1);
    assert_eq!(
        serde_json::from_str::<Vec<Value>>(&json_status.stdout)
            .unwrap()
            .len(),
        102
    );
    let list = app.artisan(["horizon:list"]).await;
    list.assert_successful();
    assert!(list.stdout.contains("remote-master"));
    masters.forget("remote-master").await.unwrap();
    let running = app.artisan(["horizon:status"]).await;
    running.assert_successful();
    assert!(running.stdout.contains("INFO") && running.stdout.contains("Horizon is running."));

    for (name, state) in [
        (
            "first:shared-high",
            json!({"host":peer,"status":"paused","pid":1}),
        ),
        (
            "second:shared-high",
            json!({"host":host,"status":"running","pid":1}),
        ),
        (
            "third:remote-only",
            json!({"host":peer,"status":"running","pid":1}),
        ),
        ("fourth:legacy-only", json!({"status":"running","pid":1})),
    ] {
        repository.heartbeat_supervisor(name, &state).await.unwrap();
    }
    let local = app.artisan(["horizon:supervisor-status", "high"]).await;
    local.assert_successful();
    assert!(local.stdout.contains("high is running"));
    for (name, key) in [
        ("remote-only", "third:remote-only"),
        ("legacy-only", "fourth:legacy-only"),
    ] {
        app.artisan(["horizon:supervisor-status", name])
            .await
            .assert_exit_code(1);
        app.artisan(["horizon:pause-supervisor", name])
            .await
            .assert_exit_code(1);
        assert!(!repository.supervisor_paused(key).await.unwrap());
        repository.pause_supervisor(key).await.unwrap();
        app.artisan(["horizon:continue-supervisor", name])
            .await
            .assert_exit_code(1);
        assert!(repository.supervisor_paused(key).await.unwrap());
    }
}

struct Handler(&'static str);
impl openrust::contracts::queue::JobHandler for Handler {
    fn handle<'a>(
        &'a self,
        context: &'a mut openrust::queue::JobContext<'_>,
        _: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async move {
            match self.0 {
                "release" => context.release(Duration::from_secs(60)),
                "fail" => return Err(openrust::queue::QueueError::InvalidPayload),
                _ => {}
            }
            Ok(())
        })
    }
}

#[derive(Debug)]
struct TransportObservation {
    name: String,
    queue: String,
    id: String,
    uuid: String,
    counts: Option<(u64, u64, u64)>,
    status: String,
    monitored: bool,
    attempts: u32,
    delay_ms: Option<u64>,
    failed: Option<bool>,
}

struct TransportListener {
    queue: Arc<dyn openrust::contracts::queue::Queue>,
    repository: Arc<Repository>,
    observations: std::sync::Mutex<Vec<TransportObservation>>,
    fail_settlement_listeners: bool,
    fail_reservation_listener: bool,
}

impl openrust::contracts::events::Listener for TransportListener {
    fn handle<'a>(
        &'a self,
        event: &'a dyn openrust::contracts::events::Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            use openrust_horizon::{JobDeleted, JobPending, JobPushed, JobReleased, JobReserved};
            let released = event.downcast_ref::<JobReleased>();
            let deleted = event.downcast_ref::<JobDeleted>();
            let (connection, queue, id, payload, attempts) =
                if let Some(event) = event.downcast_ref::<JobPending>() {
                    (
                        &event.connection_name,
                        &event.queue,
                        &event.id,
                        &event.payload,
                        0,
                    )
                } else if let Some(event) = event.downcast_ref::<JobPushed>() {
                    (
                        &event.connection_name,
                        &event.queue,
                        &event.id,
                        &event.payload,
                        0,
                    )
                } else if let Some(event) = event.downcast_ref::<JobReserved>() {
                    (
                        &event.connection_name,
                        &event.job.queue,
                        &event.job.id,
                        &event.job.payload,
                        event.job.attempts,
                    )
                } else if let Some((connection, job)) = released
                    .map(|event| (&event.connection_name, &event.job))
                    .or_else(|| deleted.map(|event| (&event.connection_name, &event.job)))
                {
                    (connection, &job.queue, &job.id, &job.payload, job.attempts)
                } else {
                    return Ok(None);
                };
            assert_eq!(connection, "redis");
            let counts = self.queue.metrics(Some(queue)).await.ok().map(|metrics| {
                (
                    metrics.pending.unwrap(),
                    metrics.delayed.unwrap(),
                    metrics.reserved.unwrap(),
                )
            });
            let details = self.repository.jobs().find(id).await.unwrap().unwrap();
            let snapshot = openrust_horizon::JobPayload::from_payload(payload)
                .unwrap()
                .unwrap();
            assert_eq!(details.tags, snapshot.tags());
            assert_eq!(details.silenced, snapshot.is_silenced());
            let monitored = self
                .repository
                .tags()
                .jobs("tracked")
                .await
                .unwrap()
                .contains(id);
            self.observations
                .lock()
                .unwrap()
                .push(TransportObservation {
                    name: event.name().into(),
                    queue: queue.clone(),
                    id: id.clone(),
                    uuid: payload.uuid().into(),
                    counts,
                    status: details.record.status,
                    monitored,
                    attempts,
                    delay_ms: details.release_delay_ms,
                    failed: deleted.map(|event| event.failed),
                });
            if let Some(event) = released {
                assert_eq!(
                    details.release_delay_ms.map(u128::from),
                    Some(event.delay.as_millis())
                );
            }
            if self.fail_settlement_listeners && (released.is_some() || deleted.is_some()) {
                return Err(openrust::events::EventError::InvalidPayload);
            }
            if self.fail_reservation_listener && event.downcast_ref::<JobReserved>().is_some() {
                return Err(openrust::events::EventError::InvalidPayload);
            }
            Ok(None)
        })
    }
}

fn transport_config(
    base: &Path,
    env: &Environment,
) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut settings = config(base, env)?;
    let database = settings.get_mut("database").unwrap();
    database["default"] = json!("sqlite");
    database["connections"] = json!({"sqlite":{"driver":"sqlite","database":":memory:"}});
    Ok(settings)
}

fn transport_routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::tag(|_| vec!["tracked".into()]).unwrap();
    Ok(())
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn delayed_transaction_prepares_horizon_metadata_before_commit() {
    use openrust::{
        contracts::foundation::Application as _,
        queue::{Delay, DispatchOptions, DispatchResult, Payload, QueueManager},
    };
    use openrust_horizon::JobPayload;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TAG_CALLS: AtomicUsize = AtomicUsize::new(0);
    fn tags(_: &Payload) -> Vec<String> {
        vec![format!("seen-{}", TAG_CALLS.fetch_add(1, Ordering::SeqCst))]
    }
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(transport_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Horizon::tag(tags).unwrap();
                Ok(())
            }))
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app.app().make::<QueueManager>().await.unwrap();
    let redis = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let jobs = repository.jobs();
    let database = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let config = app.app().config().unwrap();
    config
        .set("horizon.silenced_tags", json!(["seen-1"]))
        .unwrap();

    let payload = Payload::new("transaction-probe", json!({})).unwrap();
    let delayed = DispatchOptions {
        queue: Some("delayed".into()),
        delay: Some(Delay::For(Duration::from_secs(90))),
        after_commit: Some(true),
        ..Default::default()
    };
    let immediate = DispatchOptions {
        queue: Some("immediate".into()),
        delay: None,
        ..delayed.clone()
    };
    let mut rollback = database.begin_transaction().await.unwrap();
    for options in [&delayed, &immediate] {
        assert_eq!(
            manager
                .dispatch(payload.clone(), options.clone(), Some(&mut rollback))
                .await
                .unwrap(),
            DispatchResult::Deferred
        );
    }
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 1);
    rollback.roll_back(None).await.unwrap();
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 1);
    assert!(jobs.get_pending(None).await.unwrap().is_empty());

    let mut commit = database.begin_transaction().await.unwrap();
    for options in [&delayed, &immediate] {
        assert_eq!(
            manager
                .dispatch(payload.clone(), options.clone(), Some(&mut commit))
                .await
                .unwrap(),
            DispatchResult::Deferred
        );
    }
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 2);
    assert!(jobs.get_pending(None).await.unwrap().is_empty());
    assert_eq!(
        redis.metrics(Some("delayed")).await.unwrap().delayed,
        Some(0)
    );
    config
        .set("horizon.silenced_tags", json!(["seen-2"]))
        .unwrap();
    commit.commit().await.unwrap();
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(
        redis.metrics(Some("delayed")).await.unwrap().delayed,
        Some(1)
    );
    assert_eq!(
        redis.metrics(Some("immediate")).await.unwrap().pending,
        Some(1)
    );
    let rows = jobs.get_pending(None).await.unwrap();
    assert_eq!(rows.len(), 2);
    for (queue, tag) in [("delayed", "seen-1"), ("immediate", "seen-2")] {
        let details = rows.iter().find(|item| item.record.queue == queue).unwrap();
        assert_eq!(details.tags, [tag]);
        assert!(details.silenced);
        let payload: Payload = serde_json::from_value(details.payload.clone()).unwrap();
        let metadata = JobPayload::from_payload(&payload).unwrap().unwrap();
        assert_eq!(metadata.tags(), [tag]);
        assert!(metadata.is_silenced());
    }
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_public_transport_events_follow_writes_reservations_and_commit() {
    use openrust::{
        contracts::events::Dispatcher as _,
        database::DatabaseManager,
        events::{Delivery, Dispatcher},
        queue::{Delay, DispatchOptions, DispatchResult, Payload, QueueManager},
    };
    use openrust_horizon::{JobPending, JobPushed, JobReserved};
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(transport_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(transport_routes))
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app.app().make::<QueueManager>().await.unwrap();
    let queue = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    repository.tags().monitor("tracked").await.unwrap();
    let listener = Arc::new(TransportListener {
        queue: queue.clone(),
        repository,
        observations: Default::default(),
        fail_settlement_listeners: false,
        fail_reservation_listener: false,
    });
    let dispatcher = app.app().make::<Dispatcher>().await.unwrap();
    for name in [JobPending::NAME, JobPushed::NAME, JobReserved::NAME] {
        dispatcher
            .listen(name, listener.clone(), Delivery::Sync)
            .unwrap();
    }
    let payload = Payload::new("probe", json!({"id": 42})).unwrap();
    let immediate = queue.push(&payload, None).await.unwrap().unwrap();
    let mut reserved = queue.pop_now(None).await.unwrap().unwrap();
    assert_eq!(reserved.get_job_id(), immediate);
    reserved.delete().await.unwrap();
    let delayed = queue
        .later(
            Delay::For(Duration::from_secs(300)),
            &payload,
            Some("later"),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(queue.pop_now(Some("later")).await.unwrap().is_none());
    let redis = app
        .app()
        .make::<openrust::redis::RedisManager>()
        .await
        .unwrap();
    redis
        .connection(None)
        .unwrap()
        .set("queues:broken", b"wrong type")
        .await
        .unwrap();
    assert!(queue.push(&payload, Some("broken")).await.is_err());
    let database = app
        .app()
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let options = DispatchOptions {
        queue: Some("transactional".into()),
        after_commit: Some(true),
        ..Default::default()
    };
    let mut transaction = database.begin_transaction().await.unwrap();
    assert_eq!(
        manager
            .dispatch(payload.clone(), options.clone(), Some(&mut transaction))
            .await
            .unwrap(),
        DispatchResult::Deferred
    );
    transaction.roll_back(None).await.unwrap();
    let mut transaction = database.begin_transaction().await.unwrap();
    assert_eq!(
        manager
            .dispatch(payload.clone(), options, Some(&mut transaction))
            .await
            .unwrap(),
        DispatchResult::Deferred
    );
    assert_eq!(listener.observations.lock().unwrap().len(), 6);
    transaction.commit().await.unwrap();
    let observations = listener.observations.lock().unwrap();
    assert_eq!(observations.len(), 8);
    for row in observations.iter() {
        assert_eq!(row.uuid, payload.uuid());
        assert_ne!(row.id, row.uuid);
    }
    assert_eq!(observations[0].id, immediate);
    assert_eq!(observations[1].id, immediate);
    assert_eq!(observations[2].id, immediate);
    assert_eq!(observations[3].id, delayed);
    assert_eq!(observations[4].id, delayed);
    assert_eq!(observations[6].id, observations[7].id);
    let rows: Vec<_> = observations
        .iter()
        .map(|row| {
            (
                row.name.as_str(),
                row.queue.as_str(),
                row.counts,
                row.status.as_str(),
                row.monitored,
                row.attempts,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                "JobPending",
                "default",
                Some((0, 0, 0)),
                "pending",
                false,
                0
            ),
            ("JobPushed", "default", Some((1, 0, 0)), "pending", true, 0),
            (
                "JobReserved",
                "default",
                Some((0, 0, 1)),
                "reserved",
                true,
                1
            ),
            ("JobPending", "later", Some((0, 0, 0)), "pending", false, 0),
            ("JobPushed", "later", Some((0, 1, 0)), "pending", true, 0),
            ("JobPending", "broken", None, "pending", false, 0),
            (
                "JobPending",
                "transactional",
                Some((0, 0, 0)),
                "pending",
                false,
                0
            ),
            (
                "JobPushed",
                "transactional",
                Some((1, 0, 0)),
                "pending",
                true,
                0
            ),
        ]
    );
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_public_settlement_events_follow_confirmed_operations_and_count_once() {
    use openrust::{
        contracts::events::Dispatcher as _,
        events::{Delivery, Dispatcher},
        queue::{JobOptions, Payload, QueueManager},
    };
    use openrust_horizon::{JobDeleted, JobReleased};
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(authorized_routes))
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app.app().make::<QueueManager>().await.unwrap();
    let queue = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let listener = Arc::new(TransportListener {
        queue: queue.clone(),
        repository: repository.clone(),
        observations: Default::default(),
        fail_settlement_listeners: true,
        fail_reservation_listener: false,
    });
    let dispatcher = app.app().make::<Dispatcher>().await.unwrap();
    for name in [JobReleased::NAME, JobDeleted::NAME] {
        dispatcher
            .listen(name, listener.clone(), Delivery::Sync)
            .unwrap();
    }
    let payload = Payload::new("probe", json!({"id": 42})).unwrap();
    let release_id = queue
        .push(&payload, Some("release"))
        .await
        .unwrap()
        .unwrap();
    let mut released = queue.pop_now(Some("release")).await.unwrap().unwrap();
    released
        .release(Duration::from_millis(90_123))
        .await
        .unwrap();
    assert!(released.is_released());
    released.release(Duration::ZERO).await.unwrap();
    released.delete().await.unwrap();

    let delete_id = queue.push(&payload, Some("delete")).await.unwrap().unwrap();
    let mut deleted = queue.pop_now(Some("delete")).await.unwrap().unwrap();
    deleted.delete().await.unwrap();
    assert!(deleted.is_deleted());
    deleted.delete().await.unwrap();
    assert_eq!(
        repository
            .metrics("redis", "delete")
            .await
            .unwrap()
            .completed,
        1
    );

    queue.push(&payload, Some("stale")).await.unwrap();
    let mut stale = queue.pop_now(Some("stale")).await.unwrap().unwrap();
    queue.clearable().unwrap().clear("stale").await.unwrap();
    assert!(stale.release(Duration::ZERO).await.is_err());
    assert!(stale.delete().await.is_err());
    assert!(!stale.is_deleted() && !stale.is_released());

    let chain_id = queue.push(&payload, Some("chain")).await.unwrap().unwrap();
    let mut member = queue.pop_now(Some("chain")).await.unwrap().unwrap();
    member
        .advance_chain(&Payload::new("next", json!({})).unwrap())
        .await
        .unwrap();
    assert_eq!(queue.metrics(Some("chain")).await.unwrap().pending, Some(1));

    manager.register("fail", Arc::new(Handler("fail"))).unwrap();
    let failed_id = queue
        .push(&Payload::new("fail", json!({})).unwrap(), None)
        .await
        .unwrap()
        .unwrap();
    app.artisan(["queue:work", "redis", "--once", "--force", "--memory=1024"])
        .await
        .assert_successful();
    let retry = Payload::new("fail", json!({}))
        .unwrap()
        .with_options(JobOptions {
            max_tries: Some(2),
            backoff: vec![Duration::from_secs(90)],
            ..Default::default()
        });
    let retry_id = queue.push(&retry, None).await.unwrap().unwrap();
    app.artisan(["queue:work", "redis", "--once", "--force", "--memory=1024"])
        .await
        .assert_successful();
    let metrics = repository.metrics("redis", "default").await.unwrap();
    assert_eq!(
        (
            metrics.attempts,
            metrics.failed,
            metrics.retrying,
            metrics.completed
        ),
        (2, 1, 1, 0)
    );
    let details = repository.jobs().find(&retry_id).await.unwrap().unwrap();
    assert_eq!(details.release_delay_ms, Some(90_000));
    let request = http::Request::builder()
        .uri(format!("/api/horizon/api/jobs/{retry_id}"))
        .header("x-test-operator", "operator")
        .body(openrust::http::Body::empty())
        .unwrap();
    let response = app.call(request).await.unwrap();
    response.assert_ok();
    let job: Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(job["release_delay_ms"], 90_000);
    let observations = listener.observations.lock().unwrap();
    let expected = [
        (
            JobReleased::NAME,
            &release_id,
            (0, 1, 0),
            "pending",
            Some(90_123),
            None,
        ),
        (
            JobDeleted::NAME,
            &delete_id,
            (0, 0, 0),
            "completed",
            None,
            Some(false),
        ),
        (
            JobDeleted::NAME,
            &chain_id,
            (1, 0, 0),
            "completed",
            None,
            Some(false),
        ),
        (
            JobDeleted::NAME,
            &failed_id,
            (0, 0, 0),
            "reserved",
            None,
            Some(true),
        ),
        (
            JobReleased::NAME,
            &retry_id,
            (0, 1, 0),
            "pending",
            Some(90_000),
            None,
        ),
    ];
    let observed: Vec<_> = observations
        .iter()
        .map(|row| {
            (
                row.name.as_str(),
                &row.id,
                row.counts.unwrap(),
                row.status.as_str(),
                row.delay_ms,
                row.failed,
            )
        })
        .collect();
    assert_eq!(observed, expected);
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_queue_names_and_reservation_observer_errors_preserve_delivery() {
    use openrust::{
        contracts::events::Dispatcher as _,
        events::{Delivery, Dispatcher},
        queue::{Payload, QueueManager},
    };
    use openrust_horizon::JobReserved;
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(authorized_routes))
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app.app().make::<QueueManager>().await.unwrap();
    let queue = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let listener = Arc::new(TransportListener {
        queue: queue.clone(),
        repository: repository.clone(),
        observations: Default::default(),
        fail_settlement_listeners: false,
        fail_reservation_listener: true,
    });
    app.app()
        .make::<Dispatcher>()
        .await
        .unwrap()
        .listen(JobReserved::NAME, listener.clone(), Delivery::Sync)
        .unwrap();
    manager
        .register("probe", Arc::new(Handler("success")))
        .unwrap();
    let payload = Payload::new("probe", json!({})).unwrap();
    let name = "出库 汇总:报表";
    let long = "a".repeat(255);
    let group = format!("{name},{long}");
    let mut ids = Vec::new();
    for queue_name in [name, &long] {
        ids.push(
            queue
                .push(&payload, Some(queue_name))
                .await
                .unwrap()
                .unwrap(),
        );
    }
    repository
        .heartbeat_supervisor(
            "queue-name-probe",
            &json!({"name":"queue-name-probe","processes":{format!("redis:{group}"):1}}),
        )
        .await
        .unwrap();
    let request = http::Request::builder()
        .uri("/api/horizon/api/workload")
        .header("x-test-operator", "operator")
        .body(openrust::http::Body::empty())
        .unwrap();
    let response = app.call(request).await.unwrap();
    response.assert_ok();
    let workload: Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(workload[0]["name"], group);
    assert_eq!(workload[0]["length"], 2);
    assert_eq!(workload[0]["split_queues"][name]["length"], 1);
    assert_eq!(workload[0]["split_queues"][&long]["length"], 1);
    for queue_name in [name, &long] {
        app.artisan([
            "horizon:work",
            "redis",
            "--queue",
            &group,
            "--once",
            "--force",
            "--memory=1024",
        ])
        .await
        .assert_successful();
        assert_eq!(
            repository
                .metrics("redis", queue_name)
                .await
                .unwrap()
                .completed,
            1
        );
        assert_eq!(queue.metrics(Some(queue_name)).await.unwrap().size, 0);
    }
    let literal = "逗号,原样";
    let id = queue.push(&payload, Some(literal)).await.unwrap().unwrap();
    let mut job = queue.pop_now(Some(literal)).await.unwrap().unwrap();
    assert_eq!(job.get_job_id(), id);
    job.delete().await.unwrap();
    assert_eq!(
        repository
            .metrics("redis", literal)
            .await
            .unwrap()
            .completed,
        1
    );
    ids.push(id);
    let rows = listener.observations.lock().unwrap();
    assert_eq!(
        rows.iter().map(|row| &row.id).collect::<Vec<_>>(),
        ids.iter().collect::<Vec<_>>()
    );
    for (row, queue_name) in rows.iter().zip([name, &long, literal]) {
        assert_eq!(row.queue, queue_name);
        assert_eq!(row.status, "reserved");
        assert_eq!(row.attempts, 1);
    }
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_migration_events_precede_reservation_and_preserve_newer_attempts() {
    use openrust::{
        container::Callback,
        contracts::events::{Dispatcher as _, Event, Listener},
        events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
        queue::{Delay, Payload, QueueManager},
    };
    use openrust_horizon::JobsMigrated;

    struct MigrationProbe {
        queue: Arc<dyn openrust::contracts::queue::Queue>,
        repository: Arc<Repository>,
        observations: std::sync::Mutex<Vec<(JobsMigrated, u64, u64, u64)>>,
    }
    impl Listener for MigrationProbe {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let Some(batch) = event.downcast_ref::<JobsMigrated>() else {
                    return Ok(None);
                };
                assert_eq!(batch.connection_name, "redis");
                assert_eq!(batch.queue, "migration");
                // 回放事件时只记录状态读数，由调用方验证不能回退已有终态。
                for job in &batch.payloads {
                    let details = self.repository.jobs().find(&job.id).await.unwrap().unwrap();
                    if details.record.status == "pending" {
                        assert_eq!(details.release_delay_ms, Some(0));
                        assert_eq!(details.record.attempts, job.attempts);
                        assert_eq!(details.payload["data"], *job.payload.data());
                    }
                }
                let counts = self.queue.metrics(Some(&batch.queue)).await.unwrap();
                self.observations.lock().unwrap().push((
                    batch.clone(),
                    counts.pending.unwrap(),
                    counts.delayed.unwrap(),
                    counts.reserved.unwrap(),
                ));
                // 迁移已经发生，监听器错误不能妨碍后续保留。
                if batch.payloads.is_empty() {
                    Ok(None)
                } else {
                    Err(openrust::events::EventError::InvalidPayload)
                }
            })
        }
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(authorized_routes))
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app.app().make::<QueueManager>().await.unwrap();
    let queue = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let redis = app
        .app()
        .make::<openrust::redis::RedisManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let payload = Payload::new("migration", json!({"precise": 9_007_199_254_740_993u64})).unwrap();
    let name = Some("migration");
    let delayed = queue.push(&payload, name).await.unwrap().unwrap();
    let mut released = queue.pop_now(name).await.unwrap().unwrap();
    released.release(Duration::from_secs(120)).await.unwrap();
    let expired = queue.push(&payload, name).await.unwrap().unwrap();
    let mut stale = queue.pop_now(name).await.unwrap().unwrap();
    let future = queue
        .later(Delay::For(Duration::from_secs(120)), &payload, name)
        .await
        .unwrap()
        .unwrap();
    for (source, id) in [("delayed", &delayed), ("reserved", &expired)] {
        let key = redis.key(&format!("queues:migration:{source}"));
        let marked = redis.command("EVAL", &[
            b"for _, raw in ipairs(redis.call('ZRANGE', KEYS[1], 0, -1)) do if cjson.decode(raw).id == ARGV[1] then return redis.call('ZADD', KEYS[1], 'CH', -1, raw) end end; return 0",
            b"1", key.as_bytes(), id.as_bytes()
        ]).await.unwrap();
        assert_eq!(marked, openrust::redis::Value::Int(1));
    }
    let probe = Arc::new(MigrationProbe {
        queue: queue.clone(),
        repository: repository.clone(),
        observations: Default::default(),
    });
    let dispatcher = app.app().make::<Dispatcher>().await.unwrap();
    dispatcher
        .listen(JobsMigrated::NAME, probe.clone(), Delivery::Sync)
        .unwrap();
    let mut current = queue.pop_now(name).await.unwrap().unwrap();
    assert_eq!(current.get_job_id(), delayed);
    assert_eq!(current.attempts(), 2);
    let (stale_delayed, stale_expired) = {
        let observations = probe.observations.lock().unwrap();
        assert_eq!(observations.len(), 2);
        let (delayed_batch, ready, delayed_count, reserved) = &observations[0];
        assert_eq!((*ready, *delayed_count, *reserved), (1, 1, 1));
        assert_eq!(delayed_batch.payloads.len(), 1);
        assert_eq!(delayed_batch.payloads[0].id, delayed);
        assert_eq!(delayed_batch.payloads[0].attempts, 1);
        let (expired_batch, ready, delayed_count, reserved) = &observations[1];
        assert_eq!((*ready, *delayed_count, *reserved), (2, 1, 0));
        assert_eq!(expired_batch.payloads.len(), 1);
        assert_eq!(expired_batch.payloads[0].id, expired);
        assert_eq!(expired_batch.payloads[0].attempts, 1);
        (delayed_batch.clone(), expired_batch.clone())
    };
    let response = app
        .call(
            http::Request::builder()
                .uri(format!("/api/horizon/api/jobs/{expired}"))
                .header("x-test-operator", "operator")
                .body(openrust::http::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response.assert_ok();
    let details: Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(details["status"], "pending");
    assert_eq!(details["release_delay_ms"], 0);
    assert_eq!(
        details["payload"]["data"]["precise"],
        9_007_199_254_740_993u64
    );
    let recent = repository.recent(100).await.unwrap();
    let legacy = recent.iter().find(|job| job.id == expired).unwrap();
    assert_eq!(legacy.status, "pending");
    assert!(stale.delete().await.is_err());
    assert_eq!(
        repository
            .jobs()
            .find(&future)
            .await
            .unwrap()
            .unwrap()
            .release_delay_ms,
        None
    );

    let mut next = queue.pop_now(name).await.unwrap().unwrap();
    assert_eq!(next.get_job_id(), expired);
    assert_eq!(next.attempts(), 2);
    next.delete().await.unwrap();
    assert!(queue.pop_now(name).await.unwrap().is_none());
    {
        let observations = probe.observations.lock().unwrap();
        assert_eq!(observations.len(), 6);
        assert!(
            observations[2..]
                .iter()
                .all(|(batch, ..)| batch.payloads.is_empty())
        );
    }
    app.app()
        .call(
            Callback::new(&[], |_| async move {
                for event in [stale_delayed, stale_expired] {
                    let result = dispatcher
                        .dispatch(Arc::new(event), &mut EventContext::new())
                        .await;
                    assert!(matches!(
                        result,
                        Err(openrust::events::EventError::InvalidPayload)
                    ));
                }
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    let reserved = repository.jobs().find(&delayed).await.unwrap().unwrap();
    let completed = repository.jobs().find(&expired).await.unwrap().unwrap();
    assert_eq!(
        (reserved.record.status.as_str(), reserved.record.attempts),
        ("reserved", 2)
    );
    assert_eq!(
        (completed.record.status.as_str(), completed.record.attempts),
        ("completed", 2)
    );
    assert_eq!(
        repository
            .metrics("redis", "migration")
            .await
            .unwrap()
            .completed,
        1
    );
    current.delete().await.unwrap();
    assert_eq!(
        repository
            .metrics("redis", "migration")
            .await
            .unwrap()
            .completed,
        2
    );
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn native_worker_hooks_count_success_release_failure_and_hide_payloads() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app
        .app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap();
    for kind in ["success", "release", "fail"] {
        manager.register(kind, Arc::new(Handler(kind))).unwrap();
        let payload = openrust::queue::Payload::new(
            kind,
            json!({"password":"never-in-horizon","token":"private-token"}),
        )
        .unwrap();
        manager
            .connection(Some("redis"))
            .unwrap()
            .push(&payload, None)
            .await
            .unwrap();
        app.artisan(["queue:work", "redis", "--once", "--force", "--memory=1024"])
            .await
            .assert_successful();
    }
    let repository = app.app().make::<Repository>().await.unwrap();
    let metrics = repository.metrics("redis", "default").await.unwrap();
    assert_eq!(metrics.attempts, 3);
    assert_eq!(metrics.completed, 1);
    assert_eq!(metrics.released, 1);
    assert_eq!(metrics.failed, 1);
    let recent = repository.recent(100).await.unwrap();
    assert_eq!(recent.len(), 3);
    let json = serde_json::to_string(&recent).unwrap();
    assert!(!json.contains("never-in-horizon"));
    assert!(!json.contains("private-token"));
    assert!(!json.contains("exception"));
}

#[tokio::test]
async fn dashboard_mutations_require_authorization_and_a_same_origin_request_header() {
    let mut application = app(true);
    for (operator, marker, site) in [
        (false, false, "same-origin"),
        (true, false, "same-origin"),
        (true, true, "cross-site"),
    ] {
        let mut request = http::Request::builder()
            .method("POST")
            .uri("/api/horizon/api/pause")
            .header("sec-fetch-site", site);
        if operator {
            request = request.header("x-test-operator", "operator");
        }
        if marker {
            request = request.header("x-horizon-request", "1");
        }
        application
            .call(request.body(openrust::http::Body::empty()).unwrap())
            .await
            .unwrap()
            .assert_status(http::StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_horizon_repositories_preserve_job_lifecycle_tags_snapshots_and_process_state() {
    let config = Arc::new(openrust::config::Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"redis":{"default":{"url":url()}}}),
    )])));
    let connection = openrust::redis::RedisManager::new(config)
        .connection(None)
        .unwrap();
    let store = Repository::new(
        connection,
        format!("repositories-{}:", uuid::Uuid::now_v7()),
        60,
    )
    .unwrap();
    let payload =
        openrust::queue::Payload::new("probe", json!({"secret":"operator-only"})).unwrap();
    let mut item = record();
    assert_ne!(item.id, payload.uuid());
    let tags: Vec<String> = vec!["App\\Order:42".into()];
    store.tags().monitor(&tags[0]).await.unwrap();
    store
        .jobs()
        .pushed(&item.id, "redis", "default", &payload, &tags)
        .await
        .unwrap();
    assert_eq!(store.jobs().count_pending().await.unwrap(), 1);
    store
        .record_payload(item.clone(), "processing", &payload, &tags, false, None)
        .await
        .unwrap();
    store
        .record_payload(item.clone(), "released", &payload, &tags, false, None)
        .await
        .unwrap();
    store
        .record_payload(item.clone(), "finalize", &payload, &tags, false, None)
        .await
        .unwrap();
    assert_eq!(
        store
            .jobs()
            .find(&item.id)
            .await
            .unwrap()
            .unwrap()
            .record
            .status,
        "pending"
    );
    assert_eq!(store.jobs().count_completed().await.unwrap(), 0);
    item.attempts = 2;
    store
        .record_payload(item.clone(), "processing", &payload, &tags, false, None)
        .await
        .unwrap();
    store
        .record_payload(
            item.clone(),
            "failed",
            &payload,
            &tags,
            false,
            Some("operator-only exception"),
        )
        .await
        .unwrap();
    store
        .record_payload(
            item.clone(),
            "finalize",
            &payload,
            &tags,
            false,
            Some("operator-only exception"),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .jobs()
            .find_failed(&item.id)
            .await
            .unwrap()
            .unwrap()
            .exception
            .as_deref(),
        Some("operator-only exception")
    );
    assert_eq!(store.jobs().count_failed().await.unwrap(), 1);
    assert_eq!(
        store
            .tags()
            .count(&format!("failed:{}", tags[0]))
            .await
            .unwrap(),
        1
    );
    assert!(
        !serde_json::to_string(&store.recent(100).await.unwrap())
            .unwrap()
            .contains("operator-only")
    );
    assert_eq!(
        store.tags().paginate(&tags[0], 0, 25).await.unwrap(),
        vec![item.id.clone()]
    );
    store
        .jobs()
        .store_retry_reference(&item.id, "retry-id")
        .await
        .unwrap();
    assert_eq!(
        store
            .jobs()
            .find(&item.id)
            .await
            .unwrap()
            .unwrap()
            .retried_by
            .len(),
        1
    );
    assert!(store.jobs().delete_failed(&item.id).await.unwrap());
    assert!(!store.jobs().delete_failed(&item.id).await.unwrap());
    assert_eq!(store.tags().count(&tags[0]).await.unwrap(), 0);
    item.attempts = 1;
    store
        .record_payload(item.clone(), "processing", &payload, &tags, true, None)
        .await
        .unwrap();
    store
        .record_payload(item.clone(), "finalize", &payload, &tags, true, None)
        .await
        .unwrap();
    store
        .record_payload(item.clone(), "finalize", &payload, &tags, true, None)
        .await
        .unwrap();
    assert_eq!(store.jobs().count_silenced().await.unwrap(), 1);
    assert_eq!(store.jobs().count_completed().await.unwrap(), 0);
    let metrics = store.metrics_repository();
    assert_eq!(metrics.throughput_for_job("probe").await.unwrap(), 1);
    assert_eq!(metrics.runtime_for_job("probe").await.unwrap(), 12.5);
    metrics.snapshot().await.unwrap();
    assert_eq!(metrics.throughput_for_job("probe").await.unwrap(), 0);
    assert_eq!(
        metrics.snapshots_for_job("probe").await.unwrap()[0]["throughput"],
        1
    );
    for _ in 0..30 {
        metrics.increment_queue("redis:default", 5.).await.unwrap();
        metrics.snapshot().await.unwrap();
    }
    assert!(
        metrics
            .snapshots_for_queue("redis:default")
            .await
            .unwrap()
            .len()
            <= 24
    );
    metrics.clear().await.unwrap();
    assert!(metrics.measured_queues().await.unwrap().is_empty());
    assert_eq!(store.metrics("redis", "default").await.unwrap().attempts, 0);
    assert!(store.jobs().find(&item.id).await.unwrap().is_some());
    store
        .heartbeat_supervisor(
            "master:supervisor",
            &json!({"options":{"timeout":70},"processes":{"redis:default":2}}),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .supervisor_repository()
            .longest_active_timeout()
            .await
            .unwrap(),
        70
    );
    store.pause_supervisor("master:supervisor").await.unwrap();
    assert!(store.supervisor_paused("master:supervisor").await.unwrap());
    assert!(
        store
            .pop_commands("master:supervisor")
            .await
            .unwrap()
            .is_empty()
    );
    store
        .push_command("master:supervisor", "terminate")
        .await
        .unwrap();
    assert_eq!(
        store.pop_commands("master:supervisor").await.unwrap(),
        vec!["terminate"]
    );
    assert!(
        store
            .pop_commands("master:supervisor")
            .await
            .unwrap()
            .is_empty()
    );
    store.processes().orphaned("master", &[7, 8]).await.unwrap();
    let observed = store.processes().all_orphans("master").await.unwrap();
    store.processes().orphaned("master", &[7, 9]).await.unwrap();
    let updated = store.processes().all_orphans("master").await.unwrap();
    assert_eq!(observed.get(&7), updated.get(&7));
    assert!(!updated.contains_key(&8));
    assert!(updated.contains_key(&9));
    store.forget_supervisor("master:supervisor").await.unwrap();
    assert!(
        store
            .supervisor("master:supervisor")
            .await
            .unwrap()
            .is_none()
    );
}

fn retry_config(base: &Path, env: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut values = config(base, env)?;
    let database = base.join("failures.sqlite");
    std::fs::File::create(&database).unwrap();
    let settings = values.get_mut("database").unwrap();
    settings["default"] = json!("sqlite");
    settings["connections"] = json!({"sqlite":{"driver":"sqlite","database":database}});
    values.get_mut("queue").unwrap()["failed"] =
        json!({"driver":"database-uuids","database":"sqlite","table":"failed_jobs"});
    Ok(values)
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_job_types_and_silencing_follow_native_producers_and_raw_retry() {
    use openrust::{
        broadcasting::{BroadcastChannel, Channel},
        contracts::{
            broadcasting::ShouldBroadcast,
            events::{Event, Listener},
        },
        database::eloquent::{Collection, Model, ModelMetadata, ModelState},
        events::{
            Dispatcher, EventContext, EventError, EventFuture, EventResult, ListenerRegistry,
            ListenerResponse, QueuedListenerOptions,
        },
        http::Response,
        mail::{
            MailResult, Mailable,
            mailables::{Content, Envelope},
        },
        notifications::{
            AnonymousNotifiable, Notifiable, Notification as NativeNotification,
            NotificationResult, NotificationRoute, SendQueuedNotifications, messages::MailDelivery,
        },
        queue::{DispatchOptions, JobType, Payload, QueueManager},
        support::facades::{Crypt, Event as EventFacade, Mail, Notification, Queue, Route},
    };
    use openrust_horizon::{JobPayload, Silenced, Tags};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Published {
        item: i64,
    }
    impl Silenced for Published {}
    impl Event for Published {
        fn name(&self) -> &str {
            "payload.published"
        }
    }
    impl ShouldBroadcast for Published {
        fn broadcast_on(&self) -> Vec<BroadcastChannel> {
            vec![BroadcastChannel::Public(Channel {
                name: "news".into(),
            })]
        }
        fn broadcast_queue(&self) -> Option<&str> {
            Some("broadcasts")
        }
    }

    struct AuditListener;
    impl Silenced for AuditListener {}
    impl Listener for AuditListener {
        fn name(&self) -> &'static str {
            "custom.audit-label"
        }

        fn handle<'a>(
            &'a self,
            _: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async { Err(EventError::InvalidPayload) })
        }
    }
    fn listeners(registry: &mut ListenerRegistry) -> EventResult<()> {
        registry.listen_queued::<Published>(
            "payload.published",
            Arc::new(AuditListener),
            QueuedListenerOptions {
                listener_id: "custom.audit".into(),
                queue: Some("events".into()),
                connection: None,
                delay: None,
                after_commit: None,
            },
        )
    }

    struct Receipt;
    impl Mailable for Receipt {
        fn kind(&self) -> &str {
            "custom.receipt"
        }

        fn envelope(&self) -> MailResult<Envelope> {
            Ok(Envelope {
                to: vec!["to@example.org".into()],
                subject: "Receipt".into(),
                ..Default::default()
            })
        }
        fn content(&self) -> MailResult<Content> {
            Ok(Content {
                text: Some("Receipt".into()),
                ..Default::default()
            })
        }
    }
    struct Notice(bool);
    impl NativeNotification for Notice {
        fn via(&self, _: &dyn Notifiable) -> NotificationResult<Vec<String>> {
            Ok(vec!["mail".into()])
        }
        fn to_mail(&self, _: &dyn Notifiable) -> NotificationResult<Option<MailDelivery>> {
            Ok(Some(MailDelivery::Mailable(Box::new(Receipt))))
        }
        fn queue_options(&self, _: &dyn Notifiable, _: &str) -> Option<DispatchOptions> {
            Some(DispatchOptions {
                queue: Some(
                    if self.0 {
                        "encrypted-notices"
                    } else {
                        "notices"
                    }
                    .into(),
                ),
                ..Default::default()
            })
        }
        fn should_encrypt(&self, _: &dyn Notifiable, _: &str) -> bool {
            self.0
        }
        fn queue_middleware(&self, _: &dyn Notifiable, _: &str) -> Option<&str> {
            self.0.then_some("audit")
        }
    }

    struct QuietJob;
    impl Silenced for QuietJob {}
    impl openrust::contracts::queue::JobHandler for QuietJob {
        fn handle<'a>(
            &'a self,
            _: &'a mut openrust::queue::JobContext<'_>,
            _: &'a Payload,
        ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    struct TaggedItem(ModelState);
    impl Model for TaggedItem {
        fn metadata() -> &'static ModelMetadata {
            static METADATA: ModelMetadata = ModelMetadata::new("tagged_items");
            &METADATA
        }
        fn from_state(state: ModelState) -> Self {
            Self(state)
        }
        fn state(&self) -> &ModelState {
            &self.0
        }
        fn state_mut(&mut self) -> &mut ModelState {
            &mut self.0
        }
    }

    fn item_tags(payload: &Payload) -> openrust_horizon::Result<Vec<String>> {
        use openrust::database::Value as Key;

        let items = payload.data()["items"].as_array().unwrap();
        let models = Collection::new(
            items
                .iter()
                .map(|item| {
                    let mut model = TaggedItem(ModelState::default());
                    let key = match item.get("id").unwrap_or(item) {
                        Value::Null => Key::Null,
                        Value::Bool(value) => Key::Bool(*value),
                        Value::String(value) => Key::Text(value.clone()),
                        Value::Number(value) if value.is_i64() => {
                            Key::Integer(value.as_i64().unwrap())
                        }
                        Value::Number(value) => Key::Real(value.as_f64().unwrap()),
                        _ => panic!("unexpected fixture key {item}"),
                    };
                    model.set_attribute("id", key);
                    if let Some(category) = item.get("category").and_then(Value::as_str) {
                        model.set_attribute("category", category);
                    }
                    model
                })
                .collect(),
        );
        Tags::for_models(models.iter())
    }

    fn register_tags() {
        Horizon::tag_for::<AuditListener>(|payload| {
            assert_eq!(payload.data()["item"], 4);
            vec!["listener".into(), "common".into(), "listener".into()]
        })
        .unwrap();
        Horizon::tag_for::<Published>(|payload| {
            if payload.event_type().is_some() {
                assert_eq!(payload.data()["item"], 4);
            }
            vec!["event".into(), "common".into()]
        })
        .unwrap();
        Horizon::tag_for::<Receipt>(|payload| {
            assert_eq!(payload.data()["envelope"]["subject"], "Receipt");
            vec!["mail".into(), "mail".into()]
        })
        .unwrap();
        Horizon::tag_models_for::<Receipt>(|_| panic!("explicit tags should take precedence"))
            .unwrap();
        Horizon::tag_for::<Notice>(|payload| {
            let clear: Value;
            let delivery = if let Some(ciphertext) = payload.data()["encrypted_delivery"].as_str() {
                clear =
                    serde_json::from_slice(&Crypt::decrypt_string(ciphertext).unwrap()).unwrap();
                &clear
            } else {
                payload.data()
            };
            assert_eq!(delivery["delivery"]["channel"], "mail");
            vec!["notification".into()]
        })
        .unwrap();
        Horizon::tag_for::<QuietJob>(|_| Vec::new()).unwrap();
        Horizon::tag_models_for::<QuietJob>(item_tags).unwrap();
        Horizon::tag(|payload| {
            if payload.event_type().is_some() {
                assert_eq!(
                    Tags::extract_explicit_tags(payload).unwrap(),
                    ["listener", "common", "event"]
                );
            }
            vec!["global".into()]
        })
        .unwrap();
    }

    fn dispatch(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let quiet = Payload::new(
                std::any::type_name::<QuietJob>(),
                json!({"items":[4,"4",4,9,null,false,{"id":false,"category":"false"},{"id":true,"category":"true"},1.5,{"id":4,"category":"different"}]}),
            )
            .unwrap();
            assert!(Tags::extract_explicit_tags(&quiet).unwrap().is_empty());
            assert_eq!(
                Tags::model_tags_for(&quiet).unwrap(),
                item_tags(&quiet).unwrap()
            );
            EventFacade::dispatch(Published { item: 4 }).await.unwrap();
            Mail::mailer(None)
                .unwrap()
                .queue(
                    &Receipt,
                    DispatchOptions {
                        queue: Some("mail".into()),
                        ..Default::default()
                    },
                    None,
                )
                .await
                .unwrap();
            let target = AnonymousNotifiable::default()
                .route(
                    "mail",
                    NotificationRoute::Mail(vec!["to@example.org".into()]),
                )
                .unwrap();
            Notification::send(&[&target], &Notice(false))
                .await
                .unwrap();
            Notification::send(&[&target], &Notice(true)).await.unwrap();
            Queue::dispatch(
                Payload::new("mail.send_queued_mailable", json!({})).unwrap(),
                DispatchOptions {
                    queue: Some("ordinary".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
            Queue::dispatch(
                Payload::new(
                    "quiet.job",
                    json!({"items":[4,"4",4,9,null,false,{"id":false,"category":"false"},{"id":true,"category":"true"},1.5,{"id":4,"category":"different"}]}),
                )
                .unwrap(),
                DispatchOptions {
                    queue: Some("quiet".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
            Ok(Response::from("queued"))
        })
    }
    fn fixture_config(
        base: &Path,
        env: &Environment,
    ) -> Result<BTreeMap<String, Value>, ConfigError> {
        let mut config = retry_config(base, env)?;
        config.get_mut("app").unwrap()["key"] = json!("01234567890123456789012345678901");
        config.get_mut("horizon").unwrap()["silenced"] = json!([
            std::any::type_name::<Receipt>(),
            "notifications.send_queued_notifications",
            "notifications.send_queued_notifications.audit",
        ]);
        config.insert(
            "mail".into(),
            json!({
                "default":"array", "from":{"address":"from@example.org","name":"Horizon"},
                "mailers":{"array":{"transport":"array"}}
            }),
        );
        Ok(config)
    }
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(fixture_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Horizon::silence::<Published>().unwrap();
                Horizon::silence::<AuditListener>().unwrap();
                Horizon::silence::<QuietJob>().unwrap();
                register_tags();
                Route::get("/classify", dispatch)
            }))
    })
    .unwrap();
    app.boot().await.unwrap();
    let dispatcher = app.app().make::<Dispatcher>().await.unwrap();
    dispatcher.register_broadcast::<Published>();
    dispatcher.register(listeners).unwrap();
    let manager = app.app().make::<QueueManager>().await.unwrap();
    manager.register("quiet.job", Arc::new(QuietJob)).unwrap();
    SendQueuedNotifications::register_middleware(&manager, "audit", vec![]).unwrap();
    app.app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap()
        .get_schema_builder()
        .create("failed_jobs", |table| {
            table.id();
            table.string("uuid", None).unique();
            table.text("connection");
            table.text("queue");
            table.long_text("payload");
            table.long_text("exception");
            table.timestamp("failed_at", 0);
        })
        .await
        .unwrap();
    app.get("/api/classify").await.unwrap().assert_ok();

    let transport = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let jobs = repository.jobs();
    for (queue, kind, label, silenced) in [
        ("broadcasts", JobType::Broadcast, "broadcast", true),
        ("mail", JobType::Mail, "mail", true),
        ("notices", JobType::Notification, "notification", false),
        (
            "encrypted-notices",
            JobType::Notification,
            "notification",
            false,
        ),
        ("ordinary", JobType::Job, "job", false),
        ("quiet", JobType::Job, "job", true),
    ] {
        let mut job = transport.pop_now(Some(queue)).await.unwrap().unwrap();
        assert_eq!(job.payload().job_type(), kind);
        let snapshot = JobPayload::from_payload(job.payload()).unwrap().unwrap();
        assert_eq!(snapshot.job_type(), kind);
        assert_eq!(snapshot.is_silenced(), silenced);
        let details = jobs.find(job.get_job_id()).await.unwrap().unwrap();
        assert_eq!(details.silenced, silenced);
        assert_eq!(details.payload["redis_metadata"]["horizon"]["type"], label);
        let model_tags = ["4", "9", "", "", "1", "1.5", "4"]
            .map(|id| format!("{}:{id}", std::any::type_name::<TaggedItem>()));
        let expected: &[&str] = match queue {
            "broadcasts" => &["event", "common", "global"],
            "mail" => &["mail", "global"],
            "notices" | "encrypted-notices" => &["notification", "global"],
            "quiet" => &[
                &model_tags[0],
                &model_tags[1],
                &model_tags[2],
                &model_tags[3],
                &model_tags[4],
                &model_tags[5],
                &model_tags[6],
                "global",
            ],
            _ => &["global"],
        };
        assert_eq!(snapshot.tags(), expected);
        assert_eq!(details.tags, expected);
        assert_eq!(
            Tags::target_for(job.payload()),
            Some(job.payload().subject_type().unwrap_or(job.payload().name()))
        );
        if kind == JobType::Notification {
            assert_eq!(
                job.payload().subject_type(),
                Some(std::any::type_name::<Notice>())
            );
        }
        if queue == "encrypted-notices" {
            assert!(job.payload().data()["encrypted_delivery"].is_string());
            assert_eq!(
                job.payload().name(),
                "notifications.send_queued_notifications.audit"
            );
        }
        if queue == "mail" {
            let mut old = details.payload;
            old.as_object_mut().unwrap().remove("job_type");
            old.as_object_mut().unwrap().remove("subject_type");
            old["redis_metadata"]["horizon"]
                .as_object_mut()
                .unwrap()
                .remove("type");
            let old: Payload = serde_json::from_value(old).unwrap();
            assert_eq!(old.job_type(), JobType::Job);
            assert!(old.subject_type().is_none());
            assert_eq!(
                JobPayload::from_payload(&old).unwrap().unwrap().job_type(),
                JobType::Job
            );
        }
        job.delete().await.unwrap();
    }
    app.artisan([
        "horizon:work",
        "redis",
        "--queue=events",
        "--once",
        "--tries=1",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    let failed = jobs.get_failed(None).await.unwrap();
    assert_eq!(failed.len(), 1);
    assert!(failed[0].silenced);
    assert_eq!(failed[0].tags, ["listener", "common", "event", "global"]);
    let failed_payload: Payload = serde_json::from_value(failed[0].payload.clone()).unwrap();
    assert_eq!(
        Tags::target_for(&failed_payload),
        Some(std::any::type_name::<Published>())
    );
    assert_eq!(
        failed[0].payload["redis_metadata"]["horizon"]["type"],
        "event"
    );
    let original = &failed[0].record.id;
    app.artisan(["horizon:retry", original])
        .await
        .assert_successful();
    let retried = transport.pop_now(Some("events")).await.unwrap().unwrap();
    assert_eq!(retried.payload().job_type(), JobType::Event);
    let metadata = JobPayload::from_payload(retried.payload())
        .unwrap()
        .unwrap();
    assert_eq!(metadata.job_type(), JobType::Job);
    assert!(!metadata.is_silenced());
    assert_eq!(metadata.retry_of(), Some(original.as_str()));
    assert_eq!(metadata.tags(), failed[0].tags);

    let mut isolated = TestApp::create(|builder| {
        builder
            .with_config(fixture_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
    })
    .unwrap();
    isolated.boot().await.unwrap();
    let manager = isolated.app().make::<QueueManager>().await.unwrap();
    manager.register("quiet.job", Arc::new(QuietJob)).unwrap();
    let redis = manager.connection(Some("redis")).unwrap();
    let id = redis
        .push(&Payload::new("quiet.job", json!({})).unwrap(), None)
        .await
        .unwrap()
        .unwrap();
    let repository = isolated.app().make::<Repository>().await.unwrap();
    let isolated_job = repository.jobs().find(&id).await.unwrap().unwrap();
    assert!(!isolated_job.silenced);
    assert!(isolated_job.tags.is_empty());
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_public_failure_follows_confirmation_and_worker_failed_callback() {
    use openrust::{
        contracts::{
            container::Container as _,
            events::{Dispatcher as _, Event, Listener},
            foundation::Application as _,
            queue::JobHandler,
        },
        events::{
            Delivery, Dispatcher, EventContext, EventError, EventFuture, EventResult,
            ListenerResponse,
        },
        queue::{
            DispatchOptions, JobContext, Payload, QueueError, QueueFuture, QueueManager,
            QueueResult,
            failed::{FailedJob, FailedJobProviderInterface},
        },
    };
    use openrust_horizon::{JobDeleted, JobFailed};

    struct FailureJob(Arc<std::sync::atomic::AtomicUsize>);
    impl JobHandler for FailureJob {
        fn handle<'a>(
            &'a self,
            _: &'a mut JobContext<'_>,
            _: &'a Payload,
        ) -> QueueFuture<'a, QueueResult<()>> {
            Box::pin(async {
                Err(QueueError::execution(std::io::Error::other(
                    "failure-probe",
                )))
            })
        }
        fn failed<'a>(
            &'a self,
            _: &'a mut JobContext<'_>,
            _: &'a Payload,
            _: &'a QueueError,
        ) -> QueueFuture<'a, QueueResult<()>> {
            Box::pin(async move {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(QueueError::InvalidPayload)
            })
        }
    }

    struct FailureProbe {
        repository: Arc<Repository>,
        store: Arc<dyn FailedJobProviderInterface>,
        queue: Arc<dyn openrust::contracts::queue::Queue>,
        callbacks: Arc<std::sync::atomic::AtomicUsize>,
        observed: std::sync::Mutex<Vec<(&'static str, String, String)>>,
        core_failures: std::sync::Mutex<Vec<String>>,
    }
    impl Listener for FailureProbe {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                if let Some(failed) = event.downcast_ref::<openrust::queue::events::JobFailed>() {
                    self.core_failures
                        .lock()
                        .unwrap()
                        .push(failed.connection_name.clone());
                    return Ok(None);
                }
                if let Some(deleted) = event.downcast_ref::<JobDeleted>() {
                    assert!(deleted.failed);
                    self.observed.lock().unwrap().push((
                        JobDeleted::NAME,
                        deleted.job.id.clone(),
                        String::new(),
                    ));
                    return Ok(None);
                }
                let Some(failed) = event.downcast_ref::<JobFailed>() else {
                    return Ok(None);
                };
                assert_eq!(failed.connection_name, "redis");
                let job = &failed.job;
                assert_eq!(
                    self.queue.metrics(Some(&job.queue)).await.unwrap().reserved,
                    Some(0)
                );
                let details = self
                    .repository
                    .jobs()
                    .find_failed(&job.id)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(details.payload_uuid().unwrap(), job.payload.uuid());
                assert_eq!(details.record.attempts, job.attempts);
                assert_eq!(
                    details.exception.as_deref(),
                    Some(failed.exception.as_str())
                );
                assert!(
                    self.repository
                        .tags()
                        .jobs("failed:tracked")
                        .await
                        .unwrap()
                        .contains(&job.id)
                );
                let persisted = self.store.find(job.payload.uuid()).await.unwrap();
                match job.queue.as_str() {
                    "manual" => assert!(persisted.is_none()),
                    "worker" => {
                        assert_eq!(self.callbacks.load(std::sync::atomic::Ordering::SeqCst), 1);
                        assert_eq!(persisted.unwrap().exception, failed.exception);
                    }
                    "recovery" => assert_eq!(persisted.unwrap().exception, failed.exception),
                    queue => panic!("unexpected failure on {queue}"),
                }
                self.observed.lock().unwrap().push((
                    JobFailed::NAME,
                    job.id.clone(),
                    failed.exception.clone(),
                ));
                Err(EventError::InvalidPayload)
            })
        }
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|base, env| {
                let mut values = retry_config(base, env)?;
                values.get_mut("queue").unwrap()["connections"]["sync"] = json!({"driver":"sync"});
                values.get_mut("queue").unwrap()["connections"]["wrapper"] =
                    json!({"driver":"failover","connections":["redis","sync"]});
                Ok(values)
            })
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(transport_routes))
    })
    .unwrap();
    app.boot().await.unwrap();
    let database = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    database
        .get_schema_builder()
        .create("failed_jobs", |table| {
            table.id();
            table.string("uuid", None).unique();
            table.text("connection");
            table.text("queue");
            table.long_text("payload");
            table.long_text("exception");
            table.timestamp("failed_at", 0);
        })
        .await
        .unwrap();
    let manager = app.app().make::<QueueManager>().await.unwrap();
    let queue = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let store = app
        .app()
        .container()
        .get::<dyn FailedJobProviderInterface>("queue.failer")
        .unwrap();
    let callbacks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    manager
        .register("failure-probe", Arc::new(FailureJob(callbacks.clone())))
        .unwrap();
    let probe = Arc::new(FailureProbe {
        repository: repository.clone(),
        store: store.clone(),
        queue: queue.clone(),
        callbacks,
        observed: Default::default(),
        core_failures: Default::default(),
    });
    let dispatcher = app.app().make::<Dispatcher>().await.unwrap();
    for name in [
        JobDeleted::NAME,
        JobFailed::NAME,
        openrust::queue::events::JobFailed::NAME,
    ] {
        dispatcher
            .listen(name, probe.clone(), Delivery::Sync)
            .unwrap();
    }
    let payload = || Payload::new("failure-probe", json!({})).unwrap();
    let manual_id = queue
        .push(&payload(), Some("manual"))
        .await
        .unwrap()
        .unwrap();
    let mut manual = queue.pop_now(Some("manual")).await.unwrap().unwrap();
    manual.fail(None).await.unwrap();
    manual.fail(None).await.unwrap();
    assert!(manual.is_deleted() && manual.has_failed());
    let worker_id = manager
        .connection(Some("wrapper"))
        .unwrap()
        .push(&payload(), Some("worker"))
        .await
        .unwrap()
        .unwrap();
    app.artisan([
        "horizon:work",
        "wrapper",
        "--queue=worker",
        "--once",
        "--tries=1",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    let restored = payload();
    let recovered_id = queue
        .push(&restored, Some("recovery"))
        .await
        .unwrap()
        .unwrap();
    store
        .log(&FailedJob {
            id: restored.uuid().into(),
            connection: "redis".into(),
            queue: "recovery".into(),
            payload: restored,
            exception: "durable failure".into(),
            failed_at: "2026-01-01 00:00:00".into(),
        })
        .await
        .unwrap();
    app.artisan([
        "horizon:work",
        "redis",
        "--queue=recovery",
        "--once",
        "--tries=1",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    queue
        .push(&payload(), Some("stale"))
        .await
        .unwrap()
        .unwrap();
    let mut stale = queue.pop_now(Some("stale")).await.unwrap().unwrap();
    queue.clearable().unwrap().clear("stale").await.unwrap();
    assert!(stale.fail(None).await.is_err());
    assert!(!stale.is_deleted() && !stale.has_failed());
    assert!(
        manager
            .dispatch(
                payload(),
                DispatchOptions {
                    connection: Some("sync".into()),
                    ..Default::default()
                },
                None
            )
            .await
            .is_err()
    );
    let observed = probe.observed.lock().unwrap();
    assert_eq!(observed.len(), 6);
    for (pair, id) in observed
        .chunks_exact(2)
        .zip([manual_id, worker_id, recovered_id])
    {
        assert_eq!((pair[0].0, pair[1].0), (JobDeleted::NAME, JobFailed::NAME));
        assert_eq!((&pair[0].1, &pair[1].1), (&id, &id));
    }
    assert_eq!(observed[1].2, "任务被手动标记为失败。");
    assert!(observed[3].2.contains("failure-probe"));
    assert_eq!(observed[5].2, "durable failure");
    assert_eq!(*probe.core_failures.lock().unwrap(), ["redis", "sync"]);
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn native_retries_keep_transport_ids_separate_from_the_failed_payload_uuid() {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        queue::{JobOptions, Payload},
    };
    use openrust_horizon::JobPayload;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TAG_CALLS: AtomicUsize = AtomicUsize::new(0);
    fn tags(_: &Payload) -> Vec<String> {
        let call = TAG_CALLS.fetch_add(1, Ordering::SeqCst);
        vec!["quiet".into(), format!("call-{call}")]
    }
    fn snapshot_config(
        base: &Path,
        env: &Environment,
    ) -> Result<BTreeMap<String, Value>, ConfigError> {
        let mut settings = retry_config(base, env)?;
        settings.get_mut("horizon").unwrap()["silenced_tags"] = json!(["quiet"]);
        Ok(settings)
    }
    fn snapshot(details: &openrust_horizon::JobDetails) -> openrust_horizon::JobPayloadMetadata {
        let payload: Payload = serde_json::from_value(details.payload.clone()).unwrap();
        JobPayload::from_payload(&payload).unwrap().unwrap()
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(snapshot_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Horizon::tag(tags).unwrap();
                let task = Payload::new("prepare", json!({})).unwrap();
                let mut raw = JobPayload::new(r#"{"tags":["quiet","manual"]}"#).unwrap();
                raw.prepare(Some(&task)).unwrap();
                assert_eq!(raw.tags(), &json!(["quiet", "manual", "quiet", "call-0"]));
                assert!(raw.is_silenced());
                authorized_routes()
            }))
    })
    .unwrap();
    app.boot().await.unwrap();
    assert_eq!(TAG_CALLS.swap(0, Ordering::SeqCst), 1);
    app.app()
        .config()
        .unwrap()
        .set("horizon.silenced_tags", json!(["preset"]))
        .unwrap();
    let database = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    database
        .get_schema_builder()
        .create("failed_jobs", |table| {
            table.id();
            table.string("uuid", None).unique();
            table.text("connection");
            table.text("queue");
            table.long_text("payload");
            table.long_text("exception");
            table.timestamp("failed_at", 0);
        })
        .await
        .unwrap();
    let manager = app
        .app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap();
    manager.register("fail", Arc::new(Handler("fail"))).unwrap();
    let deadline = std::time::SystemTime::now() + Duration::from_secs(600);
    let payload = Payload::new("fail", json!({"private":"retry-data"}))
        .unwrap()
        .with_options(JobOptions {
            retry_until: Some(deadline),
            max_exceptions: Some(1),
            ..JobOptions::default()
        });
    let payload = JobPayload::with_tags(
        payload,
        vec!["preset".into(), "quiet".into(), "preset".into()],
    );
    let id = manager
        .connection(Some("redis"))
        .unwrap()
        .push(&payload, Some("outside-plan"))
        .await
        .unwrap()
        .unwrap();
    assert_ne!(id, payload.uuid());
    let repository = app.app().make::<Repository>().await.unwrap();
    let jobs = repository.jobs();
    let pending = jobs.find(&id).await.unwrap().unwrap();
    let queued_snapshot = snapshot(&pending);
    assert_eq!(
        queued_snapshot.tags(),
        ["preset", "quiet", "preset", "quiet", "call-0"]
    );
    let raw = JobPayload::new(pending.payload.to_string()).unwrap();
    assert_eq!(raw.id(), Some(payload.uuid()));
    assert_eq!(raw.command_name(), Some("fail"));
    assert_eq!(raw.display_name(), Some("fail"));
    assert_eq!(raw.tags(), &json!(queued_snapshot.tags()));
    assert!(raw.is_silenced());
    assert!(queued_snapshot.is_silenced());
    assert!(queued_snapshot.pushed_at().parse::<f64>().unwrap() > 0.0);
    assert!(!queued_snapshot.is_retry());
    assert_eq!(pending.tags, queued_snapshot.tags());
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 1);
    app.artisan([
        "queue:work",
        "redis",
        "--queue=outside-plan",
        "--once",
        "--tries=1",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    let original = jobs.find_failed(&id).await.unwrap().unwrap();
    assert_eq!(original.payload_uuid().unwrap(), payload.uuid());
    assert_eq!(original.tags, queued_snapshot.tags());
    assert_eq!(snapshot(&original).pushed_at(), queued_snapshot.pushed_at());
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 1);
    assert!(jobs.find(payload.uuid()).await.unwrap().is_none());
    tokio::time::sleep(Duration::from_millis(1100)).await;
    app.artisan(["horizon:retry", &id])
        .await
        .assert_successful();
    let original = jobs.find_failed(&id).await.unwrap().unwrap();
    let retry = original.retried_by[0]["id"].as_str().unwrap().to_owned();
    assert_ne!(retry, id);
    assert_ne!(retry, payload.uuid());
    let retried = jobs.find(&retry).await.unwrap().unwrap();
    assert_eq!(retried.payload_uuid().unwrap(), payload.uuid());
    let retry_snapshot = snapshot(&retried);
    assert_eq!(retry_snapshot.retry_of(), Some(id.as_str()));
    assert!(retry_snapshot.is_retry());
    assert_eq!(retry_snapshot.tags(), queued_snapshot.tags());
    assert!(!retry_snapshot.is_silenced());
    assert!(!retried.silenced);
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 1);
    let retried_payload: Payload = serde_json::from_value(retried.payload).unwrap();
    assert!(retried_payload.options().retry_until.unwrap() > deadline);
    app.artisan([
        "queue:work",
        "redis",
        "--queue=outside-plan",
        "--once",
        "--tries=1",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    assert_eq!(
        jobs.find_failed(&id).await.unwrap().unwrap().retried_by[0]["status"],
        "failed"
    );
    assert_eq!(
        jobs.find_failed_by_uuid(payload.uuid())
            .await
            .unwrap()
            .unwrap()
            .record
            .id,
        retry
    );
    assert_eq!(jobs.count_failed().await.unwrap(), 2);
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 1);
    // Removing the historical parent must not erase the retry's native failure.
    app.artisan(["horizon:forget", &id])
        .await
        .assert_successful();
    let failures = app
        .app()
        .container()
        .get::<dyn openrust::queue::failed::FailedJobProviderInterface>("queue.failer")
        .unwrap();
    assert!(failures.find(payload.uuid()).await.unwrap().is_some());
    assert!(jobs.find_failed(&retry).await.unwrap().is_some());
    app.artisan(["horizon:forget", &retry])
        .await
        .assert_successful();
    assert!(failures.find(payload.uuid()).await.unwrap().is_none());
    assert_eq!(jobs.count_failed().await.unwrap(), 0);
    assert!(repository.recent(100).await.unwrap().is_empty());

    app.app()
        .config()
        .unwrap()
        .set("horizon.silenced_tags", json!(["quiet"]))
        .unwrap();
    manager
        .register("success", Arc::new(Handler("success")))
        .unwrap();
    let success = Payload::new("success", json!({})).unwrap();
    let queue = manager.connection(Some("redis")).unwrap();
    let quiet = queue.push(&success, Some("quiet")).await.unwrap().unwrap();
    assert!(snapshot(&jobs.find(&quiet).await.unwrap().unwrap()).is_silenced());
    app.app()
        .config()
        .unwrap()
        .set("horizon.silenced_tags", json!([]))
        .unwrap();
    app.artisan([
        "horizon:work",
        "redis",
        "--queue=quiet",
        "--once",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    let silenced = jobs.get_silenced(None).await.unwrap();
    assert_eq!(silenced.len(), 1);
    assert_eq!(silenced[0].record.id, quiet);
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), 2);

    // 旧版 Redis 投递不含扩展元数据，仍可由当前 Worker 保留并完成。
    let redis = app
        .app()
        .make::<openrust::redis::RedisManager>()
        .await
        .unwrap();
    let redis = redis.connection(None).unwrap();
    let raw = json!({
        "id": "legacy-delivery",
        "payload": serde_json::to_string(&success).unwrap(),
        "attempts": 0
    })
    .to_string();
    let key = redis.key("queues:legacy");
    redis
        .command("RPUSH", &[key.as_bytes(), raw.as_bytes()])
        .await
        .unwrap();
    app.artisan([
        "horizon:work",
        "redis",
        "--queue=legacy",
        "--once",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    let legacy = jobs.find("legacy-delivery").await.unwrap().unwrap();
    assert_eq!(legacy.record.status, "completed");
    assert!(!legacy.silenced);
    let legacy_payload: Payload = serde_json::from_value(legacy.payload).unwrap();
    assert!(JobPayload::from_payload(&legacy_payload).unwrap().is_none());

    let old_failure = Payload::new("fail", json!({})).unwrap();
    let raw = json!({
        "id": "legacy-failure",
        "payload": serde_json::to_string(&old_failure).unwrap(),
        "attempts": 0
    })
    .to_string();
    redis
        .command("RPUSH", &[key.as_bytes(), raw.as_bytes()])
        .await
        .unwrap();
    app.artisan([
        "horizon:work",
        "redis",
        "--queue=legacy",
        "--once",
        "--tries=1",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    let tag_calls = TAG_CALLS.load(Ordering::SeqCst);
    let request = http::Request::builder()
        .method("POST")
        .uri("/api/horizon/api/jobs/retry/legacy-failure")
        .header("x-test-operator", "operator")
        .header("x-horizon-request", "1")
        .body(openrust::http::Body::empty())
        .unwrap();
    let response = app.call(request).await.unwrap();
    response.assert_ok();
    assert_eq!(
        serde_json::from_slice::<Value>(response.body()).unwrap(),
        json!({"retried":1})
    );
    let legacy_failure = jobs.find_failed("legacy-failure").await.unwrap().unwrap();
    let new_id = legacy_failure.retried_by[0]["id"].as_str().unwrap();
    let metadata = snapshot(&jobs.find(new_id).await.unwrap().unwrap());
    assert_eq!(metadata.retry_of(), Some("legacy-failure"));
    assert_eq!(
        metadata.tags(),
        ["quiet".to_owned(), format!("call-{tag_calls}")]
    );
    assert_eq!(TAG_CALLS.load(Ordering::SeqCst), tag_calls + 1);
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_workload_preserves_grouped_waits_order_and_queue_keys() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(authorized_routes))
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app
        .app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap();
    let queue = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    for (name, runtime) in [
        ("high", 1200.),
        ("low", 1200.),
        ("standalone", 2400.),
        ("0", 1200.),
        ("1", 1200.),
        ("01", 1200.),
        ("-1", 1200.),
    ] {
        let payload = openrust::queue::Payload::new("success", json!({})).unwrap();
        queue.push(&payload, Some(name)).await.unwrap().unwrap();
        repository
            .metrics_repository()
            .increment_queue(&format!("redis:{name}"), runtime)
            .await
            .unwrap();
    }
    let delayed = openrust::queue::Payload::new("success", json!({})).unwrap();
    queue
        .later(
            openrust::queue::Delay::Until(std::time::SystemTime::now() + Duration::from_secs(120)),
            &delayed,
            Some("high"),
        )
        .await
        .unwrap()
        .unwrap();
    for name in ["master:first", "master:second"] {
        repository
            .heartbeat_supervisor(
                name,
                &json!({
                    "name":name,"pid":1,"status":"running","processes":{
                        "redis:high,low,high":1,"redis:low,low":1,"redis:standalone":0,
                        "redis:0,1":1,"redis:1,0":1,"redis:01,-1":1
                    }
                }),
            )
            .await
            .unwrap();
    }
    let response = app
        .call(
            http::Request::builder()
                .uri("/api/horizon/api/workload")
                .header("x-test-operator", "operator")
                .body(openrust::http::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response.assert_ok();
    let workload: Value = serde_json::from_slice(response.body()).unwrap();
    assert_eq!(
        workload,
        json!([
            {"name":"0,1","connection":"redis","length":2,"wait":1,"processes":2,"split_queues":[
                {"name":0,"length":1,"wait":1},{"name":1,"length":1,"wait":2}
            ]},
            {"name":"01,-1","connection":"redis","length":2,"wait":1,"processes":2,"split_queues":{
                "01":{"name":"01","length":1,"wait":1},"-1":{"name":-1,"length":1,"wait":2}
            }},
            {"name":"1,0","connection":"redis","length":2,"wait":1,"processes":2,"split_queues":{
                "1":{"name":1,"length":1,"wait":1},"0":{"name":0,"length":1,"wait":2}
            }},
            {"name":"high,low,high","connection":"redis","length":2,"wait":2,"processes":2,"split_queues":{
                "high":{"name":"high","length":1,"wait":1},"low":{"name":"low","length":1,"wait":2}
            }},
            {"name":"low,low","connection":"redis","length":1,"wait":1,"processes":2,"split_queues":{
                "low":{"name":"low","length":1,"wait":1}
            }},
            {"name":"standalone","connection":"redis","length":1,"wait":2,"processes":0,"split_queues":null}
        ])
    );
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_jobs_outside_the_deployment_plan_keep_pending_and_lifecycle_metadata() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(authorized_routes))
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app
        .app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap();
    manager
        .register("success", Arc::new(Handler("success")))
        .unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let payload = openrust::queue::Payload::new("success", json!({})).unwrap();
    let id = manager
        .connection(Some("redis"))
        .unwrap()
        .push(&payload, Some("outside-plan"))
        .await
        .unwrap()
        .unwrap();
    let pending = repository.jobs().find(&id).await.unwrap().unwrap();
    assert_eq!(pending.record.queue, "outside-plan");
    assert_eq!(pending.record.status, "pending");
    repository.heartbeat_supervisor("master:outside", &json!({"name":"master:outside","pid":1,"status":"running","processes":{"redis:outside-plan":1,"redis:default":1},"options":{"balance":"off"}})).await.unwrap();
    let response = app
        .call(
            http::Request::builder()
                .uri("/api/horizon/api/stats")
                .header("x-test-operator", "operator")
                .body(openrust::http::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    response.assert_ok();
    let stats: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
    let queues = stats["queues"].as_array().unwrap();
    assert_eq!(
        queues
            .iter()
            .filter(|queue| queue["queue"] == "outside-plan")
            .count(),
        1
    );
    assert_eq!(
        queues
            .iter()
            .filter(|queue| queue["queue"] == "default")
            .count(),
        1
    );

    app.artisan([
        "queue:work",
        "redis",
        "--queue=outside-plan",
        "--once",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    let completed = repository.jobs().find(&id).await.unwrap().unwrap();
    assert_eq!(completed.record.status, "completed");
    assert_eq!(
        repository
            .metrics("redis", "outside-plan")
            .await
            .unwrap()
            .completed,
        1
    );
}

struct PausedFailures {
    row: std::sync::Mutex<Option<openrust::queue::failed::FailedJob>>,
    pause: std::sync::atomic::AtomicBool,
    snapshot_before_pause: bool,
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
    unconditional_deletes: std::sync::atomic::AtomicUsize,
    conditional_deletes: std::sync::atomic::AtomicUsize,
}
impl openrust::queue::failed::FailedJobProviderInterface for PausedFailures {
    fn log<'a>(
        &'a self,
        job: &'a openrust::queue::failed::FailedJob,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<Option<String>>> {
        Box::pin(async move {
            *self.row.lock().unwrap() = Some(job.clone());
            Ok(Some(job.id.clone()))
        })
    }
    fn all(
        &self,
    ) -> openrust::queue::QueueFuture<
        '_,
        openrust::queue::QueueResult<Vec<openrust::queue::failed::FailedJob>>,
    > {
        Box::pin(async move { Ok(self.row.lock().unwrap().clone().into_iter().collect()) })
    }
    fn find<'a>(
        &'a self,
        _: &'a str,
    ) -> openrust::queue::QueueFuture<
        'a,
        openrust::queue::QueueResult<Option<openrust::queue::failed::FailedJob>>,
    > {
        Box::pin(async move {
            let snapshot = self.row.lock().unwrap().clone();
            if self.pause.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            Ok(if self.snapshot_before_pause {
                snapshot
            } else {
                self.row.lock().unwrap().clone()
            })
        })
    }
    fn forget<'a>(
        &'a self,
        _: &'a str,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<bool>> {
        Box::pin(async move {
            self.unconditional_deletes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.row.lock().unwrap().take().is_some())
        })
    }
    fn forget_retry<'a>(
        &'a self,
        snapshot: &'a openrust::queue::failed::FailedJob,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<bool>> {
        Box::pin(async move {
            self.conditional_deletes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut row = self.row.lock().unwrap();
            if row.as_ref().is_some_and(|current| {
                serde_json::to_value(current).unwrap() == serde_json::to_value(snapshot).unwrap()
            }) {
                row.take();
                Ok(true)
            } else {
                Ok(false)
            }
        })
    }
    fn flush(
        &self,
        _: Option<Duration>,
    ) -> openrust::queue::QueueFuture<'_, openrust::queue::QueueResult<()>> {
        Box::pin(async move {
            self.row.lock().unwrap().take();
            Ok(())
        })
    }
}
#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn native_failure_replacement_during_lookup_preserves_the_new_delivery() {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        queue::failed::{FailedJob, FailedJobProviderInterface as _},
    };
    for operation in ["forget", "console-retry", "dashboard-retry"] {
        let mut app = TestApp::create(|builder| {
            builder
                .with_config(config)
                .with_providers(vec![Box::new(HorizonServiceProvider)])
                .with_routing(Routing::default().api(authorized_routes))
        })
        .unwrap();
        app.boot().await.unwrap();
        let payload = openrust::queue::Payload::new("fail", json!({"attempt":"original"})).unwrap();
        let mut changed = serde_json::to_value(&payload).unwrap();
        changed["delivery_id"] = json!(uuid::Uuid::now_v7().to_string());
        let replacement: openrust::queue::Payload = serde_json::from_value(changed).unwrap();
        assert_eq!(payload.uuid(), replacement.uuid());
        let original_id = uuid::Uuid::now_v7().to_string();
        let replacement_id = uuid::Uuid::now_v7().to_string();
        let native = FailedJob {
            id: payload.uuid().into(),
            connection: "redis".into(),
            queue: "default".into(),
            payload: payload.clone(),
            exception: "old failure".into(),
            failed_at: "2026-11-21 12:00:00".into(),
        };
        let replacement_native = FailedJob {
            payload: replacement.clone(),
            exception: "new failure".into(),
            ..native.clone()
        };
        let provider = Arc::new(PausedFailures {
            row: std::sync::Mutex::new(Some(native)),
            pause: std::sync::atomic::AtomicBool::new(true),
            snapshot_before_pause: operation == "forget",
            entered: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
            unconditional_deletes: std::sync::atomic::AtomicUsize::new(0),
            conditional_deletes: std::sync::atomic::AtomicUsize::new(0),
        });
        app.app()
            .container()
            .instance_named::<dyn openrust::queue::failed::FailedJobProviderInterface>(
                "queue.failer",
                provider.clone(),
            )
            .unwrap();
        let repository = app.app().make::<Repository>().await.unwrap();
        let jobs = repository.jobs();
        jobs.failed(
            "old failure",
            &original_id,
            "redis",
            "default",
            &payload,
            &[],
        )
        .await
        .unwrap();
        let request = http::Request::builder()
            .method("POST")
            .uri(format!("/api/horizon/api/jobs/retry/{original_id}"))
            .header("x-test-operator", "operator")
            .header("x-horizon-request", "1")
            .body(openrust::http::Body::empty())
            .unwrap();
        let action = async {
            match operation {
                "forget" => {
                    app.artisan(["horizon:forget", &original_id])
                        .await
                        .assert_successful();
                }
                "console-retry" => {
                    app.artisan(["horizon:retry", &original_id])
                        .await
                        .assert_exit_code(1);
                }
                _ => {
                    let response = app.call(request).await.unwrap();
                    response.assert_ok();
                    assert_eq!(
                        serde_json::from_slice::<serde_json::Value>(response.body()).unwrap(),
                        json!({"retried":0})
                    );
                }
            }
        };
        let replace_during_lookup = async {
            provider.entered.notified().await;
            provider.log(&replacement_native).await.unwrap();
            if operation == "forget" {
                jobs.failed(
                    "new failure",
                    &replacement_id,
                    "redis",
                    "default",
                    &replacement,
                    &[],
                )
                .await
                .unwrap();
            }
            provider.resume.notify_one();
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(action, replace_during_lookup);
        })
        .await
        .unwrap();
        let retained = provider.row.lock().unwrap().clone().unwrap();
        assert_eq!(
            serde_json::to_value(retained.payload).unwrap(),
            serde_json::to_value(&replacement).unwrap()
        );
        assert_eq!(
            provider
                .unconditional_deletes
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert_eq!(
            provider
                .conditional_deletes
                .load(std::sync::atomic::Ordering::SeqCst),
            usize::from(operation == "forget")
        );
        assert_eq!(jobs.count_pending().await.unwrap(), 0);
        if operation == "forget" {
            assert!(jobs.find_failed(&original_id).await.unwrap().is_none());
            assert_eq!(
                jobs.find_failed_by_uuid(payload.uuid())
                    .await
                    .unwrap()
                    .unwrap()
                    .record
                    .id,
                replacement_id
            );
        } else {
            assert!(
                jobs.find_failed(&original_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .retried_by
                    .is_empty()
            );
        }
    }
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_chain_publishes_successor_events_at_atomic_queue_boundary() {
    use openrust::{
        bus::Bus,
        contracts::events::{Dispatcher as _, Event, Listener},
        events::{
            Delivery, Dispatcher, EventContext, EventError, EventFuture, EventResult,
            ListenerResponse,
        },
        http::Response,
        queue::{Delay, Payload, QueueError, QueueManager},
        support::facades::Route,
    };
    use openrust_horizon::{JobDeleted, JobPending, JobPushed};

    fn dispatch_chain(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let head = Payload::new("head", json!({})).unwrap();
            let next = Payload::new("next", json!({"serial": 9_007_199_254_740_993_u64}))
                .unwrap()
                .on_queue(" 后继:发货")
                .delay(Delay::For(Duration::from_secs(90)));
            Bus::chain([head, next])
                .on_queue("source")
                .dispatch()
                .await?;
            Response::json(http::StatusCode::OK, json!({"queued": true}))
        })
    }

    struct RejectCertainEvents;
    impl Listener for RejectCertainEvents {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                if event
                    .downcast_ref::<JobPending>()
                    .is_some_and(|event| event.queue == "pre-error")
                    || event
                        .downcast_ref::<JobPushed>()
                        .is_some_and(|event| event.queue == "post-error")
                {
                    return Err(EventError::InvalidPayload);
                }
                Ok(None)
            })
        }
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(transport_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(|| {
                transport_routes()?;
                Route::get("/dispatch-chain", dispatch_chain)
            }))
    })
    .unwrap();
    app.boot().await.unwrap();
    let manager = app.app().make::<QueueManager>().await.unwrap();
    manager.register("head", Arc::new(Handler("head"))).unwrap();
    let queue = manager.connection(Some("redis")).unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    repository.tags().monitor("tracked").await.unwrap();
    let listener = Arc::new(TransportListener {
        queue: queue.clone(),
        repository,
        observations: Default::default(),
        fail_settlement_listeners: false,
        fail_reservation_listener: false,
    });
    let dispatcher = app.app().make::<Dispatcher>().await.unwrap();
    for name in [JobPending::NAME, JobPushed::NAME, JobDeleted::NAME] {
        dispatcher
            .listen(name, listener.clone(), Delivery::Sync)
            .unwrap();
    }
    for name in [JobPending::NAME, JobPushed::NAME] {
        dispatcher
            .listen(name, Arc::new(RejectCertainEvents), Delivery::Sync)
            .unwrap();
    }

    app.get("/api/dispatch-chain").await.unwrap().assert_ok();
    app.artisan([
        "horizon:work",
        "redis",
        "--queue=source",
        "--once",
        "--force",
        "--memory=1024",
    ])
    .await
    .assert_successful();
    {
        let rows = listener.observations.lock().unwrap();
        let [
            head_pending,
            head_pushed,
            next_pending,
            next_pushed,
            head_deleted,
        ] = rows.as_slice()
        else {
            panic!("unexpected chain events: {rows:?}");
        };
        assert_eq!(head_pending.id, head_pushed.id);
        assert_eq!(head_pushed.id, head_deleted.id);
        assert_eq!(next_pending.id, next_pushed.id);
        assert_eq!(next_pending.uuid, next_pushed.uuid);
        assert_ne!(next_pending.id, head_pending.id);
        assert_eq!(
            (&*next_pending.name, &*next_pending.queue),
            ("JobPending", " 后继:发货")
        );
        assert_eq!(next_pending.counts, Some((0, 0, 0)));
        assert!(!next_pending.monitored);
        assert_eq!(
            (&*next_pushed.name, &*next_pushed.queue),
            ("JobPushed", " 后继:发货")
        );
        assert_eq!(next_pushed.counts, Some((0, 1, 0)));
        assert!(next_pushed.monitored);
        assert_eq!(
            (&*head_deleted.name, &*head_deleted.status),
            ("JobDeleted", "completed")
        );
        assert_eq!(head_deleted.counts, Some((0, 0, 0)));
    }

    let head = Payload::new("head", json!({})).unwrap();
    let next = Payload::new("next", json!({"serial": 9_007_199_254_740_993_u64})).unwrap();
    queue.push(&head, Some("direct")).await.unwrap();
    let mut reserved = queue.pop_now(Some("direct")).await.unwrap().unwrap();
    listener.observations.lock().unwrap().clear();
    assert!(
        reserved
            .advance_chain_to(&next, Some("pre-error"), None)
            .await
            .is_err()
    );
    assert!(!reserved.is_deleted());
    assert_eq!(
        queue.metrics(Some("direct")).await.unwrap().reserved,
        Some(1)
    );
    assert_eq!(
        queue.metrics(Some("pre-error")).await.unwrap().pending,
        Some(0)
    );
    reserved
        .advance_chain_to(&next, Some("post-error"), None)
        .await
        .unwrap();
    assert!(reserved.is_deleted());
    assert!(matches!(
        reserved.advance_chain(&next).await,
        Err(QueueError::LostReservation)
    ));
    let successor = queue.pop_now(Some("post-error")).await.unwrap().unwrap();
    assert_eq!(successor.payload().uuid(), next.uuid());
    assert_eq!(
        successor.payload().data()["serial"],
        9_007_199_254_740_993_u64
    );

    queue.push(&head, Some("stale-chain")).await.unwrap();
    let mut stale = queue.pop_now(Some("stale-chain")).await.unwrap().unwrap();
    queue
        .clearable()
        .unwrap()
        .clear("stale-chain")
        .await
        .unwrap();
    let before = listener.observations.lock().unwrap().len();
    assert!(matches!(
        stale
            .advance_chain_to(&next, Some("invalid:reserved"), None)
            .await,
        Err(QueueError::InvalidConfiguration)
    ));
    assert_eq!(listener.observations.lock().unwrap().len(), before);
    assert!(matches!(
        stale.advance_chain_to(&next, Some("lost"), None).await,
        Err(QueueError::LostReservation)
    ));
    assert!(!stale.is_deleted());
    assert_eq!(queue.metrics(Some("lost")).await.unwrap().pending, Some(0));

    let rows = listener.observations.lock().unwrap();
    assert_eq!(rows[0].name, "JobPending");
    assert_eq!(rows[0].queue, "pre-error");
    assert_eq!(rows[1].name, "JobPending");
    assert_eq!(rows[1].queue, "post-error");
    assert_eq!(rows[2].name, "JobPushed");
    assert_eq!(rows[2].queue, "post-error");
    assert_eq!(rows[2].id, successor.get_job_id());
    assert_eq!(rows[3].name, "JobDeleted");
    assert_eq!(rows[3].queue, "direct");
    assert_eq!(rows.len(), before + 1);
    assert_eq!(rows[before].name, "JobPending");
    assert_eq!(rows[before].queue, "lost");
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn failover_selects_deferred_target_before_commit_and_surfaces_its_write_error() {
    use openrust::{
        events::Dispatcher,
        queue::{
            Delay, DispatchOptions, DispatchResult, Payload, QueueManager, events::QueueFailedOver,
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    static CALLS: AtomicUsize = AtomicUsize::new(0);

    fn fixture_config(
        base: &Path,
        env: &Environment,
    ) -> Result<BTreeMap<String, Value>, ConfigError> {
        let mut settings = transport_config(base, env)?;
        let endpoint = url();
        let server = endpoint.strip_suffix("/0").unwrap();
        settings.get_mut("database").unwrap()["redis"]["secondary"] =
            json!({"url": format!("{server}/1")});
        let connections = &mut settings.get_mut("queue").unwrap()["connections"];
        connections["backup"] = json!({
            "driver":"redis", "connection":"secondary", "queue":"default", "retry_after":90, "after_commit":true
        });
        connections["fallback"] = json!({
            "driver":"failover", "connections":["redis", "backup"]
        });
        connections["missing-primary"] = json!({
            "driver":"failover", "connections":["absent", "backup"]
        });
        Ok(settings)
    }

    let mut app = TestApp::create(|builder| {
        builder
            .with_config(fixture_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(|| {
                Horizon::tag(|_| {
                    vec![format!("snapshot-{}", CALLS.fetch_add(1, Ordering::SeqCst))]
                })
                .unwrap();
                Ok(())
            }))
    })
    .unwrap();
    app.boot().await.unwrap();

    let manager = app.app().make::<QueueManager>().await.unwrap();
    let redis = app
        .app()
        .make::<openrust::redis::RedisManager>()
        .await
        .unwrap();
    let primary = redis.connection(None).unwrap();
    let backup = manager.connection(Some("backup")).unwrap();
    let database = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    let repository = app.app().make::<Repository>().await.unwrap();
    let jobs = repository.jobs();
    let dispatcher = app.app().make::<Dispatcher>().await.unwrap();
    let events = dispatcher.fake(&[QueueFailedOver::NAME]);
    let payload = Payload::new("failover-commit-probe", json!({})).unwrap();
    let options = DispatchOptions {
        connection: Some("fallback".into()),
        queue: Some("commit-failover".into()),
        delay: Some(Delay::For(Duration::from_secs(90))),
        after_commit: Some(true),
    };

    let mut transaction = database.begin_transaction().await.unwrap();
    assert_eq!(
        manager
            .dispatch(payload.clone(), options.clone(), Some(&mut transaction))
            .await
            .unwrap(),
        DispatchResult::Deferred,
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert!(jobs.get_pending(None).await.unwrap().is_empty());
    transaction.roll_back(None).await.unwrap();
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);

    primary
        .set("queues:commit-failover:delayed", b"wrong redis key type")
        .await
        .unwrap();
    let mut transaction = database.begin_transaction().await.unwrap();
    assert_eq!(
        manager
            .dispatch(payload.clone(), options.clone(), Some(&mut transaction))
            .await
            .unwrap(),
        DispatchResult::Deferred,
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    assert!(jobs.get_pending(None).await.unwrap().is_empty());
    assert!(matches!(
        transaction.commit().await.unwrap_err(),
        openrust::database::DatabaseError::AfterCommit { .. },
    ));
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(
        backup
            .metrics(Some("commit-failover"))
            .await
            .unwrap()
            .delayed,
        Some(0)
    );
    assert!(events.dispatched(QueueFailedOver::NAME).is_empty());

    let mut transaction = database.begin_transaction().await.unwrap();
    assert_eq!(
        manager
            .dispatch(
                payload,
                DispatchOptions {
                    connection: Some("missing-primary".into()),
                    queue: Some("second-choice".into()),
                    after_commit: None,
                    ..options
                },
                Some(&mut transaction)
            )
            .await
            .unwrap(),
        DispatchResult::Deferred,
    );
    assert_eq!(CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(events.dispatched(QueueFailedOver::NAME).len(), 1);
    assert_eq!(
        backup.metrics(Some("second-choice")).await.unwrap().delayed,
        Some(0)
    );
    transaction.commit().await.unwrap();
    assert_eq!(CALLS.load(Ordering::SeqCst), 3);
    assert_eq!(
        backup.metrics(Some("second-choice")).await.unwrap().delayed,
        Some(1)
    );
    let rows = jobs.get_pending(None).await.unwrap();
    let retried = rows
        .iter()
        .find(|job| job.record.queue == "second-choice")
        .unwrap();
    assert_eq!(retried.record.connection, "backup");
    assert_eq!(retried.tags, ["snapshot-2"]);
}
