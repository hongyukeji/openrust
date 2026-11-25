use openrust::{
    contracts::{
        container::Container as _,
        events::{Event as EventContract, Listener},
        foundation::Application as _,
    },
    database::{
        Value,
        eloquent::{Model, ModelMetadata, ModelState},
    },
    events::{Delivery, EventContext, EventError, EventFuture, EventResult, ListenerResponse},
    foundation::configuration::Routing,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    notifications::{
        AnonymousNotifiable, Notification, NotificationRoute, messages::MailDelivery,
        slack::SlackRoute,
    },
    queue::Payload,
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Config, Event, Http, Notification as Notifications, Route},
    },
    testing::TestApp,
};
use openrust_horizon::{
    Horizon, HorizonServiceProvider, LongWaitDetected, LongWaitDetectedNotification,
    LongWaitNotificationFactory, MasterSupervisor, MasterSupervisorLooped, Repository, Supervisor,
    SupervisorLooped, SupervisorOptions, Tags,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
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
        Response::json(StatusCode::OK, Tags::for_payload(&payload).unwrap())
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
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(Routing::default().api(if tagged {
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
    assert!(Tags::for_payload(&Payload::new("outside", json!({})).unwrap()).is_err());
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
fn model_tags_convert_scalar_keys_and_include_the_rust_model_type() {
    let mut order = Order::from_state(ModelState::default());
    let model_type = std::any::type_name::<Order>();
    assert_eq!(Tags::for_model(&order).unwrap(), format!("{model_type}:"));
    for (value, key) in [
        (Value::Null, ""),
        (Value::Bool(false), ""),
        (Value::Bool(true), "1"),
        (Value::Integer(42), "42"),
        (Value::Unsigned(43), "43"),
        (Value::Real(1.5), "1.5"),
        (Value::Real(1.23e30), "1.23E+30"),
        (Value::Text("order-44".into()), "order-44"),
        (Value::Bytes(b"order-45".to_vec()), "order-45"),
    ] {
        order.state_mut().set_attribute("id", value);
        assert_eq!(
            Tags::for_model(&order).unwrap(),
            format!("{model_type}:{key}")
        );
    }
    for value in [
        Value::Bytes(vec![0xff]),
        Value::Json(serde_json::json!({"id": 46})),
    ] {
        order.state_mut().set_attribute("id", value);
        assert!(Tags::for_model(&order).is_err());
    }
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
    assert_eq!(mail.envelope.subject, ": Long Queue Wait Detected");
    let slack = notification
        .to_legacy_slack(&recipient)
        .unwrap()
        .unwrap()
        .to_array();
    assert_eq!(slack["username"], "OpenRust Horizon");
    assert_eq!(
        slack["attachments"][0]["text"],
        "[] The \"orders\" queue on the \"redis\" connection has a wait time of 120 seconds."
    );
    let sms = serde_json::to_value(
        notification
            .to_sms(&recipient, "+15555550100".into())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sms["to"], "+15555550100");
    assert_eq!(sms["content"], slack["attachments"][0]["text"]);
}

fn named_notification_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let notification = notification();
        let recipient = recipient();
        let Some(MailDelivery::Message(mail)) = notification.to_mail(&recipient).unwrap() else {
            panic!("expected mail message")
        };
        assert_eq!(mail.envelope.subject, "运维: Long Queue Wait Detected");
        let content = "[运维] The \"orders\" queue on the \"redis\" connection has a wait time of 120 seconds.";
        assert_eq!(
            notification
                .to_sms(&recipient, "+15555550100".into())
                .unwrap()
                .unwrap()
                .content(),
            content
        );

        Http::fake(|_| {
            Some(openrust::http::client::Response::from_json(
                StatusCode::OK,
                &json!({"ok":true}),
            ))
        })
        .unwrap();
        for destination in [
            "https://example.test/webhook".into(),
            SlackRoute::for_webhook("https://example.test/webhook", Some("#operators".into())),
            SlackRoute::for_webhook("https://example.test/webhook", Some(String::new())),
            "C123".into(),
        ] {
            let slack = AnonymousNotifiable::default()
                .route("slack", NotificationRoute::Slack(destination))
                .unwrap();
            Notifications::send(&[&slack], &notification).await.unwrap();
        }
        let sent = Http::recorded().unwrap();
        let [
            (webhook, _),
            (override_webhook, _),
            (empty_channel, _),
            (bot, _),
        ] = sent.as_slice()
        else {
            panic!("expected three webhook requests and one Bot API request")
        };
        assert_eq!(webhook.url.as_str(), "https://example.test/webhook");
        assert!(!webhook.headers.contains_key(http::header::AUTHORIZATION));
        let payload: serde_json::Value =
            serde_json::from_slice(webhook.body.as_deref().unwrap()).unwrap();
        assert_eq!(payload["text"], "Oh no! Something needs your attention.");
        assert_eq!(payload["attachments"][0]["title"], "Long Wait Detected");
        assert_eq!(payload["attachments"][0]["text"], content);
        assert!(payload.get("channel").is_none());
        assert_eq!(override_webhook.url, webhook.url);
        assert!(
            !override_webhook
                .headers
                .contains_key(http::header::AUTHORIZATION)
        );
        let overridden: serde_json::Value =
            serde_json::from_slice(override_webhook.body.as_deref().unwrap()).unwrap();
        assert_eq!(overridden["channel"], "#operators");
        assert_eq!(overridden["attachments"], payload["attachments"]);
        assert_eq!(empty_channel.body, webhook.body);
        assert_eq!(bot.url.as_str(), "https://example.test/bot");
        assert_eq!(
            bot.headers[http::header::AUTHORIZATION],
            "Bearer test-token"
        );
        let payload: serde_json::Value =
            serde_json::from_slice(bot.body.as_deref().unwrap()).unwrap();
        assert_eq!(payload["channel"], "C123");
        assert_eq!(payload["blocks"][0]["text"]["text"], "Long Wait Detected");
        assert_eq!(payload["blocks"][1]["text"]["text"], content);
        Response::json(StatusCode::OK, json!({"recorded":true}))
    })
}

#[tokio::test]
async fn named_long_wait_notifications_reach_both_native_slack_transports() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                Ok(BTreeMap::from([(
                    "services".into(),
                    json!({"slack":{"notifications":{
                        "bot_user_oauth_token":"test-token","endpoint":"https://example.test/bot"
                    }}}),
                )]))
            })
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(
                Routing::default().api(|| Route::get("/notification", named_notification_response)),
            )
    })
    .unwrap();
    std::fs::write(app.root().join(".env"), "HORIZON_NAME=运维\n").unwrap();
    app.get("/api/notification").await.unwrap().assert_ok();
}

