use openrust::{
    database::{
        Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    notifications::{AnonymousNotifiable, Notification, NotificationRoute, messages::MailDelivery},
    queue::Payload,
    support::facades::{Notification as Notifications, Route},
    testing::TestApp,
};
use openrust_horizon::{Horizon, HorizonServiceProvider, LongWaitDetected, Repository, Tags};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

fn first_tags(_: &Payload) -> Vec<String> {
    vec!["orders".into(), String::new(), "customer:42".into()]
}
fn second_tags(_: &Payload) -> Vec<String> {
    vec!["orders".into(), "priority".into(), "customer:42".into()]
}
fn tags_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let payload = Payload::new("send-order", json!({"id":42})).unwrap();
        Response::json(StatusCode::OK, Tags::forPayload(&payload).unwrap())
    })
}
fn tagged_routes() -> Result<(), openrust::routing::RoutingError> {
    Horizon::tag(first_tags).unwrap();
    Horizon::tag(second_tags).unwrap();
    Route::get("/tags", tags_response)
}
fn untagged_routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/tags", tags_response)
}
fn app(tagged: bool) -> TestApp {
    TestApp::create(|builder| {
        builder
            .withProviders(vec![Box::new(HorizonServiceProvider)])
            .withRouting(Routing::default().api(if tagged {
                tagged_routes
            } else {
                untagged_routes
            }))
    })
    .unwrap()
}
#[tokio::test]
async fn tag_callbacks_are_application_local_and_preserve_first_nonempty_occurrence() {
    let mut tagged = app(true);
    let mut empty = app(false);
    let first = tagged.get("/api/tags").await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(first.body()).unwrap(),
        json!(["orders", "customer:42", "priority"])
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(empty.get("/api/tags").await.unwrap().body())
            .unwrap(),
        json!([])
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(tagged.get("/api/tags").await.unwrap().body())
            .unwrap(),
        json!(["orders", "customer:42", "priority"])
    );
    assert!(Horizon::tag(first_tags).is_err());
    assert!(Tags::forPayload(&Payload::new("outside", json!({})).unwrap()).is_err());
}

