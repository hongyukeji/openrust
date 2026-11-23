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
        .assertStatus(http::StatusCode::FORBIDDEN);
    authorized
        .get("/api/horizon/api/stats")
        .await
        .unwrap()
        .assertStatus(http::StatusCode::FORBIDDEN);
    let request = http::Request::builder()
        .uri("/api/horizon")
        .header("x-test-operator", "operator")
        .body(openrust::http::Body::empty())
        .unwrap();
    let response = authorized.call(request).await.unwrap();
    response
        .assertOk()
        .assertHeader("cache-control", "no-store");
    assert!(String::from_utf8_lossy(response.body()).contains("Recent attempts"));
    denied
        .get("/api/horizon")
        .await
        .unwrap()
        .assertStatus(http::StatusCode::FORBIDDEN);
    denied.artisan(["help", "horizon"]).await.assertSuccessful();
    denied
        .artisan(["vendor:publish", "--tag=horizon-config"])
        .await
        .assertSuccessful();
    let path = denied.root().join("config/horizon.rs");
    assert!(path.exists());
    std::fs::write(&path, "// customization").unwrap();
    denied
        .artisan(["vendor:publish", "--tag=horizon-config"])
        .await
        .assertSuccessful();
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
    assert_eq!(metrics.runtimeForQueue(), 12.5);
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
            .runtimeForQueue(),
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
            .assertSuccessful();
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
            .assertStatus(http::StatusCode::FORBIDDEN);
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
    assert_eq!(store.jobs().countPending().await.unwrap(), 1);
    store
        .recordPayload(item.clone(), "processing", &payload, &tags, false, None)
        .await
        .unwrap();
    store
        .recordPayload(item.clone(), "released", &payload, &tags, false, None)
        .await
        .unwrap();
    store
        .recordPayload(item.clone(), "finalize", &payload, &tags, false, None)
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
    assert_eq!(store.jobs().countCompleted().await.unwrap(), 0);
    item.attempts = 2;
    store
        .recordPayload(item.clone(), "processing", &payload, &tags, false, None)
        .await
        .unwrap();
    store
        .recordPayload(
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
        .recordPayload(
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
            .findFailed(&item.id)
            .await
            .unwrap()
            .unwrap()
            .exception
            .as_deref(),
        Some("operator-only exception")
    );
    assert_eq!(store.jobs().countFailed().await.unwrap(), 1);
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
        .storeRetryReference(&item.id, "retry-id")
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
    assert!(store.jobs().deleteFailed(&item.id).await.unwrap());
    assert!(!store.jobs().deleteFailed(&item.id).await.unwrap());
    assert_eq!(store.tags().count(&tags[0]).await.unwrap(), 0);
    item.attempts = 1;
    store
        .recordPayload(item.clone(), "processing", &payload, &tags, true, None)
        .await
        .unwrap();
    store
        .recordPayload(item.clone(), "finalize", &payload, &tags, true, None)
        .await
        .unwrap();
    store
        .recordPayload(item.clone(), "finalize", &payload, &tags, true, None)
        .await
        .unwrap();
    assert_eq!(store.jobs().countSilenced().await.unwrap(), 1);
    assert_eq!(store.jobs().countCompleted().await.unwrap(), 0);
    let metrics = store.metricsRepository();
    assert_eq!(metrics.throughputForJob("probe").await.unwrap(), 1);
    assert_eq!(metrics.runtimeForJob("probe").await.unwrap(), 12.5);
    metrics.snapshot().await.unwrap();
    assert_eq!(metrics.throughputForJob("probe").await.unwrap(), 0);
    assert_eq!(
        metrics.snapshotsForJob("probe").await.unwrap()[0]["throughput"],
        1
    );
    for _ in 0..30 {
        metrics.incrementQueue("redis:default", 5.).await.unwrap();
        metrics.snapshot().await.unwrap();
    }
    assert!(
        metrics
            .snapshotsForQueue("redis:default")
            .await
            .unwrap()
            .len()
            <= 24
    );
    metrics.clear().await.unwrap();
    assert!(metrics.measuredQueues().await.unwrap().is_empty());
    assert_eq!(store.metrics("redis", "default").await.unwrap().attempts, 0);
    assert!(store.jobs().find(&item.id).await.unwrap().is_some());
    store
        .heartbeatSupervisor(
            "master:supervisor",
            &json!({"options":{"timeout":70},"processes":{"redis:default":2}}),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .supervisorRepository()
            .longestActiveTimeout()
            .await
            .unwrap(),
        70
    );
    store.pauseSupervisor("master:supervisor").await.unwrap();
    assert!(store.supervisorPaused("master:supervisor").await.unwrap());
    assert!(
        store
            .popCommands("master:supervisor")
            .await
            .unwrap()
            .is_empty()
    );
    store
        .pushCommand("master:supervisor", "terminate")
        .await
        .unwrap();
    assert_eq!(
        store.popCommands("master:supervisor").await.unwrap(),
        vec!["terminate"]
    );
    assert!(
        store
            .popCommands("master:supervisor")
            .await
            .unwrap()
            .is_empty()
    );
    store.processes().orphaned("master", &[7, 8]).await.unwrap();
    let observed = store.processes().allOrphans("master").await.unwrap();
    store.processes().orphaned("master", &[7, 9]).await.unwrap();
    let updated = store.processes().allOrphans("master").await.unwrap();
    assert_eq!(observed.get(&7), updated.get(&7));
    assert!(!updated.contains_key(&8));
    assert!(updated.contains_key(&9));
    store.forgetSupervisor("master:supervisor").await.unwrap();
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
async fn native_retries_keep_transport_ids_separate_from_the_failed_payload_uuid() {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(retry_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
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
        .getSchemaBuilder()
        .create("failed_jobs", |table| {
            table.id();
            table.string("uuid", None).unique();
            table.text("connection");
            table.text("queue");
            table.longText("payload");
            table.longText("exception");
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
    let payload = openrust::queue::Payload::new("fail", json!({"private":"retry-data"})).unwrap();
    let id = manager
        .connection(Some("redis"))
        .unwrap()
        .push(&payload, Some("outside-plan"))
        .await
        .unwrap()
        .unwrap();
    assert_ne!(id, payload.uuid());
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
    .assertSuccessful();
    let repository = app.app().make::<Repository>().await.unwrap();
    let jobs = repository.jobs();
    let original = jobs.findFailed(&id).await.unwrap().unwrap();
    assert_eq!(original.payloadUuid().unwrap(), payload.uuid());
    assert!(jobs.find(payload.uuid()).await.unwrap().is_none());
    app.artisan(["horizon:retry", &id]).await.assertSuccessful();
    let original = jobs.findFailed(&id).await.unwrap().unwrap();
    let retry = original.retried_by[0]["id"].as_str().unwrap().to_owned();
    assert_ne!(retry, id);
    assert_ne!(retry, payload.uuid());
    assert_eq!(
        jobs.find(&retry)
            .await
            .unwrap()
            .unwrap()
            .payloadUuid()
            .unwrap(),
        payload.uuid()
    );
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
    .assertSuccessful();
    assert_eq!(
        jobs.findFailed(&id).await.unwrap().unwrap().retried_by[0]["status"],
        "failed"
    );
    assert_eq!(
        jobs.findFailedByUuid(payload.uuid())
            .await
            .unwrap()
            .unwrap()
            .record
            .id,
        retry
    );
    assert_eq!(jobs.countFailed().await.unwrap(), 2);
    // Removing the historical parent must not erase the retry's native failure.
    app.artisan(["horizon:forget", &id])
        .await
        .assertSuccessful();
    let failures = app
        .app()
        .container()
        .get::<dyn openrust::queue::failed::FailedJobProviderInterface>("queue.failer")
        .unwrap();
    assert!(failures.find(payload.uuid()).await.unwrap().is_some());
    assert!(jobs.findFailed(&retry).await.unwrap().is_some());
    app.artisan(["horizon:forget", &retry])
        .await
        .assertSuccessful();
    assert!(failures.find(payload.uuid()).await.unwrap().is_none());
    assert_eq!(jobs.countFailed().await.unwrap(), 0);
    assert!(repository.recent(100).await.unwrap().is_empty());
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
    repository.heartbeatSupervisor("master:outside", &json!({"name":"master:outside","pid":1,"status":"running","processes":{"redis:outside-plan":1,"redis:default":1},"options":{"balance":"off"}})).await.unwrap();
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
    response.assertOk();
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
    .assertSuccessful();
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
                        .assertSuccessful();
                }
                "console-retry" => {
                    app.artisan(["horizon:retry", &original_id])
                        .await
                        .assertExitCode(1);
                }
                _ => {
                    let response = app.call(request).await.unwrap();
                    response.assertOk();
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
        assert_eq!(jobs.countPending().await.unwrap(), 0);
        if operation == "forget" {
            assert!(jobs.findFailed(&original_id).await.unwrap().is_none());
            assert_eq!(
                jobs.findFailedByUuid(payload.uuid())
                    .await
                    .unwrap()
                    .unwrap()
                    .record
                    .id,
                replacement_id
            );
        } else {
            assert!(
                jobs.findFailed(&original_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .retried_by
                    .is_empty()
            );
        }
    }
}