#[derive(Default)]
struct SupervisorLoopObserver(AtomicUsize);

struct MasterLoopObserver {
    cycles: AtomicUsize,
    job_indices: [String; 7],
}

impl Listener for MasterLoopObserver {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventContract,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event
                .downcast_ref::<MasterSupervisorLooped>()
                .expect("typed Master event");
            assert!(event.paused);
            let repository = App::make::<Repository>().await?;
            let snapshot = repository
                .master_supervisors()
                .find(&event.name)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(snapshot["status"], "paused");
            if self.cycles.load(Ordering::Relaxed) == 0 {
                let redis = App::make::<openrust::redis::RedisManager>()
                    .await
                    .unwrap()
                    .connection(None)
                    .unwrap();
                for key in &self.job_indices {
                    assert_eq!(
                        redis
                            .command("ZSCORE", &[key.as_bytes(), b"probe-stale"])
                            .await
                            .unwrap(),
                        openrust::redis::Value::Nil
                    );
                    redis
                        .command("ZADD", &[key.as_bytes(), b"0", b"probe-next"])
                        .await
                        .unwrap();
                }
                self.cycles.fetch_add(1, Ordering::Relaxed);
                return Err(EventError::InvalidPayload);
            }
            self.cycles.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        })
    }
}

impl Listener for SupervisorLoopObserver {
    fn handle<'a>(
        &'a self,
        event: &'a dyn EventContract,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            let event = event
                .downcast_ref::<SupervisorLooped>()
                .expect("typed Supervisor event");
            assert!(event.paused);
            assert_eq!(event.name, "probe-supervisor");
            let repository = App::make::<Repository>().await?;
            let snapshot = repository.supervisor(&event.name).await.unwrap().unwrap();
            assert_eq!(snapshot["status"], "paused");
            if self.0.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err(EventError::InvalidPayload);
            }
            Ok(None)
        })
    }
}