struct Order(ModelState);
impl Model for Order {
    fn metadata() -> &'static ModelMetadata {
        static META: ModelMetadata = ModelMetadata::new("orders");
        &META
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
#[test]
fn model_tags_require_a_supported_identifier_and_include_the_rust_model_type() {
    let mut order = Order::from_state(ModelState::default());
    assert!(Tags::forModel(&order).is_err());
    for (value, key) in [
        (Value::Integer(42), "42"),
        (Value::Unsigned(43), "43"),
        (Value::Text("order-44".into()), "order-44"),
    ] {
        order.state_mut().setAttribute("id", value);
        assert_eq!(
            Tags::forModel(&order).unwrap(),
            format!("{}:{key}", std::any::type_name::<Order>())
        );
    }
    order.state_mut().setAttribute("id", Value::Bool(true));
    assert!(Tags::forModel(&order).is_err());
}

fn notification() -> LongWaitDetected {
    LongWaitDetected {
        connection: "redis".into(),
        queue: "orders".into(),
        seconds: 120,
    }
}
fn recipient() -> AnonymousNotifiable {
    AnonymousNotifiable::default()
        .route(
            "mail",
            NotificationRoute::Mail(vec!["operator@example.test".into()]),
        )
        .unwrap()
        .route(
            "slack",
            NotificationRoute::Slack("https://example.test/webhook".into()),
        )
        .unwrap()
        .route("sms", NotificationRoute::Sms("+15555550100".into()))
        .unwrap()
}
#[test]
fn long_wait_notification_selects_explicit_routes_and_builds_channel_messages() {
    let notification = notification();
    let recipient = recipient();
    assert_eq!(
        notification.via(&recipient).unwrap(),
        ["mail", "slack", "sms"]
    );
    assert!(
        notification
            .via(&AnonymousNotifiable::default())
            .unwrap()
            .is_empty()
    );
    let Some(MailDelivery::Message(mail)) = notification.to_mail(&recipient).unwrap() else {
        panic!("expected mail message")
    };
    assert_eq!(
        mail.envelope.subject,
        "OpenRust Horizon: Long Queue Wait Detected"
    );
    let slack = notification
        .to_legacy_slack(&recipient)
        .unwrap()
        .unwrap()
        .toArray();
    assert_eq!(slack["username"], "OpenRust Horizon");
    assert_eq!(
        slack["text"],
        "The \"orders\" queue on the \"redis\" connection has a wait time of 120 seconds."
    );
    let sms = serde_json::to_value(
        notification
            .to_sms(&recipient, "+15555550100".into())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sms["to"], "+15555550100");
    assert_eq!(sms["content"], slack["text"]);
}
fn notification_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Horizon::routeMailNotificationsTo("operator@example.test").unwrap();
        Horizon::routeSlackNotificationsTo("https://example.test/webhook").unwrap();
        Horizon::routeSmsNotificationsTo("+15555550100").unwrap();
        let fake = Notifications::fake().unwrap();
        fake.assertNothingSent();
        let notification = notification();
        Notifications::send(&[&recipient()], &notification)
            .await
            .unwrap();
        fake.assertSentOnDemandOnce(notification.kind())
            .assertSentOnDemandMatching(notification.kind(), |entry| {
                entry.channels == ["mail", "slack", "sms"]
                    && entry.data == json!({"connection":"redis","queue":"orders","seconds":120})
            })
            .assertCount(1);
        Response::json(StatusCode::OK, json!({"recorded":true}))
    })
}
#[tokio::test]
async fn long_wait_notifications_use_the_native_notification_fake() {
    let mut app = TestApp::create(|builder| {
        builder
            .withProviders(vec![Box::new(HorizonServiceProvider)])
            .withRouting(
                Routing::default().api(|| Route::get("/notification", notification_response)),
            )
    })
    .unwrap();
    app.get("/api/notification").await.unwrap().assertOk();
    assert!(Horizon::routeMailNotificationsTo("outside@example.test").is_err());
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_long_wait_notification_and_snapshot_locks_are_atomic_expiring_and_isolated() {
    assert_eq!(std::env::var("HORIZON_REQUIRE_REDIS").as_deref(), Ok("1"));
    assert!(
        std::env::var("HORIZON_ACCEPTANCE_REDIS_CONTAINER")
            .unwrap()
            .starts_with("openrust-horizon-")
    );
    let config = Arc::new(openrust::config::Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"redis":{"options":{"prefix":format!("locks-{}:",uuid::Uuid::now_v7())},"default":{"url":std::env::var("HORIZON_TEST_REDIS_URL").unwrap()}}}),
    )])));
    let connection = openrust::redis::RedisManager::new(config)
        .connection(None)
        .unwrap();
    let repository = Arc::new(Repository::new(connection.clone(), "horizon:".into(), 60).unwrap());
    let mut contenders = Vec::new();
    for _ in 0..16 {
        let repository = repository.clone();
        contenders.push(tokio::spawn(async move {
            (
                repository.notificationLock("redis:orders").await.unwrap(),
                repository
                    .metricsRepository()
                    .acquireWaitTimeMonitorLock()
                    .await
                    .unwrap(),
                repository
                    .metricsRepository()
                    .acquireSnapshotLock()
                    .await
                    .unwrap(),
            )
        }));
    }
    let mut acquired = [0, 0, 0];
    for contender in contenders {
        let (notification, monitor, snapshot) = contender.await.unwrap();
        acquired[0] += usize::from(notification);
        acquired[1] += usize::from(monitor);
        acquired[2] += usize::from(snapshot);
    }
    assert_eq!(acquired, [1, 1, 1]);
    assert!(repository.notificationLock("redis:other").await.unwrap());
    let isolated = Repository::new(connection.clone(), "isolated:".into(), 60).unwrap();
    assert!(isolated.notificationLock("redis:orders").await.unwrap());
    assert!(
        isolated
            .metricsRepository()
            .acquireWaitTimeMonitorLock()
            .await
            .unwrap()
    );
    assert!(
        isolated
            .metricsRepository()
            .acquireSnapshotLock()
            .await
            .unwrap()
    );
    let script = "local count=0; for _,key in ipairs(redis.call('KEYS',ARGV[1])) do local ttl=redis.call('TTL',key); local maximum=300; if string.sub(key,-17)=='wait_time_monitor' then maximum=60 elseif string.sub(key,-17)==':metrics:snapshot' then maximum=270 end; if ttl<1 or ttl>maximum then return 0 end;count=count+1 end;return count";
    let pattern = connection.key("horizon:{horizon}:*");
    assert_eq!(
        connection
            .command("EVAL", &[script.as_bytes(), b"0", pattern.as_bytes()])
            .await
            .unwrap(),
        openrust::redis::Value::Int(4)
    );
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_retry_references_follow_completion_failure_and_fast_retry_completion() {
    assert_eq!(std::env::var("HORIZON_REQUIRE_REDIS").as_deref(), Ok("1"));
    assert!(
        std::env::var("HORIZON_ACCEPTANCE_REDIS_CONTAINER")
            .unwrap()
            .starts_with("openrust-horizon-")
    );
    let config = Arc::new(openrust::config::Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"redis":{"options":{"prefix":format!("retry-{}:",uuid::Uuid::now_v7())},"default":{"url":std::env::var("HORIZON_TEST_REDIS_URL").unwrap()}}}),
    )])));
    let connection = openrust::redis::RedisManager::new(config)
        .connection(None)
        .unwrap();
    let repository = Repository::new(connection, "horizon:".into(), 60).unwrap();
    let jobs = repository.jobs();
    let parent = Payload::new("parent", json!({})).unwrap();
    jobs.failed(
        "initial failure",
        "original-transport",
        "redis",
        "default",
        &parent,
        &[],
    )
    .await
    .unwrap();
    let completed = parent.clone();
    jobs.storeRetryReference("original-transport", "completed-transport")
        .await
        .unwrap();
    assert_eq!(
        jobs.find("original-transport")
            .await
            .unwrap()
            .unwrap()
            .retried_by[0]["status"],
        "pending"
    );
    jobs.completed(
        "completed-transport",
        "redis",
        "default",
        &completed,
        &[],
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        jobs.find("original-transport")
            .await
            .unwrap()
            .unwrap()
            .retried_by[0]["status"],
        "completed"
    );
    let failed = parent.clone();
    jobs.storeRetryReference("original-transport", "failed-transport")
        .await
        .unwrap();
    jobs.failed(
        "second failure",
        "failed-transport",
        "redis",
        "default",
        &failed,
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        jobs.find("original-transport")
            .await
            .unwrap()
            .unwrap()
            .retried_by[1]["status"],
        "failed"
    );
    assert_eq!(
        jobs.findFailedByUuid(parent.uuid())
            .await
            .unwrap()
            .unwrap()
            .record
            .id,
        "failed-transport"
    );
    assert!(jobs.find(parent.uuid()).await.unwrap().is_none());
    assert!(jobs.deleteFailed("failed-transport").await.unwrap());
    assert!(
        jobs.findFailedByUuid(parent.uuid())
            .await
            .unwrap()
            .is_none()
    );
    // The parent retains its result even after the child record is forgotten.
    assert_eq!(
        jobs.find("original-transport")
            .await
            .unwrap()
            .unwrap()
            .retried_by[1]["status"],
        "failed"
    );
    let fast = parent.clone();
    jobs.completed("fast-transport", "redis", "default", &fast, &[], true)
        .await
        .unwrap();
    jobs.storeRetryReference("original-transport", "fast-transport")
        .await
        .unwrap();
    let details = jobs.find("original-transport").await.unwrap().unwrap();
    assert_eq!(details.retried_by[2]["status"], "completed");
    assert!(details.payload.is_object());
    assert!(details.tags.is_empty());
}