fn supervisor_webhook_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let repository = App::make::<Repository>().await.unwrap();
        let queues = openrust::queue::QueueFacade::manager().unwrap();
        let queue = queues.connection(Some("redis")).unwrap();
        let payload = Payload::new("probe", json!({})).unwrap();
        queue.push(&payload, Some("alerts")).await.unwrap();
        repository
            .metrics_repository()
            .increment_queue("redis:alerts", 2500.)
            .await
            .unwrap();
        repository
            .heartbeat_supervisor(
                "probe",
                &json!({"name":"probe","pid":1,"processes":{"redis:alerts":1}}),
            )
            .await
            .unwrap();

        Horizon::route_slack_webhook_notifications_to("https://example.test/webhook", "#operators")
            .unwrap();
        Http::prevent_stray_requests(true).unwrap();
        Http::fake(|request| {
            (request.url().as_str() == "https://example.test/webhook").then(|| {
                openrust::http::client::Response::from_json(StatusCode::OK, &json!({"ok":true}))
            })
        })
        .unwrap();
        let redis = App::make::<openrust::redis::RedisManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let prefix = Config::get("horizon.prefix").unwrap().unwrap();
        let stale_indices = ["masters", "supervisors"]
            .map(|kind| redis.key(&format!("{}{{horizon}}:{kind}", prefix.as_str().unwrap())));
        let job_indices = [
            "recent",
            "pending",
            "completed",
            "silenced",
            "recent_failed",
            "failed",
            "monitored",
        ]
        .map(|kind| {
            redis.key(&format!(
                "{}{{horizon}}:jobs:{kind}",
                prefix.as_str().unwrap()
            ))
        });
        let recent_score = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string();
        for key in &job_indices {
            assert_eq!(
                redis
                    .command(
                        "ZADD",
                        &[
                            key.as_bytes(),
                            b"0",
                            b"probe-stale",
                            recent_score.as_bytes(),
                            b"probe-fresh"
                        ]
                    )
                    .await
                    .unwrap(),
                openrust::redis::Value::Int(2)
            );
        }
        for key in &stale_indices {
            assert_eq!(
                redis
                    .command("ZADD", &[key.as_bytes(), b"0", b"probe-stale"])
                    .await
                    .unwrap(),
                openrust::redis::Value::Int(1)
            );
        }
        let master = MasterSupervisor::new(
            repository.clone(),
            queues.clone(),
            BTreeMap::new(),
            std::env::current_exe().unwrap(),
            App::base_path("").unwrap(),
            "testing".into(),
        )
        .unwrap();
        repository.pause().await.unwrap();
        let master_observer = Arc::new(MasterLoopObserver {
            cycles: AtomicUsize::new(0),
            job_indices,
        });
        Event::listen(
            MasterSupervisorLooped::NAME,
            master_observer.clone(),
            Delivery::Sync,
        )
        .unwrap();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let (result, heartbeat) = tokio::join!(master.run(&shutdown), async {
            let heartbeat = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while master_observer.cycles.load(Ordering::Relaxed) < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await;
            shutdown.cancel();
            heartbeat
        });
        result.unwrap();
        heartbeat.unwrap();
        for key in &master_observer.job_indices {
            for member in [b"probe-next".as_slice(), b"probe-fresh"] {
                assert_ne!(
                    redis
                        .command("ZSCORE", &[key.as_bytes(), member])
                        .await
                        .unwrap(),
                    openrust::redis::Value::Nil
                );
            }
        }
        for key in &stale_indices {
            assert_eq!(
                redis
                    .command("ZSCORE", &[key.as_bytes(), b"probe-stale"])
                    .await
                    .unwrap(),
                openrust::redis::Value::Nil
            );
        }
        assert!(Http::recorded().unwrap().is_empty());
        repository.r#continue().await.unwrap();

        let observer = Arc::new(SupervisorLoopObserver::default());
        Event::listen(SupervisorLooped::NAME, observer.clone(), Delivery::Sync).unwrap();
        let supervisor = Supervisor::new(
            repository,
            queues,
            BTreeMap::from([(
                "probe-supervisor".into(),
                SupervisorOptions {
                    name: "probe-supervisor".into(),
                    queue: vec!["alerts".into()],
                    max_processes: 0,
                    memory: 1024,
                    ..SupervisorOptions::default()
                },
            )]),
            std::env::current_exe().unwrap(),
            App::base_path("").unwrap(),
            "testing".into(),
        )
        .unwrap()
        .paused(true);
        let shutdown = tokio_util::sync::CancellationToken::new();
        let (result, seen) = tokio::join!(supervisor.run(&shutdown), async {
            let seen = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while observer.0.load(Ordering::Relaxed) < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await;
            shutdown.cancel();
            seen
        });
        result.unwrap();
        seen.unwrap();
        let sent = Http::recorded().unwrap();
        let [(webhook, response)] = sent.as_slice() else {
            panic!("expected a single alert")
        };
        assert!(response.is_some());
        let payload: serde_json::Value =
            serde_json::from_slice(webhook.body.as_deref().unwrap()).unwrap();
        assert_eq!(payload["channel"], "#operators");
        assert_eq!(
            payload["attachments"][0]["text"],
            "[] The \"alerts\" queue on the \"redis\" connection has a wait time of 3 seconds."
        );
        Response::json(StatusCode::OK, json!({"recorded":true}))
    })
}

fn wait_config(
    base: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    assert_eq!(std::env::var("HORIZON_REQUIRE_REDIS").as_deref(), Ok("1"));
    Ok(BTreeMap::from([
        (
            "database".into(),
            json!({"redis":{
                "options":{"prefix":format!("{}:",base.file_name().unwrap().to_string_lossy())},
                "default":{"url":std::env::var("HORIZON_TEST_REDIS_URL").unwrap()}
            }}),
        ),
        (
            "queue".into(),
            json!({"connections":{"redis":{"driver":"redis","connection":"default","queue":"alerts"}}}),
        ),
        (
            "horizon".into(),
            json!({"memory_limit":1024,"waits":{"redis:alerts":1}}),
        ),
    ]))
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_supervisor_delivers_wait_alerts_and_master_only_persists_its_state() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(wait_config)
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(
                Routing::default().api(|| Route::get("/alert", supervisor_webhook_response)),
            )
    })
    .unwrap();
    app.get("/api/alert").await.unwrap().assert_ok();
}

struct CustomWaitNotification(u64);

impl Notification for CustomWaitNotification {
    fn via(
        &self,
        _: &dyn openrust::notifications::Notifiable,
    ) -> openrust::notifications::NotificationResult<Vec<String>> {
        Ok(vec!["mail".into()])
    }

    fn fake_data(&self) -> serde_json::Value {
        json!({"custom_wait":self.0})
    }
}

impl LongWaitDetectedNotification for CustomWaitNotification {
    fn signature(&self) -> String {
        "custom:shared".into()
    }
}

struct CustomWaitFactory;
impl LongWaitNotificationFactory for CustomWaitFactory {
    fn make(&self, event: &LongWaitDetected) -> Box<dyn LongWaitDetectedNotification> {
        Box::new(CustomWaitNotification(event.seconds))
    }
}

struct CustomWaitProvider;
impl ServiceProvider for CustomWaitProvider {
    fn register<'a>(
        &'a mut self,
        app: &'a openrust::foundation::Application,
    ) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .instance::<dyn LongWaitNotificationFactory>(Arc::new(CustomWaitFactory))?;
            Ok(())
        })
    }
}

fn custom_wait_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Horizon::route_mail_notifications_to("operator@example.test").unwrap();
        let fake = Notifications::fake().unwrap();
        let first = notification();
        let custom = first.to_notification().await.unwrap();
        assert_eq!(custom.signature(), "custom:shared");
        Event::dispatch(first).await.unwrap();
        Event::dispatch(LongWaitDetected {
            queue: "another-queue".into(),
            ..notification()
        })
        .await
        .unwrap();
        fake.assert_sent_on_demand_once(custom.kind())
            .assert_sent_on_demand_matching(custom.kind(), |entry| {
                entry.channels == ["mail"] && entry.data == json!({"custom_wait":120})
            })
            .assert_count(1);
        Response::json(StatusCode::OK, json!({"recorded":true}))
    })
}

#[tokio::test]
#[ignore = "requires dedicated Redis 8 acceptance wrapper"]
async fn redis_wait_event_listener_uses_the_container_factory_and_custom_signature() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(wait_config)
            .with_providers(vec![
                Box::new(CustomWaitProvider),
                Box::new(HorizonServiceProvider),
            ])
            .with_routing(
                Routing::default().api(|| Route::get("/custom-alert", custom_wait_response)),
            )
    })
    .unwrap();
    app.get("/api/custom-alert").await.unwrap().assert_ok();
}
fn notification_response(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        Horizon::route_mail_notifications_to("operator@example.test").unwrap();
        Horizon::route_slack_notifications_to("https://example.test/webhook").unwrap();
        Horizon::route_slack_webhook_notifications_to("https://example.test/webhook", "#operators")
            .unwrap();
        Horizon::route_sms_notifications_to("+15555550100").unwrap();
        let fake = Notifications::fake().unwrap();
        fake.assert_nothing_sent();
        let notification = notification();
        Notifications::send(&[&recipient()], &notification)
            .await
            .unwrap();
        fake.assert_sent_on_demand_once(notification.kind())
            .assert_sent_on_demand_matching(notification.kind(), |entry| {
                entry.channels == ["mail", "slack", "sms"]
                    && entry.data == json!({"connection":"redis","queue":"orders","seconds":120})
            })
            .assert_count(1);
        Response::json(StatusCode::OK, json!({"recorded":true}))
    })
}
#[tokio::test]
async fn long_wait_notifications_use_the_native_notification_fake() {
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(vec![Box::new(HorizonServiceProvider)])
            .with_routing(
                Routing::default().api(|| Route::get("/notification", notification_response)),
            )
    })
    .unwrap();
    app.get("/api/notification").await.unwrap().assert_ok();
    assert!(Horizon::route_mail_notifications_to("outside@example.test").is_err());
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
                repository.notification_lock("redis:orders").await.unwrap(),
                repository
                    .metrics_repository()
                    .acquire_wait_time_monitor_lock()
                    .await
                    .unwrap(),
                repository
                    .metrics_repository()
                    .acquire_snapshot_lock()
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
    assert!(repository.notification_lock("redis:other").await.unwrap());
    let isolated = Repository::new(connection.clone(), "isolated:".into(), 60).unwrap();
    assert!(isolated.notification_lock("redis:orders").await.unwrap());
    assert!(
        isolated
            .metrics_repository()
            .acquire_wait_time_monitor_lock()
            .await
            .unwrap()
    );
    assert!(
        isolated
            .metrics_repository()
            .acquire_snapshot_lock()
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
    jobs.store_retry_reference("original-transport", "completed-transport")
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
    jobs.store_retry_reference("original-transport", "failed-transport")
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
        jobs.find_failed_by_uuid(parent.uuid())
            .await
            .unwrap()
            .unwrap()
            .record
            .id,
        "failed-transport"
    );
    assert!(jobs.find(parent.uuid()).await.unwrap().is_none());
    assert!(jobs.delete_failed("failed-transport").await.unwrap());
    assert!(
        jobs.find_failed_by_uuid(parent.uuid())
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
    jobs.store_retry_reference("original-transport", "fast-transport")
        .await
        .unwrap();
    let details = jobs.find("original-transport").await.unwrap().unwrap();
    assert_eq!(details.retried_by[2]["status"], "completed");
    assert!(details.payload.is_object());
    assert!(details.tags.is_empty());
}
