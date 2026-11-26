use openrust::support::facades::Facade;
use openrust_telescope::{
    EntryType, IncomingEntry, Telescope, TelescopeServiceProvider,
    storage::{DatabaseEntriesRepository, EntryQueryOptions},
};
use serde_json::json;

#[path = "../config/telescope.rs"]
mod telescope_config;

fn routes() -> Result<(), openrust::routing::RoutingError> {
    for path in ["/probe", "/ignore/probe"] {
        openrust::support::facades::Route::get(path, handler)?;
    }
    openrust::support::facades::Route::get("/retry", retry)?;
    openrust::support::facades::Route::get("/logs", log_handler)?;
    openrust::support::facades::Route::get("/jobs", job_handler)?;
    openrust::support::facades::Route::group(openrust::routing::RouteGroup::new(), |router| {
        router.post("/input", input_handler)?;
        router.post("/unread", handler)?;
        Ok(())
    })?;
    openrust::support::facades::Route::get("/mail", mail_handler)?;
    openrust::support::facades::Route::get("/native", native_handler)?;
    openrust::support::facades::Route::get("/async", async_handler)?;
    openrust::support::facades::Route::get("/redis", redis_handler)?;
    openrust::support::facades::Route::get("/client", client_handler)?;
    openrust::support::facades::Route::get("/client-request-shape", client_request_shape_handler)?;
    openrust::support::facades::Route::get(
        "/client-response-shape",
        client_response_shape_handler,
    )?;
    openrust::support::facades::Route::get(
        "/client-multipart-middleware",
        multipart_middleware_handler,
    )?;
    openrust::support::facades::Route::get("/identity", identity_handler)?;
    openrust::support::facades::Route::get("/guest", identity_handler)?;
    openrust::support::facades::Route::get("/request-watcher/{kind}", watched_response)?;
    openrust::support::facades::Route::post("/request-watcher/{kind}", watched_response)?;
    openrust::support::facades::Route::get("/request-view", watched_view)?;
    openrust::support::facades::Route::post("/observed-plain", observed_plain_body)?;
    openrust::support::facades::Route::post("/observed-upload", observed_upload)?;
    Ok(())
}

fn web_routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/request-session", observed_session)?;
    Ok(())
}

fn observed_session(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        use openrust::contracts::session::Session as _;
        assert!(request.session()?.has("_token"));
        request
            .session_mut()?
            .put("_token", "session-token".into())?;
        request.session_mut()?.put(
            "nested",
            json!({"password":"session-password","remember":"public"}),
        )?;
        Ok(openrust::http::Response::from("session probe"))
    })
}

fn redis_handler(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        use openrust::redis::Redis;
        Redis::set("probe", b"actual").await.unwrap();
        assert_eq!(
            Redis::get("probe").await.unwrap().as_deref(),
            Some(b"actual".as_slice())
        );
        Redis::command("DEL", &[b"telescope-test:probe"])
            .await
            .unwrap();
        Ok(openrust::http::Response::from("observed redis"))
    })
}

#[tokio::test]
#[ignore = "requires isolated Redis 8 fixture and TELESCOPE_TEST_REDIS_URL"]
async fn native_redis_commands_preserve_connection_parameters_and_elapsed_time() {
    assert!(std::env::var("TELESCOPE_TEST_REDIS_URL").is_ok());
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/redis").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::Redis), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 3);
    assert!(
        entries
            .iter()
            .any(|entry| entry.content["command"] == "SET telescope-test:probe actual")
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.content["command"] == "GET telescope-test:probe")
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.content["command"] == "DEL telescope-test:probe")
    );
    assert!(entries.iter().all(|entry| {
        entry.content["connection"] == "default"
            && entry.content["time"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap()
                >= 0.0
    }));
}

#[tokio::test]
async fn native_cache_recording_pause_resume_use_the_selected_cache_store() {
    let mut app = application();
    app.boot().await.unwrap();
    let connection = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    if std::env::var("TELESCOPE_TEST_CACHE_DRIVER").as_deref() == Ok("database") {
        connection
            .get_schema_builder()
            .create("cache", |table| {
                table.string("key", 255).primary();
                table.medium_text("value");
                table.integer("expiration");
            })
            .await
            .unwrap();
        connection
            .get_schema_builder()
            .create("cache_locks", |table| {
                table.string("key", 255).primary();
                table.string("owner", 255);
                table.integer("expiration");
            })
            .await
            .unwrap();
    }
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/probe").await.unwrap().assert_ok();
    app.artisan(["telescope:pause"]).await.assert_successful();
    app.get("/api/probe").await.unwrap().assert_ok();
    app.artisan(["telescope:resume"]).await.assert_successful();
    app.get("/api/probe").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::Cache), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.content["type"] == "hit")
            .count(),
        2
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.content["type"] == "set")
            .count(),
        2
    );
    assert!(entries.iter().all(|entry| entry.content["key"] == "probe"));
    use openrust::contracts::cache::{LockProvider as _, Repository as _};
    let cache = app
        .app()
        .make::<openrust::cache::CacheManager>()
        .await
        .unwrap()
        .store(None)
        .unwrap();
    let value = openrust::cache::Value::Json(json!("temporary"));
    cache
        .put(
            "expiry-probe",
            &value,
            Some(openrust::cache::Ttl::Seconds(1)),
        )
        .await
        .unwrap();
    assert!(cache.get("expiry-probe").await.unwrap().is_some());
    let mut first = cache
        .lock("lease-probe", Some(std::time::Duration::from_secs(2)), None)
        .unwrap();
    let mut second = cache
        .lock("lease-probe", Some(std::time::Duration::from_secs(2)), None)
        .unwrap();
    assert!(first.get().await.unwrap());
    assert!(!second.get().await.unwrap());
    assert!(first.release().await.unwrap());
    assert!(second.get().await.unwrap());
    assert!(second.release().await.unwrap());
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    assert!(cache.get("expiry-probe").await.unwrap().is_none());
    assert!(cache.forget("probe").await.unwrap());
    assert!(cache.get("probe").await.unwrap().is_none());
    if std::env::var("TELESCOPE_TEST_CACHE_DRIVER").as_deref() == Ok("database") {
        let rows = connection.table("cache").get().await.unwrap();
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!(rows.iter().all(|row| match row.get("expiration") {
            Some(openrust::database::Value::Integer(expiration)) =>
                *expiration <= time + 315_360_002,
            Some(openrust::database::Value::Unsigned(expiration)) =>
                *expiration <= (time + 315_360_002) as u64,
            _ => false,
        }));
    }
}
fn handler(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let telescope = openrust_telescope::facades::Telescope::get_facade_root().unwrap();
        telescope.record_event(
            IncomingEntry::make(json!({"name":"probe"}).as_object().unwrap().clone()).unwrap(),
        );
        openrust::cache::Cache::put("probe", &openrust::cache::Value::Json(json!("value")), None)
            .await
            .unwrap();
        openrust::cache::Cache::get("probe").await.unwrap();
        Ok(openrust::http::Response::new(
            openrust::http::StatusCode::OK,
            openrust::http::Body::from("ok"),
        ))
    })
}
type FixtureConfig =
    Result<std::collections::BTreeMap<String, serde_json::Value>, openrust::config::ConfigError>;

fn provider_config(base: &std::path::Path, env: &openrust::config::Environment) -> FixtureConfig {
    let mut telescope = telescope_config::load(env)?;
    telescope["ignore_paths"] = json!(["api/ignore/*"]);
    telescope["watchers"]["QueryWatcher"]["ignore_packages"] = false.into();
    Ok(std::collections::BTreeMap::from([
        (
            "app".into(),
            json!({"env":"testing","url":"http://localhost","debug":true,"key":"01234567890123456789012345678901","cipher":"AES-256-CBC"}),
        ),
        (
            "database".into(),
            json!({"default":"testing","connections":{"testing":database_config(base)},"redis":{"default":{"url":std::env::var("TELESCOPE_TEST_REDIS_URL").ok()},"options":{"prefix":"telescope-test:"}}}),
        ),
        (
            "cache".into(),
            json!({"default":std::env::var("TELESCOPE_TEST_CACHE_DRIVER").unwrap_or("file".into()),"stores":{"file":{"driver":"file","path":base.join("cache/data"),"lock_path":base.join("cache/data")},"database":{"driver":"database","connection":"testing","table":"cache","lock_table":"cache_locks"},"redis":{"driver":"redis","connection":"default","lock_connection":"default"}}}),
        ),
        ("session".into(), json!({"driver":"array"})),
        (
            "auth".into(),
            json!({"defaults":{"guard":"probe"},"guards":{"probe":{"driver":"cached"}}}),
        ),
        (
            "queue".into(),
            json!({"default":"sync","connections":{"sync":{"driver":"sync"},"async":{"driver":"database","connection":"testing","table":"jobs","queue":"default","retry_after":30}},"batching":{"database":"testing","table":"job_batches"}}),
        ),
        (
            "mail".into(),
            json!({"default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"sender@example.org","name":"Sender"}}),
        ),
        ("telescope".into(), telescope),
    ]))
}

fn client_response_config(
    base: &std::path::Path,
    env: &openrust::config::Environment,
) -> FixtureConfig {
    let mut config = provider_config(base, env)?;
    config.get_mut("telescope").unwrap()["watchers"]["ClientRequestWatcher"]["size_limit"] =
        "0.04".into();
    Ok(config)
}

fn application() -> openrust::testing::TestApp {
    application_with_config(provider_config)
}

fn application_with_config(
    config: fn(&std::path::Path, &openrust::config::Environment) -> FixtureConfig,
) -> openrust::testing::TestApp {
    let app = openrust::testing::TestApp::create(|builder| {
        builder
            .with_config(config)
            .with_providers(vec![Box::new(TelescopeServiceProvider)])
            .with_middleware(|middleware| {
                middleware
                    .trust_proxies(
                        &["10.0.0.0/8"],
                        openrust::http::TrustedProxyHeaders::DEFAULT,
                    )
                    .unwrap();
                middleware.skip_trim_strings_when(|request| {
                    request.path().trim_start_matches('/') == "api/unread"
                });
                middleware.convert_empty_strings_to_null(|request| {
                    request.path().trim_start_matches('/') == "api/unread"
                });
            })
            .with_routing(
                openrust::foundation::configuration::Routing::default()
                    .web(web_routes)
                    .api(routes)
                    .commands(commands),
            )
    })
    .unwrap();
    std::fs::File::create(app.root().join("telescope.sqlite")).unwrap();
    app
}

fn database_config(base: &std::path::Path) -> serde_json::Value {
    match std::env::var("TELESCOPE_TEST_DRIVER")
        .as_deref()
        .unwrap_or("sqlite")
    {
        "sqlite" => json!({"driver":"sqlite","database":base.join("telescope.sqlite")}),
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()})
        }
        driver => panic!("unsupported isolated database {driver}"),
    }
}

fn commands() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Schedule::configure(|schedule| {
        schedule
            .call(|_, _| Box::pin(async { Ok(()) }))
            .name("observed callback")
            .timezone(jiff::tz::TimeZone::get("Asia/Shanghai").unwrap());
        schedule
            .call(|_, _| {
                Box::pin(async { Err(openrust::console::ConsoleError::new("observed failure")) })
            })
            .name("failing callback");
        Ok(())
    })
}

struct NativeEvent;
impl openrust::contracts::events::Event for NativeEvent {
    fn name(&self) -> &str {
        "Application\\Observed"
    }
    fn properties(&self) -> Option<serde_json::Value> {
        Some(json!({"actual":"payload"}))
    }
}

struct ObservedModel(openrust::database::eloquent::ModelState);
impl openrust::database::eloquent::Model for ObservedModel {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("observed_models")
                .timestamps(None, None)
                .fillable(&["title"]);
        &META
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
}

struct ObservedNotice(bool);
impl openrust::notifications::Notification for ObservedNotice {
    fn via(
        &self,
        _: &dyn openrust::notifications::Notifiable,
    ) -> openrust::notifications::NotificationResult<Vec<String>> {
        Ok(vec!["mail".into()])
    }
    fn to_mail(
        &self,
        _: &dyn openrust::notifications::Notifiable,
    ) -> openrust::notifications::NotificationResult<
        Option<openrust::notifications::messages::MailDelivery>,
    > {
        Ok(Some(
            openrust::notifications::messages::MailDelivery::Mailable(Box::new(ObservedMail)),
        ))
    }
    fn queue_options(
        &self,
        _: &dyn openrust::notifications::Notifiable,
        _: &str,
    ) -> Option<openrust::queue::DispatchOptions> {
        self.0.then(Default::default)
    }
}

fn native_handler(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        use openrust::{
            database::eloquent::Model as _,
            support::facades::{Event, Gate, Notification},
        };
        Event::dispatch(NativeEvent).await.unwrap();
        Gate::define_guest("observed", |_, arguments| {
            Box::pin(async move {
                assert_eq!(arguments[0].downcast_ref::<String>().unwrap(), "actual");
                Ok(openrust::auth::access::Response::allow())
            })
        })
        .unwrap();
        assert!(
            Gate::for_user(None)
                .unwrap()
                .allows("observed", &[&String::from("actual")])
                .await
                .unwrap()
        );
        let target = openrust::notifications::AnonymousNotifiable::default()
            .route(
                "mail",
                openrust::notifications::NotificationRoute::Mail(vec!["target@example.org".into()]),
            )
            .unwrap();
        Notification::send(&[&target], &ObservedNotice(false))
            .await
            .unwrap();
        Notification::send(&[&target], &ObservedNotice(true))
            .await
            .unwrap();
        let connection = openrust::database::Db::connection(None).unwrap();
        connection
            .get_schema_builder()
            .create("observed_models", |table| {
                table.id();
                table.string("title", None);
            })
            .await
            .unwrap();
        let mut model = ObservedModel::query_on(&connection)
            .make([("title".into(), "first".into())].into())
            .unwrap();
        model.save().await.unwrap();
        let id = model.get_key().unwrap().clone();
        model.set_attribute("title", "updated");
        model.save().await.unwrap();
        ObservedModel::query_on(&connection)
            .find_or_fail(id.clone())
            .await
            .unwrap();
        ObservedModel::query_on(&connection)
            .find_or_fail(id)
            .await
            .unwrap();
        model.delete().await.unwrap();
        openrust::support::dump(&json!({"real":"dump"}))
            .await
            .unwrap();
        Ok(openrust::http::Response::from(
            openrust::support::facades::View::get_facade_root()
                .unwrap()
                .render("observed", &Default::default())
                .unwrap(),
        ))
    })
}

#[tokio::test]
async fn native_event_gate_notification_model_view_and_dump_sources_are_observed() {
    let mut app = application();
    std::fs::create_dir_all(app.root().join("resources/views")).unwrap();
    std::fs::write(
        app.root().join("resources/views/observed.html"),
        "observed view",
    )
    .unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.auth(|_| Box::pin(async { Ok(true) }));
    api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/dumps",
        json!({}),
    )
    .await
    .assert_ok();
    app.get("/api/native")
        .await
        .unwrap()
        .assert_ok()
        .assert_content("observed view");
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let events = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].content["name"], "Application\\Observed");
    assert_eq!(events[0].content["payload"], json!({"actual":"payload"}));
    assert_eq!(events[0].content["listeners"], json!([]));
    let gates = repository
        .get(Some(EntryType::Gate), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].content["arguments"], json!(["actual"]));
    assert_eq!(gates[0].content["result"], "allowed");
    let notices = repository
        .get(Some(EntryType::Notification), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(notices.len(), 2);
    assert_eq!(
        notices
            .iter()
            .filter(|entry| entry.content["queued"] == true)
            .count(),
        1
    );
    assert!(
        notices
            .iter()
            .all(|entry| entry.content["notifiable"] == "Anonymous:target@example.org")
    );
    let mail = repository
        .get(Some(EntryType::Mail), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(mail.len(), 2);
    assert!(
        mail.iter()
            .all(|entry| entry.content["mailable"] == std::any::type_name::<ObservedNotice>())
    );
    assert_eq!(
        mail.iter()
            .filter(|entry| entry.content["queued"] == true)
            .count(),
        1
    );
    let models = repository
        .get(Some(EntryType::Model), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(models.len(), 4);
    let hydration = models
        .iter()
        .find(|entry| entry.content["action"] == "retrieved")
        .unwrap();
    assert_eq!(hydration.content["count"], 2);
    let queries = repository
        .get(Some(EntryType::Query), EntryQueryOptions::default())
        .await
        .unwrap();
    assert!(queries.iter().any(|entry| {
        entry.content["sql"].as_str().is_some_and(|sql| {
            sql.to_ascii_lowercase().starts_with("select") && sql.contains("observed_models")
        }) && entry.content["file"] == file!()
            && entry.content["line"].as_u64().is_some_and(|line| line > 0)
    }));
    assert!(
        models
            .iter()
            .any(|entry| entry.content["action"] == "updated"
                && entry.content["changes"]["title"] == "updated")
    );
    let views = repository
        .get(Some(EntryType::View), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].content["path"], "resources/views/observed.html");
    let dumps = repository
        .get(Some(EntryType::Dump), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(dumps.len(), 1);
    assert_eq!(dumps[0].content["entry_point_type"], "job");
    assert_eq!(
        dumps[0].content["entry_point_description"],
        "notifications.send_queued_notifications"
    );
    assert!(dumps[0].content["dump"].as_str().unwrap().contains("real"));
    let scheduled = app.artisan(["schedule:run"]).await;
    assert_eq!(scheduled.status, 0);
    let schedule = repository
        .get(Some(EntryType::ScheduledTask), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(schedule.len(), 2);
    assert!(
        schedule
            .iter()
            .all(|entry| entry.content["command"] == "Closure"
                && entry.content["expression"] == "* * * * *")
    );
    assert!(
        schedule
            .iter()
            .any(|entry| entry.content["timezone"] == "Asia/Shanghai")
    );
}

#[tokio::test]
async fn scheduled_entries_record_the_explicit_or_effective_schedule_timezone() {
    use openrust::contracts::foundation::Application as _;

    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let config = app.app().config().unwrap();
    config
        .set("app.timezone", json!("America/Toronto"))
        .unwrap();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    for (override_zone, expected) in [
        (None, json!("America/Toronto")),
        (Some(json!("Europe/Paris")), json!("Europe/Paris")),
        (Some(serde_json::Value::Null), serde_json::Value::Null),
    ] {
        repository.clear().await.unwrap();
        if let Some(override_zone) = override_zone {
            config.set("app.schedule_timezone", override_zone).unwrap();
        }
        app.artisan(["schedule:run"]).await.assert_successful();
        let scheduled = repository
            .get(Some(EntryType::ScheduledTask), EntryQueryOptions::default())
            .await
            .unwrap();
        assert_eq!(scheduled.len(), 2);
        for (description, timezone) in [
            ("observed callback", json!("Asia/Shanghai")),
            ("failing callback", expected),
        ] {
            let entry = scheduled
                .iter()
                .find(|entry| entry.content["description"] == description)
                .unwrap();
            let result = app
                .artisan(["telescope:show", &entry.id.to_string(), "--json"])
                .await;
            result.assert_successful();
            let output: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
            assert_eq!(
                output["entry"]["content"]["timezone"], timezone,
                "{description}"
            );
        }
    }
}

#[tokio::test]
async fn schedule_test_keeps_its_command_entry_without_recording_a_scheduled_task() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    app.artisan(["schedule:test", "--name=observed callback"])
        .await
        .assert_successful();
    assert!(
        repository
            .get(Some(EntryType::ScheduledTask), EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    let entries = repository
        .get(Some(EntryType::Command), EntryQueryOptions::default())
        .await
        .unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry.content["command"] == "schedule:test")
    );
    app.artisan(["schedule:run"]).await.assert_successful();
    assert_eq!(
        repository
            .get(Some(EntryType::ScheduledTask), EntryQueryOptions::default())
            .await
            .unwrap()
            .len(),
        2
    );
}

fn input_handler(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        request.input(None).await?;
        Ok(openrust::http::Response::from("decoded"))
    })
}

#[tokio::test]
async fn request_payload_snapshot_redacts_decoded_input_without_polling_unread_streams() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hide_request_parameters(&["secret", "nested.password"]);
    api_call(&mut app,"POST","/api/input?query=value&same=query",json!({"secret":"private","same":"body","nested":{"password":"hidden","public":"visible"},"empty":false})).await.assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let requests = repository
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].content["payload"],
        json!({"secret":"********","same":"body","query":"value","nested":{"password":"********","public":"visible"},"empty":false})
    );
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        app.call(
            http::Request::builder()
                .method("POST")
                .uri("/api/unread")
                .header("content-type", "application/json")
                .body(openrust::http::Body::from_stream(
                    futures_util::stream::pending::<Result<&'static str, std::io::Error>>(),
                ))
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    response.assert_ok();
    let requests = repository
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].content["payload"], serde_json::Value::Null);
}

struct ObservedMail;
impl openrust::mail::Mailable for ObservedMail {
    fn envelope(&self) -> openrust::mail::MailResult<openrust::mail::mailables::Envelope> {
        Ok(openrust::mail::mailables::Envelope {
            to: vec!["to@example.org".into()],
            cc: vec!["cc@example.org".into()],
            bcc: vec!["bcc@example.org".into()],
            subject: "Diagnostic mail".into(),
            ..Default::default()
        })
    }
    fn content(&self) -> openrust::mail::MailResult<openrust::mail::mailables::Content> {
        Ok(openrust::mail::mailables::Content {
            html: Some("<p>Hello</p><script>parent.compromised=true</script>".into()),
            text: Some("Hello".into()),
            ..Default::default()
        })
    }
}
fn mail_handler(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        openrust::support::facades::Mail::send(&ObservedMail)
            .await
            .unwrap();
        openrust::support::facades::Mail::mailer(None)
            .unwrap()
            .queue(&ObservedMail, Default::default(), None)
            .await
            .unwrap();
        Ok(openrust::http::Response::from("sent"))
    })
}
#[tokio::test]
async fn native_mail_preserves_queued_identity_and_sandboxes_preview_and_download() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/mail").await.unwrap().assert_ok();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.auth(|_| Box::pin(async { Ok(true) }));
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::Mail), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.content["queued"] == true)
            .count(),
        1
    );
    for entry in &entries {
        assert_eq!(
            entry.content["mailable"],
            std::any::type_name::<ObservedMail>()
        );
        assert_eq!(entry.content["to"], json!({"to@example.org":""}));
        let full = repository.find(&entry.id.to_string()).await.unwrap();
        assert_eq!(
            full.tags,
            ["bcc@example.org", "cc@example.org", "to@example.org"]
        );
        let preview = app
            .get(&format!(
                "/telescope/telescope-api/mail/{}/preview",
                entry.id
            ))
            .await
            .unwrap();
        preview.assert_ok();
        assert_eq!(
            preview.headers()["content-security-policy"],
            "sandbox allow-popups allow-popups-to-escape-sandbox"
        );
        assert_eq!(
            preview.body(),
            entry.content["html"].as_str().unwrap().as_bytes()
        );
        let download = app
            .get(&format!(
                "/telescope/telescope-api/mail/{}/download",
                entry.id
            ))
            .await
            .unwrap();
        download.assert_ok();
        assert_eq!(download.headers()["content-type"], "message/rfc822");
        assert!(
            download.headers()["content-disposition"]
                .to_str()
                .unwrap()
                .ends_with(".eml\"")
        );
        assert_eq!(
            download.body(),
            entry.content["raw"].as_str().unwrap().as_bytes()
        );
    }
    let deep = app.get("/telescope/mail/example").await.unwrap();
    deep.assert_ok();
    assert!(String::from_utf8_lossy(deep.body()).contains("base=\"/telescope\""));
}
#[tokio::test]
async fn provider_boot_is_lazy_and_native_lifecycles_flush_isolated_collectors() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.tag(|_| vec!["shared-configuration".into()]);
    telescope.filter(|entry| entry.content.get("name") != Some(&json!("excluded")));
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    assert!(
        repository
            .get(None, EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    for _ in 0..2 {
        app.get("/api/probe")
            .await
            .unwrap()
            .assert_ok()
            .assert_content("ok");
    }
    let events = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_ne!(events[0].batch_id, events[1].batch_id);
    assert_eq!(
        repository
            .find(&events[0].id.to_string())
            .await
            .unwrap()
            .tags,
        ["shared-configuration"]
    );
    assert_eq!(
        repository
            .get(Some(EntryType::Request), EntryQueryOptions::default())
            .await
            .unwrap()
            .len(),
        2
    );
    let caches = repository
        .get(Some(EntryType::Cache), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(caches.len(), 4);
    assert!(caches.iter().all(|entry| entry.content["key"] == "probe"));
    app.get("/api/ignore/probe").await.unwrap().assert_ok();
    assert_eq!(
        repository
            .get(Some(EntryType::Event), EntryQueryOptions::default())
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn list_filters_preserve_fixed_zero_values_cursor_text_and_tag_whitespace() {
    use std::collections::BTreeMap;

    fn names(rows: &serde_json::Value) -> Vec<&str> {
        rows.as_array()
            .unwrap()
            .iter()
            .map(|row| row["content"]["name"].as_str().unwrap())
            .collect()
    }

    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let application = app.app();
    let repository = application
        .make::<DatabaseEntriesRepository>()
        .await
        .unwrap();
    repository.clear().await.unwrap();
    let batch = uuid::Uuid::now_v7();
    let spaced = "\u{a0}edge\u{a0}";
    let entries = [
        ("visible", "0", "0"),
        ("hidden", "0", "0"),
        ("plain", "probe", "edge"),
        ("spaced", "probe", spaced),
    ]
    .map(|(name, family, tag)| {
        IncomingEntry::make(json!({"name":name}).as_object().unwrap().clone())
            .unwrap()
            .r#type(EntryType::Log)
            .batch_id(batch)
            .with_family_hash(Some(family.into()))
            .tags([tag])
    });
    repository.store(&entries).await.unwrap();
    application
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("testing"))
        .unwrap()
        .table("telescope_entries")
        .where_exact_text("uuid", &entries[1].uuid.to_string())
        .update(BTreeMap::from([(
            "should_display_on_index".into(),
            false.into(),
        )]))
        .await
        .unwrap();

    let visible = ["spaced", "plain", "visible"];
    let rows = repository
        .get(
            Some(EntryType::Log),
            EntryQueryOptions::default().before_sequence(Some(0)),
        )
        .await
        .unwrap();
    assert_eq!(names(&json!(rows)), visible);
    let spaced_tag = format!("--tag={spaced}");
    for (options, expected) in [
        (vec!["0"], &visible[..]),
        (
            vec!["--batch=0", "--before=", "--tag=0", "--family=0"],
            &visible[..],
        ),
        (vec!["--before=0"], &visible[..]),
        (vec!["--before=00"], &[][..]),
        (vec!["--limit=999999999999999999999999999999"], &visible[..]),
        (vec!["--tag=  0 , missing "], &["hidden", "visible"][..]),
        (vec![spaced_tag.as_str()], &["spaced"][..]),
    ] {
        let mut args = vec!["telescope:list", "--json"];
        args.extend(options.iter().copied());
        let result = app.artisan(args).await;
        result.assert_successful();
        let payload: serde_json::Value = serde_json::from_str(&result.stdout).unwrap();
        assert_eq!(names(&payload), expected, "{options:?}");
    }

    app.app()
        .make::<Telescope>()
        .await
        .unwrap()
        .auth(|_| Box::pin(async { Ok(true) }));
    for (input, expected) in [
        (
            json!({"batch_id":"0","before":"","tag":"0","family_hash":"0"}),
            &visible[..],
        ),
        (json!({"before":0}), &visible[..]),
        (json!({"before":"00"}), &[][..]),
        (json!({"tag":spaced}), &["plain"][..]),
    ] {
        let api = api_call(
            &mut app,
            "POST",
            "/telescope/telescope-api/logs",
            input.clone(),
        )
        .await;
        api.assert_ok();
        let payload: serde_json::Value = api.json().unwrap();
        assert_eq!(names(&payload["entries"]), expected, "{input}");
    }
}

#[tokio::test]
async fn list_filter_inputs_keep_native_batch_and_cursor_comparisons() {
    use openrust::{database::DatabaseError, http::StatusCode};
    use openrust_telescope::{Error, contracts::EntriesRepository};

    fn sequences(rows: &serde_json::Value) -> Vec<i64> {
        rows.as_array()
            .unwrap()
            .iter()
            .map(|row| row["sequence"].as_i64().unwrap())
            .collect()
    }

    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let application = app.app();
    let repository = application
        .make::<DatabaseEntriesRepository>()
        .await
        .unwrap();
    let connection = application
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("testing"))
        .unwrap();
    let driver = connection.get_driver_name();
    let batch = uuid::Uuid::parse_str("abcdef20-d1ac-4c00-ab01-013cd84a0001").unwrap();
    for (sequence, visible) in [(1_i64, true), (2, false), (3, true), (4, true)] {
        connection
            .table("telescope_entries")
            .insert(&[std::collections::BTreeMap::from([
                ("sequence".into(), sequence.into()),
                ("uuid".into(), uuid::Uuid::now_v7().to_string().into()),
                ("batch_id".into(), batch.to_string().into()),
                ("type".into(), "log".into()),
                ("content".into(), "{}".into()),
                ("should_display_on_index".into(), visible.into()),
                ("created_at".into(), "2026-11-26 16:00:00".into()),
            ])])
            .await
            .unwrap();
    }
    application
        .make::<Telescope>()
        .await
        .unwrap()
        .auth(|_| Box::pin(async { Ok(true) }));

    let visible = &[4, 3, 1][..];
    let all = &[4, 3, 2, 1][..];
    let oldest = &[1][..];
    let empty = &[][..];
    let invalid = Err("22P02");
    let standard = batch.to_string();
    let uppercase = standard.to_ascii_uppercase();
    let simple = batch.simple().to_string();
    let braced = format!("{{{batch}}}");
    let cases = [
        ("before", "0", [Ok(visible); 3]),
        ("before", "00", [Ok(empty); 3]),
        ("before", " 2 ", [Ok(oldest); 3]),
        ("before", "2.5", [Ok(oldest), Ok(oldest), invalid]),
        ("before", "2e0", [Ok(oldest), Ok(oldest), invalid]),
        ("before", "2garbage", [Ok(visible), Ok(oldest), invalid]),
        ("before", "non-numeric", [Ok(visible), Ok(empty), invalid]),
        (
            "before",
            "9223372036854775808",
            [Ok(visible), Ok(visible), Err("22003")],
        ),
        ("batch", "not-a-uuid", [Ok(empty), Ok(empty), invalid]),
        ("batch", "0", [Ok(visible); 3]),
        ("batch", &standard, [Ok(all); 3]),
        ("batch", &uppercase, [Ok(empty), Ok(all), Ok(all)]),
        ("batch", &simple, [Ok(empty), Ok(empty), Ok(all)]),
        ("batch", &braced, [Ok(empty), Ok(empty), Ok(all)]),
    ];
    let index = match driver {
        "sqlite" => 0,
        "mysql" => 1,
        "pgsql" => 2,
        _ => panic!("unexpected test driver {driver}"),
    };
    for (field, input, expected) in cases {
        let key = if field == "before" {
            "before"
        } else {
            "batch_id"
        };
        let command = app
            .artisan(["telescope:list", "--json", &format!("--{field}={input}")])
            .await;
        let response = api_call(
            &mut app,
            "POST",
            "/telescope/telescope-api/logs",
            json!({(key):input}),
        )
        .await;
        match expected[index] {
            Ok(expected) => {
                command.assert_successful();
                let payload = serde_json::from_str(&command.stdout).unwrap();
                assert_eq!(sequences(&payload), expected, "Artisan {field}={input}");
                response.assert_ok();
                let payload: serde_json::Value = response.json().unwrap();
                assert_eq!(
                    sequences(&payload["entries"]),
                    expected,
                    "HTTP {key}={input}"
                );
            }
            Err(sqlstate) => {
                assert_ne!(command.status, 0, "Artisan {field}={input}");
                assert!(
                    command
                        .stderr
                        .contains("Telescope database operation failed")
                );
                response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
                let failure = repository
                    .get_with_filter_inputs(
                        Some(EntryType::Log),
                        EntryQueryOptions::default(),
                        (field == "batch").then(|| input.to_owned()),
                        (field == "before").then(|| input.to_owned()),
                    )
                    .await
                    .unwrap_err();
                let Error::Database(DatabaseError::Driver { source, .. }) = failure else {
                    panic!("expected database driver error for {field}={input}, got {failure:?}");
                };
                assert_eq!(
                    source
                        .as_database_error()
                        .and_then(|error| error.code())
                        .as_deref(),
                    Some(sqlstate),
                    "{field}={input}",
                );
            }
        }
    }
    for (value, expected) in [
        (json!(true), empty),
        (json!(false), visible),
        (json!(2.5), oldest),
    ] {
        let response = api_call(
            &mut app,
            "POST",
            "/telescope/telescope-api/logs",
            json!({"before":value}),
        )
        .await;
        if value.is_f64() && driver == "pgsql" {
            response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
        } else {
            response.assert_ok();
            let payload: serde_json::Value = response.json().unwrap();
            assert_eq!(
                sequences(&payload["entries"]),
                expected,
                "HTTP before={value}"
            );
        }
    }

    for (key, input, expected) in [
        ("uuids", json!(null), Some(visible)),
        ("uuids", json!([]), Some(visible)),
        ("uuids", json!(["malformed"]), Some(visible)),
        (
            "uuids",
            json!(["defabc20-d1ac-4c00-ab01-013cd84a0001"]),
            Some(visible),
        ),
        ("uuids", json!([1, {"key":true}]), Some(visible)),
        ("uuids", json!({"first":"malformed"}), Some(visible)),
        ("uuids", json!("malformed"), None),
        ("uuids", json!(1), None),
        ("take", json!(null), Some(visible)),
        ("take", json!(""), Some(visible)),
        ("take", json!(false), Some(empty)),
        ("take", json!("0"), Some(empty)),
        ("take", json!(true), Some(&[4])),
        ("take", json!(" 2 "), Some(&[4, 3])),
        ("take", json!("1.8"), Some(&[4])),
        ("take", json!(1.8), Some(&[4])),
        ("take", json!("1e0"), Some(&[4])),
        ("take", json!("-1.5"), Some(visible)),
        ("take", json!("9223372036854775807"), Some(visible)),
        ("take", json!("9223372036854775808"), None),
        ("take", json!("not-numeric"), None),
        ("take", json!([]), None),
        ("take", json!({}), None),
    ] {
        let response = api_call(
            &mut app,
            "POST",
            "/telescope/telescope-api/logs",
            json!({(key):input}),
        )
        .await;
        if let Some(expected) = expected {
            response.assert_ok();
            let payload: serde_json::Value = response.json().unwrap();
            assert_eq!(
                sequences(&payload["entries"]),
                expected,
                "HTTP {key}={input}"
            );
        } else {
            response.assert_status(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
}

#[tokio::test]
async fn management_commands_validate_types_preserve_json_and_pause_across_scopes() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/probe").await.unwrap().assert_ok();
    let result = app.artisan(["telescope:list", "event", "--json"]).await;
    result.assert_successful();
    let entries: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["content"]["name"], "probe");
    let id = entries[0]["id"].as_str().unwrap();
    let result = app
        .artisan(["telescope:show", id, "--json", "--type=cache,request"])
        .await;
    result.assert_successful();
    let shown: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
    assert_eq!(shown["entry"]["id"], id);
    assert_eq!(shown["batch"].as_array().unwrap().len(), 3);
    for args in [
        vec!["telescope:list", "nonsense"],
        vec!["telescope:list", "--limit=0"],
        vec!["telescope:show", "latest:invalid"],
        vec!["telescope:show", "missing"],
        vec!["telescope:prune", "--hours=invalid"],
    ] {
        assert_eq!(app.artisan(args).await.status, 1);
    }
    app.artisan(["telescope:pause"]).await.assert_successful();
    app.get("/api/probe").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    assert_eq!(
        repository
            .get(Some(EntryType::Event), EntryQueryOptions::default())
            .await
            .unwrap()
            .len(),
        1
    );
    app.artisan(["telescope:resume"]).await.assert_successful();
    app.get("/api/probe").await.unwrap().assert_ok();
    assert_eq!(
        repository
            .get(Some(EntryType::Event), EntryQueryOptions::default())
            .await
            .unwrap()
            .len(),
        2
    );
    app.artisan(["telescope:prune", "--hours=999999", "--keep-exceptions"])
        .await
        .assert_successful();
    app.artisan(["telescope:clear"]).await.assert_successful();
    assert!(
        repository
            .get(Some(EntryType::Event), EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    app.artisan(["telescope:publish"]).await.assert_successful();
    let config = app.root().join("config/telescope.rs");
    assert!(config.exists());
    std::fs::write(&config, "// custom").unwrap();
    app.artisan(["telescope:publish"]).await.assert_successful();
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "// custom");
    app.artisan(["telescope:publish", "--force"])
        .await
        .assert_successful();
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("TELESCOPE_ENABLED")
    );
}

fn retry(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let telescope = openrust_telescope::facades::Telescope::get_facade_root().unwrap();
        let entry =
            IncomingEntry::make(json!({"name":"pending"}).as_object().unwrap().clone()).unwrap();
        let id = entry.uuid;
        telescope.record_event(entry);
        telescope.store().await.unwrap();
        openrust_telescope::jobs::ProcessPendingUpdates::new(
            vec![openrust_telescope::EntryUpdate::make(
                id,
                EntryType::Event,
                json!({"complete":true}).as_object().unwrap().clone(),
            )],
            0,
        )
        .dispatch()
        .await
        .unwrap();
        // Missing entries reach the upstream three-attempt bound under the sync driver.
        openrust_telescope::jobs::ProcessPendingUpdates::new(
            vec![openrust_telescope::EntryUpdate::make(
                uuid::Uuid::now_v7(),
                EntryType::Job,
                serde_json::Map::new(),
            )],
            0,
        )
        .dispatch()
        .await
        .unwrap();
        Ok(openrust::http::Response::new(
            openrust::http::StatusCode::OK,
            openrust::http::Body::from(id.to_string()),
        ))
    })
}
#[tokio::test]
async fn registered_pending_update_job_uses_native_queue_and_bounds_missing_entry_retries() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/retry").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].content["complete"], true);
}

fn log_handler(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        openrust::log::Log::debug("ignored", &serde_json::Map::new()).unwrap();
        openrust::log::Log::error(
            "{a} {b}",
            json!({"a":"{b}","b":"value","telescope":["explicit"]})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        openrust::log::Log::error(
            "user context",
            json!({"exception":{"class":"UserData","message":"not a native error"}})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        openrust::log::Log::log_exception(
            openrust::log::Level::Error,
            "reported",
            &std::io::Error::other("source failure"),
            &serde_json::Map::new(),
        )
        .unwrap();
        Ok(openrust::http::Response::new(
            openrust::http::StatusCode::OK,
            openrust::http::Body::from("ok"),
        ))
    })
}
#[tokio::test]
async fn native_logger_observer_interpolates_once_and_records_real_error_snapshots() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.get("/api/logs").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let logs = repository
        .get(Some(EntryType::Log), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(logs.len(), 2);
    let log = logs
        .iter()
        .find(|entry| entry.content["message"] == "{b} value")
        .unwrap();
    assert_eq!(log.content["message"], "{b} value");
    assert!(log.content["context"].get("telescope").is_none());
    assert_eq!(
        repository.find(&log.id.to_string()).await.unwrap().tags,
        ["explicit"]
    );
    let exceptions = repository
        .get(Some(EntryType::Exception), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(exceptions.len(), 1);
    assert_eq!(exceptions[0].content["message"], "source failure");
    assert!(exceptions[0].content["file"].is_null());
    assert!(exceptions[0].content["line"].is_null());
    assert_eq!(exceptions[0].content["trace"], json!([]));
}

async fn api_call(
    app: &mut openrust::testing::TestApp,
    method: &str,
    path: &str,
    value: serde_json::Value,
) -> openrust::testing::TestResponse {
    app.call(
        http::Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("sec-fetch-site", "same-origin")
            .body(openrust::http::Body::from(value.to_string()))
            .unwrap(),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn dashboard_authorization_and_native_web_api_preserve_filters_and_mutations() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let denied = app.get("/telescope").await.unwrap();
    assert_eq!(
        denied.status(),
        openrust::http::StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(denied.body())
    );
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope
        .auth(|request| Box::pin(async move { Ok(request.header("x-allow")? == Some("yes")) }));
    app.get("/telescope")
        .await
        .unwrap()
        .assert_status(openrust::http::StatusCode::FORBIDDEN);
    telescope.auth(|_| Box::pin(async { Ok(true) }));
    app.get("/telescope").await.unwrap().assert_ok();
    app.get("/api/probe").await.unwrap().assert_ok();
    let listed = api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/events",
        json!({"take":1}),
    )
    .await;
    listed.assert_ok();
    let listed: serde_json::Value = listed.json().unwrap();
    assert_eq!(listed["status"], "enabled");
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    let id = listed["entries"][0]["id"].as_str().unwrap();
    let shown = app
        .get(&format!("/telescope/telescope-api/events/{id}"))
        .await
        .unwrap();
    shown.assert_ok();
    let shown: serde_json::Value = shown.json().unwrap();
    assert_eq!(shown["entry"]["id"], id);
    assert_eq!(shown["batch"].as_array().unwrap().len(), 4);
    api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/monitored-tags",
        json!({"tag":"probe"}),
    )
    .await
    .assert_ok();
    let tags = app
        .get("/telescope/telescope-api/monitored-tags")
        .await
        .unwrap();
    tags.assert_ok();
    assert_eq!(
        tags.json::<serde_json::Value>().unwrap()["tags"],
        json!(["probe"])
    );
    api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/monitored-tags/delete",
        json!({"tag":"probe"}),
    )
    .await
    .assert_ok();
    api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/toggle-recording",
        json!({}),
    )
    .await
    .assert_ok();
    let listed = api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/events",
        json!({}),
    )
    .await;
    assert_eq!(
        listed.json::<serde_json::Value>().unwrap()["status"],
        "paused"
    );
    api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/toggle-recording",
        json!({}),
    )
    .await
    .assert_ok();
    api_call(
        &mut app,
        "DELETE",
        "/telescope/telescope-api/entries",
        json!({}),
    )
    .await
    .assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    assert!(
        repository
            .get(None, EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
}

struct QueueProbe;
impl openrust::contracts::queue::JobHandler for QueueProbe {
    fn handle<'a>(
        &'a self,
        _: &'a mut openrust::queue::JobContext<'_>,
        payload: &'a openrust::queue::Payload,
    ) -> openrust::queue::QueueFuture<'a, openrust::queue::QueueResult<()>> {
        Box::pin(async move {
            openrust::log::Log::error("queue probe executed", &serde_json::Map::new()).unwrap();
            if payload.data()["fail"] == true {
                return Err(openrust::queue::QueueError::Execution {
                    source: std::sync::Arc::new(std::io::Error::other("queue probe failed")),
                });
            }
            Ok(())
        })
    }
}

fn async_handler(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        use openrust::{
            bus::Bus,
            queue::{DispatchOptions, Payload, QueueFacade},
        };
        let batch = Bus::batch([
            Payload::new("telescope.probe", json!({"fail":false})).unwrap(),
            Payload::new("telescope.probe", json!({"fail":false})).unwrap(),
        ])
        .name("observed batch")
        .on_connection("async")
        .dispatch()
        .await
        .unwrap();
        QueueFacade::dispatch(
            Payload::new("telescope.probe", json!({"fail":true})).unwrap(),
            DispatchOptions {
                connection: Some("async".into()),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap();
        Ok(openrust::http::Response::from(batch.id))
    })
}

#[tokio::test]
async fn native_database_worker_and_batch_bulk_keep_attempts_isolated_from_the_console() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
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
    connection
        .get_schema_builder()
        .create("job_batches", |table| {
            table.string("id", 255).primary();
            table.string("name", 255);
            table.integer("total_jobs");
            table.integer("pending_jobs");
            table.integer("failed_jobs");
            table.text("failed_job_ids");
            table.text("options").nullable();
            table.integer("cancelled_at").nullable();
            table.integer("created_at");
            table.integer("finished_at").nullable();
        })
        .await
        .unwrap();
    app.app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap()
        .register("telescope.probe", std::sync::Arc::new(QueueProbe))
        .unwrap();
    app.get("/api/async").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let jobs = repository
        .get(Some(EntryType::Job), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(jobs.len(), 3);
    assert!(
        jobs.iter()
            .all(|entry| entry.content["status"] == "pending")
    );
    for _ in 0..3 {
        app.artisan([
            "queue:work",
            "async",
            "--once",
            "--sleep=0",
            "--timeout=0",
            "--tries=1",
        ])
        .await
        .assert_successful();
    }
    let jobs = repository
        .get(Some(EntryType::Job), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(
        jobs.iter()
            .filter(|entry| entry.content["status"] == "processed")
            .count(),
        2
    );
    assert_eq!(
        jobs.iter()
            .filter(|entry| entry.content["status"] == "failed")
            .count(),
        1
    );
    let logs = repository
        .get(Some(EntryType::Log), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(
        logs.iter()
            .filter(|entry| entry.content["message"] == "queue probe executed")
            .count(),
        3
    );
    let batches = repository
        .get(Some(EntryType::Batch), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].content["pendingJobs"], 0);
    assert!(batches[0].content["finishedAt"].as_str().is_some());
    let commands = repository
        .get(Some(EntryType::Command), EntryQueryOptions::default())
        .await
        .unwrap();
    assert!(
        !commands
            .iter()
            .any(|entry| entry.content["command"] == "queue:work")
    );
}
fn job_handler(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        for fail in [false, true] {
            let result = openrust::queue::QueueFacade::dispatch(
                openrust::queue::Payload::new("telescope.probe", json!({"fail":fail})).unwrap(),
                openrust::queue::DispatchOptions::default(),
                None,
            )
            .await;
            assert_eq!(result.is_err(), fail);
        }
        Ok(openrust::http::Response::empty(
            openrust::http::StatusCode::OK,
        ))
    })
}
#[tokio::test]
async fn native_sync_jobs_record_payload_and_terminal_updates_in_the_original_batch() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap()
        .register("telescope.probe", std::sync::Arc::new(QueueProbe))
        .unwrap();
    app.get("/api/jobs").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let jobs = repository
        .get(Some(EntryType::Job), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(jobs.len(), 2);
    let failed = jobs
        .iter()
        .find(|entry| entry.content["status"] == "failed")
        .unwrap();
    assert_eq!(
        failed.content["exception"]["message"],
        "queue job execution failed: queue probe failed"
    );
    assert_eq!(
        repository.find(&failed.id.to_string()).await.unwrap().tags,
        ["failed"]
    );
    assert!(
        jobs.iter()
            .any(|entry| entry.content["status"] == "processed")
    );
}
#[tokio::test]
async fn request_received_loads_monitored_tags_before_application_recording() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    repository.monitor(&["tracked".into()]).await.unwrap();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.tag(|entry| {
        if entry.r#type == Some(EntryType::Event) {
            vec!["tracked".into()]
        } else {
            vec![]
        }
    });
    telescope.filter(|entry| entry.has_monitored_tag());
    app.get("/api/probe").await.unwrap().assert_ok();
    let entries = repository
        .get(None, EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].r#type, EntryType::Event);
}

#[tokio::test]
async fn console_show_full_preserves_unicode_payload_and_list_uses_type_columns() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let message = format!("{}TAIL", "中文".repeat(400));
    let context = format!("{}CONTEXT_END", "中文".repeat(400));
    let entry = IncomingEntry::make(
        json!({"level":"error","message":message,"context":context})
            .as_object()
            .unwrap()
            .clone(),
    )
    .unwrap()
    .r#type(EntryType::Log)
    .batch_id(uuid::Uuid::now_v7());
    let id = entry.uuid.to_string();
    repository.store(&[entry]).await.unwrap();
    let listed = app.artisan(["telescope:list", "log"]).await;
    listed.assert_successful();
    assert!(listed.stdout.contains("Level\tMessage"));
    assert!(!listed.stdout.contains("TAIL"));
    let short = app.artisan(["telescope:show", &id]).await;
    short.assert_successful();
    assert!(short.stdout.contains(&message));
    assert!(!short.stdout.contains("CONTEXT_END"));
    let full = app.artisan(["telescope:show", &id, "--full"]).await;
    full.assert_successful();
    assert!(full.stdout.contains(&message));
    assert!(full.stdout.contains(&context));
    let json = app.artisan(["telescope:show", &id, "--json"]).await;
    json.assert_successful();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(json.stdout.trim()).unwrap()["entry"]["content"]
            ["message"],
        message
    );
}

#[tokio::test]
async fn console_show_renders_fixed_batch_statistics_and_limits() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let batch_id = uuid::Uuid::now_v7();
    let make = |kind, content: serde_json::Value| {
        IncomingEntry::make(content.as_object().unwrap().clone())
            .unwrap()
            .r#type(kind)
            .batch_id(batch_id)
    };
    let request = make(EntryType::Request, json!({"method":"GET","uri":"/article"}));
    let id = request.uuid.to_string();
    let sql = format!("{}TAIL", "界".repeat(32));
    let mut rows = vec![
        request,
        make(
            EntryType::Job,
            json!({"name":"App::Translate","status":"processed"}),
        ),
        make(
            EntryType::Query,
            json!({"time":"1.005ms","sql":sql,"hash":"shared","slow":true,"file":"/srv/User.php","line":17}),
        ),
        make(EntryType::Cache, json!({"type":"hit","key":"article"})),
        make(
            EntryType::Query,
            json!({"time":2.346,"sql":"SELECT 2","hash":"shared","slow":"0"}),
        ),
        make(
            EntryType::Exception,
            json!({"class":"App::Failure","message":"broken","file":"/srv/Foo.php","line":9}),
        ),
        make(EntryType::Cache, json!({"type":"missed","key":"other"})),
        make(EntryType::Cache, json!({"type":"set","key":"updated"})),
        make(
            EntryType::Log,
            json!({"level":"warning","message":"threshold"}),
        ),
        make(EntryType::Event, json!({"name":"Translated"})),
    ];
    rows.extend((0..19).map(|i| {
        make(
            EntryType::Query,
            json!({"time":0,"sql":format!("SELECT {i} AS extra")}),
        )
    }));
    repository.store(&rows).await.unwrap();

    let result = app.artisan(["telescope:show", &id]).await;
    result.assert_successful();
    let batch = result
        .stdout
        .split_once("Related Entries - batch ")
        .unwrap()
        .1;
    assert!(batch.starts_with(&batch_id.to_string()[..8]));
    let mut ordered = batch;
    for text in [
        "Queries - 21 total, 3.35ms, 1 slow, 1 duplicate group",
        "#\tUUID\tTime\tSQL\tSource\tFlags",
        "\t1.01ms\t",
        "User.php:17\tSLOW DUP",
        "\t2.35ms\tSELECT 2\t\tDUP",
        "... and 1 more queries",
        "Exceptions - 1",
        "App::Failure: broken\t/srv/Foo.php:9",
        "Cache - 1 hits, 1 misses - 50% hit rate",
        "HIT\tarticle",
        "MISS\tother",
        "SET\tupdated",
        "Logs - 1",
        "warning\tthreshold",
        "Jobs (1)",
        "Events (1)",
    ] {
        ordered = ordered
            .split_once(text)
            .unwrap_or_else(|| panic!("missing {text:?} in {batch}"))
            .1;
    }
    assert!(!batch.contains("TAIL"));
    assert!(!batch.contains("SELECT 18 AS extra"));

    let full = app
        .artisan(["telescope:show", &id, "--type=query", "--full"])
        .await;
    full.assert_successful();
    assert!(full.stdout.contains(&sql));
    assert!(!full.stdout.contains("Cache - "));
    assert!(!full.stdout.contains("SELECT 18 AS extra"));
    let empty = app
        .artisan(["telescope:show", &id, "--type=mail, redis"])
        .await;
    empty.assert_successful();
    assert!(
        empty
            .stdout
            .contains("No batch entries of type mail, redis.")
    );
    let result = app
        .artisan(["telescope:show", &id, "--type=query", "--json"])
        .await;
    result.assert_successful();
    let result: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
    assert_eq!(result["batch"].as_array().unwrap().len(), 21);
    assert_eq!(result["batch"][0]["content"]["sql"], sql);
}

#[tokio::test]
async fn console_show_renders_request_exception_and_job_details() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let make = |kind, content: serde_json::Value| {
        IncomingEntry::make(content.as_object().unwrap().clone())
            .unwrap()
            .r#type(kind)
            .batch_id(uuid::Uuid::now_v7())
    };
    let response = format!("{}RESPONSE_END", "界".repeat(501));
    let data = format!("{}DATA_END", "界".repeat(251));
    let trace = (1..=16)
        .map(|line| json!({"file":format!("/src/frame{line}.rs"), "line":line}))
        .collect::<Vec<_>>();
    let request = make(
        EntryType::Request,
        json!({
            "method":"POST","uri":"/article","response_status":422,
            "controller_action":"ArticleController::store","duration":1.234,"memory":8,
            "ip_address":"192.0.2.20","middleware":["web","auth"],
            "user":{"name":"Reader","email":"reader@example.com","id":0},
            "payload":{"key":"value"},"response":response,
        }),
    );
    let exception = make(
        EntryType::Exception,
        json!({
            "class":"App::ArticleException","file":"/src/article.rs","line":4,"message":"Validation failed",
            "line_preview":{"3":"try {","4":"return Err(error);","5":"}"},"trace":trace,
        }),
    );
    let job = make(
        EntryType::Job,
        json!({
            "name":"App::Translate","queue":"local","connection":"redis","status":"failed","tries":0,"timeout":0,
            "data":{"target":data},"exception":{"message":"Worker exited","trace":trace},
        }),
    );
    let [request_id, exception_id, job_id] =
        [&request, &exception, &job].map(|entry| entry.uuid.to_string());
    repository.store(&[request, exception, job]).await.unwrap();
    let stored_request = repository.find(&request_id).await.unwrap();
    let hostname = format!(
        "Hostname\t{}",
        stored_request.content["hostname"].as_str().unwrap()
    );

    let request = app.artisan(["telescope:show", &request_id]).await;
    request.assert_successful();
    for expected in [
        "Request: POST /article -> 422",
        &hostname,
        "Controller\tArticleController::store",
        "Duration\t1.234ms",
        "Memory\t8MB",
        "IP\t192.0.2.20",
        "Middleware\tweb, auth",
        "User\tReader (reader@example.com) #0",
        "Payload",
        "\"key\": \"value\"",
    ] {
        assert!(
            request.stdout.contains(expected),
            "{expected}: {}",
            request.stdout
        );
    }
    assert!(!request.stdout.contains("RESPONSE_END"));
    let full = app.artisan(["telescope:show", &request_id, "--full"]).await;
    full.assert_successful();
    assert!(full.stdout.contains(&response));

    let exception = app.artisan(["telescope:show", &exception_id]).await;
    exception.assert_successful();
    for expected in [
        "Exception: ArticleException",
        "Class\tApp::ArticleException",
        "File\t/src/article.rs:4",
        "Occurrences\t1",
        "Resolved\tNo",
        "Validation failed",
        "Code Context",
        "4 >\treturn Err(error);",
        "Stack Trace",
        "/src/frame15.rs:15",
        "... and 1 more frames",
    ] {
        assert!(
            exception.stdout.contains(expected),
            "{expected}: {}",
            exception.stdout
        );
    }
    assert!(!exception.stdout.contains("/src/frame16.rs"));
    let job = app.artisan(["telescope:show", &job_id, "--full"]).await;
    job.assert_successful();
    for expected in [
        "Job: Translate [failed]",
        "Status\tfailed",
        "Queue\tlocal",
        "Connection\tredis",
        "Tries\t0",
        "Timeout\t0s",
        "Worker exited",
        "/src/frame10.rs:10",
        "... and 6 more frames",
        &data,
    ] {
        assert!(job.stdout.contains(expected), "{expected}: {}", job.stdout);
    }
    assert!(!job.stdout.contains("/src/frame11.rs"));
    let stored = app.artisan(["telescope:show", &job_id, "--json"]).await;
    stored.assert_successful();
    let stored: serde_json::Value = serde_json::from_str(stored.stdout.trim()).unwrap();
    assert_eq!(
        stored["entry"]["content"]["exception"]["trace"]
            .as_array()
            .unwrap()
            .len(),
        16
    );
}

#[tokio::test]
async fn request_watcher_records_resolved_address_and_redacted_session() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hide_request_parameters(&["_token", "nested.password"]);
    let cases = [
        (
            "/request-session",
            Some("10.0.0.1:1200"),
            "203.0.113.7, 198.51.100.9, 10.0.0.2",
            json!("198.51.100.9"),
        ),
        (
            "/api/probe",
            Some("198.51.100.2:1200"),
            "203.0.113.7",
            json!("198.51.100.2"),
        ),
        (
            "/api/probe",
            Some("[2001:db8::2]:1200"),
            "",
            json!("2001:db8::2"),
        ),
        ("/api/probe", None, "203.0.113.7", json!(null)),
    ];
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    for (path, peer, forwarded, expected) in cases {
        let mut request = http::Request::builder()
            .uri(path)
            .header("X-Forwarded-For", forwarded);
        if let Some(peer) = peer {
            request = request.extension(axum::extract::ConnectInfo(
                peer.parse::<std::net::SocketAddr>().unwrap(),
            ));
        }
        let response = app
            .call(request.body(openrust::http::Body::empty()).unwrap())
            .await
            .unwrap();
        response.assert_ok();
        let latest = repository
            .get(
                Some(EntryType::Request),
                EntryQueryOptions::default().limit(1),
            )
            .await
            .unwrap()
            .pop()
            .unwrap();
        assert_eq!(latest.content["ip_address"], expected, "{path} {peer:?}");
        assert_eq!(latest.content["uri"], path);
        assert!(latest.content["duration"].as_u64().is_some());
        if path == "/request-session" {
            assert_eq!(latest.content["session"]["_token"], "********");
            assert_eq!(
                latest.content["session"]["nested"],
                json!({"password":"********","remember":"public"})
            );
            assert!(!latest.content.to_string().contains("session-password"));
        } else {
            assert_eq!(latest.content["session"], json!([]));
        }
        assert!(latest.content["memory"].is_null());
    }
}

fn watched_response(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust::http::{Response, StatusCode};
    let kind = request
        .uri()
        .path()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    Box::pin(async move {
        let text = |body: String, status| {
            let mut response = Response::from(body);
            response.parts_mut().status = status;
            response.header(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/plain"),
            )
        };
        match kind.as_str() {
            "near-limit" => Ok(text("界".repeat(1999), StatusCode::OK)),
            "limit" => Ok(text("界".repeat(2000), StatusCode::OK)),
            "status-text" => Ok(text("filtered status".to_owned(), StatusCode::ACCEPTED)),
            "status-other" => Ok(text("other status".to_owned(), StatusCode::CREATED)),
            "304" => Ok(Response::empty(StatusCode::NOT_MODIFIED)),
            "json-302" => Ok(
                Response::json(StatusCode::FOUND, json!({"v":{},"visible":true}))?.header(
                    http::header::LOCATION,
                    http::HeaderValue::from_static("/next"),
                ),
            ),
            "text-302" => Ok(text("plain".to_owned(), StatusCode::FOUND)),
            "redirect" => Ok(openrust::support::facades::Redirect::away(
                "/next",
                Default::default(),
            )
            .unwrap()
            .into()),
            _ => Ok(text("plain".to_owned(), StatusCode::OK)),
        }
    })
}

fn request_watcher_options_config(
    base: &std::path::Path,
    env: &openrust::config::Environment,
) -> FixtureConfig {
    let mut config = provider_config(base, env)?;
    let watcher = &mut config.get_mut("telescope").unwrap()["watchers"]["RequestWatcher"];
    watcher["ignore_http_methods"] = json!(["gEt", "P*"]);
    watcher["ignore_status_codes"] = json!(["0202", "201x"]);
    watcher["size_limit"] = json!(1);
    Ok(config)
}

#[tokio::test]
async fn request_watcher_applies_fixed_filters_and_buffered_response_rules() {
    let mut app = application_with_config(request_watcher_options_config);
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let mut count = 0;
    for (method, case, status, expected) in [
        ("GET", "default", 200, None),
        ("POST", "default", 200, Some(json!("plain"))),
        ("POST", "status-text", 202, None),
        ("POST", "status-other", 201, Some(json!("other status"))),
        ("POST", "near-limit", 200, Some(json!("界".repeat(1999)))),
        ("POST", "limit", 200, Some(json!("Purged By Telescope"))),
        ("POST", "304", 304, Some(json!("Empty Response"))),
        (
            "POST",
            "json-302",
            302,
            Some(json!({"v":[],"visible":true})),
        ),
        ("POST", "text-302", 302, Some(json!("plain"))),
        ("POST", "redirect", 302, Some(json!("Redirected to /next"))),
    ] {
        let path = format!("/api/request-watcher/{case}");
        let request = http::Request::builder()
            .method(method)
            .uri(&path)
            .body(openrust::http::Body::empty())
            .unwrap();
        app.call(request)
            .await
            .unwrap()
            .assert_status(http::StatusCode::from_u16(status).unwrap());
        let records = repository
            .get(Some(EntryType::Request), EntryQueryOptions::default())
            .await
            .unwrap();
        if let Some(expected) = expected {
            count += 1;
            assert_eq!(records.len(), count, "{method} {case}");
            assert_eq!(records[0].content["uri"], path);
            assert_eq!(records[0].content["response"], expected, "{method} {case}");
        } else {
            assert_eq!(records.len(), count, "{method} {case}");
        }
    }
}

struct Projection(openrust::view::ViewData);

impl openrust::view::ViewObject for Projection {
    fn properties(&self) -> Option<&openrust::view::ViewData> {
        Some(&self.0)
    }

    fn call(
        &self,
        _: &str,
        _: &[openrust::view::ViewValue],
    ) -> openrust::view::ViewResult<openrust::view::ViewValue> {
        panic!("diagnostics must use the read-only projection")
    }
}

fn watched_view(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::{contracts::support::Responsable, support::facades::View, view::ViewValue};
        let data = [
            ("label".to_owned(), ViewValue::from("public")),
            (
                "metadata".to_owned(),
                ViewValue::from(json!({"rows":[2,3]})),
            ),
            ("secret".to_owned(), ViewValue::from("private-view-value")),
            (
                "object".to_owned(),
                ViewValue::from_object(std::sync::Arc::new(Projection(
                    [("projected".into(), "readable".into())].into(),
                ))),
            ),
        ]
        .into();
        View::get_facade_root()?
            .make("telescope-observed", data)?
            .to_response(request)
            .await
    })
}

#[tokio::test]
async fn request_watcher_records_rendered_view_and_explicit_bindings() {
    let mut app = application();
    let path = app
        .root()
        .join("resources/views/telescope-observed.blade.html");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "<p>{{ $label }} {{ $shared }} {{ $composer }}</p>").unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hide_response_parameters(&["data.secret"]);
    let views = app.app().make::<openrust::view::Factory>().await.unwrap();
    views.share("shared", "outside");
    views.composer("telescope-observed", |view| {
        view.data_mut().insert("composer".into(), "included".into());
        Ok(())
    });

    app.get("/api/request-view")
        .await
        .unwrap()
        .assert_ok()
        .assert_content("<p>public outside included</p>");
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    let snapshot = &entries[0].content["response"];
    assert_eq!(snapshot["view"], path.to_string_lossy().as_ref());
    assert_eq!(snapshot["data"]["label"], "public");
    assert_eq!(snapshot["data"]["metadata"], json!({"rows":[2,3]}));
    assert_eq!(snapshot["data"]["composer"], "included");
    assert_eq!(snapshot["data"]["secret"], "********");
    assert!(snapshot["data"].get("shared").is_none());
    assert!(
        snapshot["data"]["object"]["class"]
            .as_str()
            .unwrap()
            .ends_with("::Projection")
    );
    assert_eq!(
        snapshot["data"]["object"]["properties"],
        json!({"projected":"readable"})
    );
    assert!(
        !entries[0]
            .content
            .to_string()
            .contains("private-view-value")
    );
}

fn observed_plain_body(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let bytes = request.body_bytes().await?;
        assert_eq!(bytes.as_ref(), "Plain\n正文".as_bytes());
        Ok(openrust::http::Response::from("read plain"))
    })
}

fn observed_upload(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let first = request
            .file("form.attachments.0")
            .await?
            .expect("first upload");
        assert_eq!(first.get_client_original_name(), "proof.txt");
        assert_eq!(first.get_size(), 3);
        let empty = request
            .file("form.attachments.2")
            .await?
            .expect("zero byte upload");
        assert_eq!(empty.get_size(), 0);
        request.input(None).await?;
        Ok(openrust::http::Response::from("read uploads"))
    })
}

#[tokio::test]
async fn request_watcher_records_plain_text_and_nested_upload_metadata_without_reading_streams() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hide_request_parameters(&["form.attachments.2.name", "query_secret"]);
    let boundary = "openrust-test";
    let multipart = [
        ("form[attachments][0]", None, "old"),
        ("form[attachments][1]", None, "placeholder"),
        ("form[label]", None, "paper"),
        ("form[attachments][0]", Some("client/folder/proof.txt"), "abc"),
        ("form[attachments][2]", Some("empty.log"), ""),
    ]
    .into_iter()
    .map(|(name, filename, contents)| {
        let filename = filename.map_or_else(String::new, |filename| {
            format!("; filename=\"{filename}\"\r\nContent-Type: text/plain")
        });
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"{filename}\r\n\r\n{contents}\r\n"
        )
    })
    .collect::<String>() + &format!("--{boundary}--\r\n");
    let request = |path, content_type: &str, body| {
        http::Request::post(path)
            .header(http::header::CONTENT_TYPE, content_type)
            .body(body)
            .unwrap()
    };
    for path in [
        "/api/input?visible=ok&query_secret=hidden",
        "/api/observed-upload?visible=ok&query_secret=hidden",
    ] {
        app.call(request(
            path,
            &format!("multipart/form-data; boundary={boundary}"),
            openrust::http::Body::from(multipart.clone()),
        ))
        .await
        .unwrap()
        .assert_ok();
    }
    app.call(request(
        "/api/observed-plain?visible=excluded",
        "TEXT/PLAIN; charset=utf-8",
        openrust::http::Body::from("Plain\n正文"),
    ))
    .await
    .unwrap()
    .assert_ok();
    let unread = request(
        "/api/unread?visible=excluded",
        "text/plain",
        openrust::http::Body::from_stream(futures_util::stream::pending::<
            Result<&'static str, std::io::Error>,
        >()),
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), app.call(unread))
        .await
        .unwrap()
        .unwrap()
        .assert_ok();

    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let records = repository
        .get(Some(EntryType::Request), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(records.len(), 4);
    let expected = json!({
        "form":{
            "attachments":[{"name":"proof.txt","size":"0.003KB"}, "placeholder", {"name":"********","size":"0KB"}],
            "label":"paper",
        },
        "visible":"ok",
        "query_secret":"********",
    });
    for record in &records[2..] {
        assert_eq!(
            record.content["payload"], expected,
            "{}",
            record.content["uri"]
        );
    }
    assert_eq!(records[1].content["payload"], "Plain\n正文");
    assert!(records[0].content["payload"].is_null());
}

#[tokio::test]
async fn console_show_renders_fixed_details_for_remaining_entry_types() {
    use EntryType::*;
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let long_message = format!("{}TAIL", "界".repeat(500));
    let cases = [
        (
            Query,
            json!({"connection":"default","time":"2.5","slow":true,"sql":"select ?","bindings":[42],"file":"/app/query.rs","line":17}),
            "Query",
            &[
                "Connection\tdefault",
                "Duration\t2.5ms  SLOW",
                "Source\t/app/query.rs:17",
                "SQL\tselect ?",
                "Bindings",
            ][..],
        ),
        (
            Cache,
            json!({"type":"set","key":"metrics","expiration":0,"value":{"count":3}}),
            "Cache: set",
            &["Key\tmetrics", "Expiration\t0s", "Value", "\"count\": 3"],
        ),
        (
            Log,
            json!({"level":"warning","message":long_message,"context":{"limit":3}}),
            "Log: warning",
            &["Message\t", "TAIL", "Context", "\"limit\": 3"],
        ),
        (
            Mail,
            json!({"mailable":"App::AuditMail","subject":"Audit","queued":true,"to":{"m@example.org":"Reader","b@example.org":""},"from":{"s@example.org":"System"}}),
            "Mail: Audit",
            &[
                "Mailable\tApp::AuditMail",
                "Queued\tYes",
                "To\tReader <m@example.org>, b@example.org",
                "From\tSystem <s@example.org>",
            ],
        ),
        (
            Event,
            json!({"name":"Changed","broadcast":true,"listeners":[{"name":"sync"},{"name":"async","queued":true}],"payload":{"id":42}}),
            "Event: Changed",
            &["Broadcast\tYes", "Listeners", "async (queued)", "Payload"],
        ),
        (
            Command,
            json!({"command":"audit:run","exit_code":0,"arguments":{"id":42},"options":{"verbose":true}}),
            "Command: audit:run",
            &["Exit Code\t0", "Arguments", "Options"],
        ),
        (
            ScheduledTask,
            json!({"command":"audit:run","expression":"* * * * *","timezone":"UTC","description":"nightly","output":"done"}),
            "Scheduled Task: audit:run",
            &[
                "Expression\t* * * * *",
                "Timezone\tUTC",
                "Description\tnightly",
                "Output",
            ],
        ),
        (
            ClientRequest,
            json!({"method":"PATCH","uri":"https://example.org","duration":0,"payload":{"visible":true},"response":"document"}),
            "Client Request: PATCH https://example.org",
            &["Status\tN/A", "Duration\t0ms", "Payload", "Response"],
        ),
        (
            Gate,
            json!({"ability":"audit","result":"allowed","file":"/app/policy.rs","line":4,"arguments":[42]}),
            "Gate: audit: allowed",
            &["Source\t/app/policy.rs:4", "Arguments"],
        ),
        (
            Model,
            json!({"model":"App::Article","action":"updated","count":0,"changes":{"name":"new"}}),
            "Model: App::Article: updated",
            &["Count\t0", "Changes"],
        ),
        (
            Notification,
            json!({"notification":"App::Alert","channel":"mail","notifiable":"App::Account:42","queued":true}),
            "Notification: Alert",
            &[
                "Channel\tmail",
                "Notifiable\tApp::Account:42",
                "Queued\tYes",
            ],
        ),
        (
            View,
            json!({"name":"sample","path":"/app/views/sample.blade.html","composers":[{"name":"Bound","type":"class"}],"data":{"visible":true}}),
            "View: sample",
            &[
                "Path\t/app/views/sample.blade.html",
                "Composers",
                "Bound (class)",
                "Data",
            ],
        ),
        (
            Redis,
            json!({"connection":"default","time":1.5,"command":"GET audit"}),
            "Redis",
            &[
                "Connection\tdefault",
                "Duration\t1.5ms",
                "Command\tGET audit",
            ],
        ),
        (
            Dump,
            json!({"dump":"debug snapshot"}),
            "Dump",
            &["Content", "\"dump\": \"debug snapshot\""],
        ),
        (
            Batch,
            json!({"id":"aggregate","total_jobs":2}),
            "Batch",
            &["Content", "\"total_jobs\": 2"],
        ),
    ];
    for (kind, content, heading, expected) in cases {
        let entry = IncomingEntry::make(content.as_object().unwrap().clone())
            .unwrap()
            .r#type(kind)
            .batch_id(uuid::Uuid::now_v7());
        let id = entry.uuid.to_string();
        repository.store(&[entry]).await.unwrap();
        let result = app.artisan(["telescope:show", &id]).await;
        result.assert_successful();
        assert!(
            result
                .stdout
                .lines()
                .any(|line| line.trim() == format!("INFO  {heading}")),
            "{}",
            result.stdout
        );
        for value in expected {
            assert!(
                result.stdout.contains(value),
                "{kind:?} {value}: {}",
                result.stdout
            );
        }
    }
}

impl openrust::contracts::auth::Authenticatable for ObservedModel {
    fn get_auth_identifier_name(&self) -> &str {
        "id"
    }
    fn get_auth_identifier(&self) -> openrust::auth::AuthIdentifier {
        use openrust::database::eloquent::Model;
        openrust::auth::AuthIdentifier::try_from(self.get_key().unwrap()).unwrap()
    }
    fn get_auth_password_name(&self) -> &str {
        "password"
    }
    fn get_auth_password(&self) -> Option<&str> {
        None
    }
    fn get_remember_token(&self) -> Option<&str> {
        None
    }
    fn set_remember_token(&mut self, _: Option<String>) {}
    fn get_remember_token_name(&self) -> &str {
        "remember_token"
    }
}
fn identity_handler(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::{
            database::eloquent::Model,
            support::facades::{Auth, Db},
        };
        if request.path().ends_with("identity") {
            let connection = Db::connection(None).unwrap();
            let mut actor =
                ObservedModel::new_instance_on(&connection, Default::default()).unwrap();
            actor.set_attribute("id", 42_i64);
            actor.set_attribute("name", "Observed actor");
            actor.set_attribute("email", "actor@example.org");
            actor.set_attribute("password", "never-exported");
            Auth::guard(request, None)
                .unwrap()
                .set_user(Box::new(actor));
        }
        openrust_telescope::facades::Telescope::get_facade_root()
            .unwrap()
            .record_event(
                IncomingEntry::make(json!({"name":"identity"}).as_object().unwrap().clone())
                    .unwrap(),
            );
        Ok(openrust::http::Response::from("observed"))
    })
}
#[tokio::test]
async fn authenticated_snapshots_use_resolved_identity_without_lookup_or_cross_request_leakage() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let auth = app
        .app()
        .make::<openrust::auth::AuthManager>()
        .await
        .unwrap();
    auth.register_model::<ObservedModel>("observed");
    auth.via_request("cached", |_| {
        panic!("recording must never authenticate or look up a user")
    });
    app.get("/api/identity").await.unwrap().assert_ok();
    app.get("/api/guest").await.unwrap().assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries[0].content.get("user").is_none());
    assert_eq!(
        entries[1].content["user"],
        json!({"id":42,"name":"Observed actor","email":"actor@example.org"})
    );
    assert!(
        !serde_json::to_string(&entries)
            .unwrap()
            .contains("never-exported")
    );
}

fn client_handler(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::{http::client::Http, support::facades::Db};
        Db::connection(None)
            .unwrap()
            .select("SELECT 1 AS probe", &[])
            .await
            .unwrap();
        let url = request.headers()["x-origin-url"].to_str().unwrap();
        let data = json!({"same":"body","secret":"private","nested":{"password":"hidden"},"nested.password":"also-hidden"});
        assert_eq!(Http::post(url, &data).await.unwrap().status().as_u16(), 200);
        #[derive(serde::Serialize)]
        struct FormInput {
            same: &'static str,
            secret: &'static str,
            flag: bool,
            count: u32,
            #[serde(rename = "nested.password")]
            literal_password: &'static str,
        }
        let form = FormInput {
            same: "form",
            secret: "private",
            flag: false,
            count: 2,
            literal_password: "form-password",
        };
        assert_eq!(
            Http::pending_request()
                .unwrap()
                .as_form()
                .post(url, &form)
                .await
                .unwrap()
                .status()
                .as_u16(),
            200
        );
        let headers = http::HeaderMap::from_iter([
            (
                http::HeaderName::from_static("content-type"),
                http::HeaderValue::from_static("application/octet-stream"),
            ),
            (
                http::HeaderName::from_static("x-secret"),
                http::HeaderValue::from_static("header-private"),
            ),
        ]);
        let multipart = || {
            use openrust::http::client::Part;
            Http::pending_request()
                .unwrap()
                .attach_body(
                    "file",
                    vec![255_u8; 1500],
                    Some("file.bin"),
                    headers.clone(),
                    None,
                )
                .attach_body(
                    "same",
                    "replaced",
                    Some("replaced.txt"),
                    Default::default(),
                    None,
                )
                .attach_body(
                    "opaque",
                    "replaced",
                    Some("opaque.txt"),
                    Default::default(),
                    None,
                )
                .attach("opaque", Part::text("native").file_name("label.txt"))
                .attach("utf8-bytes", Part::bytes(b"native-bytes".as_slice()))
                .attach("anonymous", Part::stream_with_length("native-stream", 13))
                .attach(
                    "binary",
                    Part::bytes(vec![254, 255])
                        .file_name("raw.bin")
                        .mime_str("application/octet-stream")
                        .unwrap()
                        .headers(http::HeaderMap::from_iter([(
                            http::HeaderName::from_static("x-secret"),
                            http::HeaderValue::from_static("native-private"),
                        )])),
                )
        };
        let precise = 1.234_567_890_123_456_7_f64;
        let data = json!({"same":"multipart","enabled":true,"optional":null,"whole":1.0,
            "descriptor":{"name":"renamed","contents":precise,"filename":"typed.txt"},
            "hidden":{"name":"secret","contents":"private"}
        });
        assert_eq!(
            multipart()
                .post(url, &data)
                .await
                .unwrap()
                .status()
                .as_u16(),
            200
        );
        assert_eq!(
            multipart()
                .redact_payload()
                .post(url, &data)
                .await
                .unwrap()
                .status()
                .as_u16(),
            200
        );
        let failed = request.headers()["x-failed-url"].to_str().unwrap();
        assert!(multipart().post(failed, &data).await.is_err());
        assert!(Http::get(url).await.unwrap().ok());
        assert!(
            Http::pending_request()
                .unwrap()
                .with_body("opaque", http::HeaderValue::from_static("text/plain"))
                .get(url)
                .await
                .unwrap()
                .ok()
        );
        assert!(Http::post(failed, &json!({"failed": true})).await.is_err());
        for url in [url, url.split('?').next().unwrap()] {
            assert!(
                Http::pending_request()
                    .unwrap()
                    .with_query_parameters([("override".into(), "yes".into())])
                    .get(url)
                    .await
                    .unwrap()
                    .ok()
            );
        }
        assert!(
            Http::pending_request()
                .unwrap()
                .head(url)
                .await
                .unwrap()
                .ok()
        );
        assert!(
            Http::pending_request()
                .unwrap()
                .with_body(
                    r#"{"raw":true}"#,
                    http::HeaderValue::from_static("application/json")
                )
                .send(http::Method::POST, url)
                .await
                .unwrap()
                .ok()
        );
        assert!(
            Http::pending_request()
                .unwrap()
                .with_headers(http::HeaderMap::from_iter([(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("text/plain"),
                )]))
                .post(url, &json!({"typed":true}))
                .await
                .unwrap()
                .ok()
        );
        assert!(
            Http::pending_request()
                .unwrap()
                .with_query_parameters([("override".into(), "yes".into())])
                .redact_payload()
                .get(url)
                .await
                .unwrap()
                .ok()
        );
        let redirect_url = url.replace("/outgoing", "/redirect");
        assert!(
            Http::pending_request()
                .unwrap()
                .with_header(
                    http::header::HOST,
                    http::HeaderValue::from_static("custom.test")
                )
                .as_form()
                .post(
                    &redirect_url,
                    &json!({"flag":false,"count":2,"secret":"private"})
                )
                .await
                .unwrap()
                .ok()
        );
        assert!(Http::get(failed).await.is_err());
        assert!(
            Http::pending_request()
                .unwrap()
                .send(http::Method::OPTIONS, &redirect_url)
                .await
                .unwrap()
                .ok()
        );
        assert!(multipart().post(&redirect_url, &data).await.unwrap().ok());
        assert!(
            multipart()
                .redact_payload()
                .post(&redirect_url, &data)
                .await
                .unwrap()
                .ok()
        );
        let replay_url = url.replace("/outgoing", "/redirect307");
        let upload_path = request.headers()["x-upload-path"].to_str().unwrap();
        assert!(
            multipart()
                .attach_file("disk", upload_path)
                .await
                .unwrap()
                .post(&replay_url, &data)
                .await
                .unwrap()
                .ok()
        );
        let retry_url = url.replace("/outgoing", "/retry");
        assert!(
            multipart()
                .retry(2, std::time::Duration::ZERO)
                .retry_when_request(|_, request| {
                    request
                        .headers_mut()
                        .insert("x-retry", http::HeaderValue::from_static("yes"));
                    Ok(true)
                })
                .post(&retry_url, &data)
                .await
                .unwrap()
                .ok()
        );
        assert!(
            Http::pending_request()
                .unwrap()
                .as_multipart()
                .post(url, &json!([]))
                .await
                .unwrap()
                .ok()
        );
        for fields in [
            vec![("0", "first"), ("1", "hidden-index"), ("0", "last")],
            vec![("1", "hidden-index"), ("0", "zero")],
            vec![("0", "zero"), ("2", "two")],
            vec![("0", "zero"), ("01", "leading-zero")],
        ] {
            let data: Vec<_> = fields
                .into_iter()
                .map(|(name, contents)| json!({"name": name, "contents": contents}))
                .collect();
            assert!(
                Http::pending_request()
                    .unwrap()
                    .as_multipart()
                    .post(url, &data)
                    .await
                    .unwrap()
                    .ok()
            );
        }
        Ok(openrust::http::Response::from("observed"))
    })
}

const MULTIPART_REPLACEMENTS: [(&str, Option<&str>, &str); 5] = [
    ("same-boundary", None, "new multipart body"),
    (
        "changed-boundary",
        Some("multipart/form-data; boundary=changed"),
        "new multipart body",
    ),
    ("json", Some("application/json"), r#"{"changed":true}"#),
    (
        "form",
        Some("application/x-www-form-urlencoded"),
        "changed=true",
    ),
    ("plain", Some("text/plain"), "new text body"),
];

fn multipart_middleware_handler(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::http::client::{Http, Part};

        let origin = request.headers()["x-origin-url"].to_str().unwrap();
        let unreachable = request.headers()["x-failed-url"].to_str().unwrap();
        for (phase, target, fails) in [
            ("before", origin, false),
            ("request", origin, false),
            ("empty", origin, false),
            ("before", unreachable, true),
            ("request", unreachable, true),
        ] {
            for (mode, media, body) in MULTIPART_REPLACEMENTS {
                if phase == "empty" && media.is_none_or(|media| media.starts_with("multipart")) {
                    continue;
                }
                let update = move |request: &mut openrust::http::client::Request| {
                    if let Some(media) = media {
                        request.headers_mut().insert(
                            http::header::CONTENT_TYPE,
                            http::HeaderValue::from_static(media),
                        );
                    }
                    request.headers_mut().remove(http::header::CONTENT_LENGTH);
                    *request.body_mut() = Some(body.into());
                    Ok(())
                };
                let pending = Http::pending_request().unwrap().as_multipart();
                let pending = if phase == "before" {
                    pending.before_sending(update)
                } else {
                    pending.with_request_middleware(update)
                };
                let (pending, data) = if phase == "empty" {
                    (pending, json!([]))
                } else {
                    (
                        pending
                            .attach(
                                "stream",
                                Part::stream_with_length("synthetic-stream", 16)
                                    .file_name("stream.txt"),
                            )
                            .attach("text", Part::text("buffered-text").file_name("text.txt"))
                            .attach("bytes", Part::bytes(vec![0xfe])),
                        json!([
                            {"name":"original","contents":false,"headers":{"X-Secret":"private"},"extra":"kept"},
                            {"name":"original","contents":true},
                        ]),
                    )
                };
                let result = pending
                    .post(&format!("{target}/{phase}/{mode}"), &data)
                    .await;
                if fails {
                    assert!(matches!(
                        result,
                        Err(openrust::http::client::Error::Connection)
                    ));
                } else {
                    assert!(result.unwrap().ok());
                }
            }
        }
        Ok(openrust::http::Response::from("observed"))
    })
}

#[tokio::test]
async fn multipart_media_rewrites_preserve_the_correct_telescope_diagnostic() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let unavailable = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let failed = unavailable.local_addr().unwrap();
    drop(unavailable);
    let wire = tokio::spawn(async move {
        for _ in 0..13 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let read = socket.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&buffer[..read]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&request[..end]).unwrap();
                    let mode = headers
                        .split_whitespace()
                        .nth(1)
                        .unwrap()
                        .rsplit('/')
                        .next()
                        .unwrap();
                    let (_, media, body) = MULTIPART_REPLACEMENTS
                        .iter()
                        .find(|case| case.0 == mode)
                        .unwrap();
                    let headers = headers.to_ascii_lowercase();
                    assert!(headers.contains(&format!(
                        "content-type: {}",
                        media.unwrap_or("multipart/form-data; boundary=")
                    )));
                    if request.len() >= end + 4 + body.len() {
                        assert_eq!(&request[end + 4..], body.as_bytes());
                        break;
                    }
                }
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
    });
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hide_request_headers(&["x-secret"]);
    telescope.hide_request_parameters(&["0.extra"]);
    let request = http::Request::builder()
        .uri("/api/client-multipart-middleware")
        .header("x-origin-url", format!("http://{address}"))
        .header("x-failed-url", format!("http://{failed}"))
        .body(openrust::http::Body::empty())
        .unwrap();
    let (response, ()) = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        tokio::try_join!(app.call(request), async {
            wire.await
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
        })
    })
    .await
    .unwrap()
    .unwrap();
    response.assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::ClientRequest), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 23);
    let fields = json!({
        "original":true,
        "stream":{"name":"stream.txt", "size":"0.016KB", "headers":[]},
        "text":"buffered-text",
        "bytes":{"name":null, "size":"0.001KB", "headers":[]},
    });
    let descriptors = json!([
        {"name":"original","contents":false,"headers":{"X-Secret":"********"},"extra":"********"},
        {"name":"original","contents":true},
        {"name":"stream","contents":{},"filename":"stream.txt","headers":[]},
        {"name":"text","contents":"buffered-text","filename":"text.txt","headers":[]},
        {"name":"bytes","contents":"�","headers":[]},
    ]);
    for entry in entries {
        let uri = entry.content["uri"].as_str().unwrap();
        let (base, mode) = uri.rsplit_once('/').unwrap();
        let phase = base.rsplit('/').next().unwrap();
        let failed = entry.content["response_status"].is_null();
        let (_, media, _) = MULTIPART_REPLACEMENTS
            .iter()
            .find(|case| case.0 == mode)
            .unwrap();
        let changed = || match mode {
            "json" => json!({"changed":true}),
            "form" => json!({"changed":"true"}),
            _ => json!([]),
        };
        let expected = if failed || phase == "empty" {
            changed()
        } else if phase == "before" || media.is_none_or(|media| media.starts_with("multipart")) {
            fields.clone()
        } else {
            descriptors.clone()
        };
        assert_eq!(entry.content["payload"], expected, "{uri}, failed={failed}");
        let media = if !failed && phase == "before" {
            None
        } else {
            *media
        };
        assert!(
            entry.content["headers"]["content-type"]
                .as_str()
                .unwrap()
                .starts_with(media.unwrap_or("multipart/form-data; boundary=")),
            "{uri}"
        );
    }
}

#[tokio::test]
async fn list_renders_fixed_plain_text_values_for_request_and_cache_rows() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let clipped_uri = format!("{}...", "界".repeat(20));
    let mut cases = vec![
        (
            EntryType::Request,
            json!({"method":"GET","uri":"界".repeat(21),"duration":0}),
            vec!["GET", clipped_uri.as_str(), "0", "0ms"],
        ),
        (
            EntryType::ClientRequest,
            json!({"method":"GET","uri":"/external"}),
            vec!["GET", "/external", "N/A", ""],
        ),
        (
            EntryType::ClientRequest,
            json!({"method":"GET","uri":"/external","response_status":"0200"}),
            vec!["GET", "/external", "200", ""],
        ),
    ];
    cases.extend(
        [
            ("hit", "HIT"),
            ("missed", "MISS"),
            ("set", "SET"),
            ("forget", "FORGET"),
            ("other", "other"),
        ]
        .map(|(action, label)| {
            (
                EntryType::Cache,
                json!({"type":action,"key":"value"}),
                vec![label, "value"],
            )
        }),
    );
    for (index, (kind, content, expected)) in cases.into_iter().enumerate() {
        let tag = format!("case-{index}");
        let entry = IncomingEntry::make(content.as_object().unwrap().clone())
            .unwrap()
            .r#type(kind)
            .batch_id(uuid::Uuid::now_v7())
            .tags([tag.clone()]);
        let id = entry.uuid.to_string();
        repository.store(&[entry]).await.unwrap();
        let result = app
            .artisan(["telescope:list", kind.as_str(), "--tag", &tag])
            .await;
        result.assert_successful();
        let row = result
            .stdout
            .lines()
            .find(|line| line.starts_with(&id[..8]))
            .unwrap();
        let fields: Vec<_> = row.split('\t').skip(1).collect();
        assert_eq!(&fields[..fields.len() - 1], expected, "{tag}");
    }
}

#[tokio::test]
async fn console_list_and_show_format_utc_relative_timestamps() {
    use openrust::support::{Date, DateTimezone};

    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let batch = uuid::Uuid::now_v7();
    for (created, now, expected) in [
        (
            "2026-11-26 12:00:00",
            "2026-11-26T12:00:00Z",
            "0 seconds ago",
        ),
        (
            "2026-11-26 12:00:00",
            "2026-11-26T11:59:59.6Z",
            "0 seconds from now",
        ),
        (
            "2026-11-26 11:59:00",
            "2026-11-26T11:59:59.999999Z",
            "59 seconds ago",
        ),
        (
            "2026-11-26 11:59:00",
            "2026-11-26T12:00:00Z",
            "1 minute ago",
        ),
        (
            "2026-11-26 15:00:00",
            "2026-11-26T12:00:00Z",
            "3 hours from now",
        ),
        ("2026-11-25 12:00:00", "2026-11-26T12:00:00Z", "1 day ago"),
        ("2026-11-19 12:00:00", "2026-11-26T12:00:00Z", "1 week ago"),
        ("2026-10-26 12:00:00", "2026-11-26T12:00:00Z", "1 month ago"),
        ("2026-01-31 12:00:00", "2026-02-28T12:00:00Z", "4 weeks ago"),
        ("2026-01-31 12:00:00", "2026-03-03T11:00:00Z", "4 weeks ago"),
        ("2026-01-31 12:00:00", "2026-03-03T12:00:00Z", "1 month ago"),
        (
            "2026-03-03 11:00:00",
            "2026-01-31T12:00:00Z",
            "1 month from now",
        ),
        (
            "2024-02-29 12:00:00",
            "2025-02-28T12:00:00Z",
            "11 months ago",
        ),
        ("2024-02-29 12:00:00", "2025-03-01T12:00:00Z", "1 year ago"),
    ] {
        let mut entry = IncomingEntry::make(serde_json::Map::new())
            .unwrap()
            .r#type(EntryType::Log)
            .batch_id(batch);
        entry.recorded_at = format!("{created}Z").parse().unwrap();
        let id = entry.uuid.to_string();
        repository.store(&[entry]).await.unwrap();
        app.travel_to(Date::from_utc(now.parse().unwrap(), DateTimezone::UTC));

        let list = app.artisan(["telescope:list", "log"]).await;
        list.assert_successful();
        let row = list
            .stdout
            .lines()
            .find(|line| line.starts_with(&id[..8]))
            .unwrap();
        assert_eq!(
            row.split('\t').next_back(),
            Some(expected),
            "{created} at {now}"
        );

        let show = app.artisan(["telescope:show", &id]).await;
        show.assert_successful();
        assert!(
            show.stdout
                .contains(&format!("Time\t{expected}  ({created})")),
            "{}",
            show.stdout
        );
        let json = app.artisan(["telescope:show", &id, "--json"]).await;
        json.assert_successful();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json.stdout).unwrap()["entry"]["created_at"],
            created
        );
    }
}

#[tokio::test]
async fn prune_fractional_hours_updates_real_storage_and_preserves_exceptions() {
    use openrust::support::{Date, DateTimezone};

    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.travel_to(Date::from_utc(
        "2026-11-26T13:00:00Z".parse().unwrap(),
        DateTimezone::UTC,
    ));
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let batch = uuid::Uuid::now_v7();
    let entry = |kind, recorded_at: &str| {
        let mut entry = IncomingEntry::make(serde_json::Map::new())
            .unwrap()
            .r#type(kind)
            .batch_id(batch);
        entry.recorded_at = recorded_at.parse().unwrap();
        entry
    };
    let old = entry(EntryType::Log, "2026-11-26T12:29:59Z");
    let boundary = entry(EntryType::Log, "2026-11-26T12:30:00Z");
    let exception = entry(EntryType::Exception, "2026-11-26T12:10:00Z")
        .with_family_hash(Some("prune-test".into()));
    let [old, boundary, exception] = [old, boundary, exception].map(|entry| {
        let id = entry.uuid.to_string();
        (entry, id)
    });
    repository
        .store(&[old.0, boundary.0, exception.0])
        .await
        .unwrap();

    app.artisan(["telescope:prune", "--hours=.5", "--keep-exceptions"])
        .await
        .assert_successful();
    assert!(matches!(
        repository.find(&old.1).await,
        Err(openrust_telescope::Error::NotFound)
    ));
    assert!(repository.find(&boundary.1).await.is_ok());
    assert!(repository.find(&exception.1).await.is_ok());
}

const CLIENT_REQUEST_SHAPE: &str = r#"{"empty":{},"list":{"0":"first","1":"second"},"reverse":{"1":{},"0":{"0":"nested"}},"leading":{"01":"value"},"array":[{},{"0":"inner"}]}"#;

fn client_request_shape_handler(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::http::client::{Error, Http};

        let origin = request.headers()["x-origin-url"].to_str().unwrap();
        let unavailable = request.headers()["x-failed-url"].to_str().unwrap();
        let raw = || {
            Http::pending_request().unwrap().with_body(
                CLIENT_REQUEST_SHAPE,
                http::HeaderValue::from_static("application/json"),
            )
        };
        raw()
            .send(http::Method::POST, &format!("{origin}/raw"))
            .await
            .unwrap();
        let structured: serde_json::Value = serde_json::from_str(CLIENT_REQUEST_SHAPE).unwrap();
        Http::pending_request()
            .unwrap()
            .post(&format!("{origin}/structured"), &structured)
            .await
            .unwrap();
        assert!(matches!(
            raw()
                .send(http::Method::POST, &format!("{unavailable}/failed"))
                .await,
            Err(Error::Connection)
        ));
        Ok(openrust::http::Response::from("observed"))
    })
}

#[tokio::test]
async fn raw_client_json_diagnostics_match_php_array_shapes() {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let origin = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = origin.local_addr().unwrap();
    let unavailable = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let failed = unavailable.local_addr().unwrap();
    drop(unavailable);
    let wire = tokio::spawn(async move {
        for path in ["raw", "structured"] {
            let (socket, _) = origin.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
            assert!(line.starts_with(&format!("POST /{path} ")));
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0; CLIENT_REQUEST_SHAPE.len()];
            socket.read_exact(&mut body).await.unwrap();
            assert_eq!(body, CLIENT_REQUEST_SHAPE.as_bytes(), "{path}");
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        }
    });
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.app()
        .make::<Telescope>()
        .await
        .unwrap()
        .hide_request_parameters(&["list.1"]);
    let request = http::Request::builder()
        .uri("/api/client-request-shape")
        .header("x-origin-url", format!("http://{address}"))
        .header("x-failed-url", format!("http://{failed}"))
        .body(openrust::http::Body::empty())
        .unwrap();
    let response = tokio::time::timeout(std::time::Duration::from_secs(30), app.call(request))
        .await
        .unwrap()
        .unwrap();
    response.assert_ok();
    wire.await.unwrap();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::ClientRequest), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 3);
    let array_shape = json!({
        "empty":[], "list":["first","********"], "reverse":{"1":[],"0":["nested"]},
        "leading":{"01":"value"}, "array":[[],["inner"]],
    });
    let mut object_shape: serde_json::Value = serde_json::from_str(CLIENT_REQUEST_SHAPE).unwrap();
    object_shape["list"]["1"] = json!("********");
    for (path, status, expected) in [
        ("raw", Some(200), &array_shape),
        ("structured", Some(200), &object_shape),
        ("failed", None, &array_shape),
    ] {
        let entry = entries
            .iter()
            .find(|entry| {
                entry.content["uri"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("/{path}"))
            })
            .unwrap();
        assert_eq!(
            entry.content.get("response_status").cloned(),
            status.map(|code| json!(code))
        );
        assert_eq!(&entry.content["payload"], expected, "{path}");
    }
}

fn client_response_shape_handler(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    let target = request.headers()["x-origin-url"]
        .to_str()
        .unwrap()
        .to_owned();
    Box::pin(async move {
        openrust::http::client::Http::pending_request()
            .unwrap()
            .without_redirecting()
            .get(&target)
            .await
            .unwrap();
        Ok(openrust::http::Response::from("observed"))
    })
}

#[tokio::test]
async fn real_client_responses_preserve_php_body_shape_and_configured_size_limit() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let cases = [
        ("html-zero", 200, "text/html", "0", json!("Empty Response")),
        (
            "html-spaced-zero",
            200,
            "text/html",
            " 0",
            json!("HTML Response"),
        ),
        ("plain-zero", 200, "text/plain", "0", json!("0")),
        (
            "json-zero",
            200,
            "application/json",
            "0",
            json!("Empty Response"),
        ),
        (
            "zero-redirect",
            302,
            "text/html",
            "0",
            json!("Redirected to /next"),
        ),
        ("empty-object", 200, "application/json", "{}", json!([])),
        (
            "container-list",
            200,
            "application/json",
            r#"{"0":{"0":"zero","1":"hidden"},"1":{}}"#,
            json!([["zero", "********"], []]),
        ),
        (
            "out-of-order-object",
            200,
            "application/json",
            r#"{"1":"one","0":"zero"}"#,
            json!({"1":"one","0":"zero"}),
        ),
        (
            "leading-zero-object",
            200,
            "application/json",
            r#"{"00":"first","1":"one"}"#,
            json!({"00":"first","1":"one"}),
        ),
        (
            "fractional-limit",
            200,
            "text/plain",
            "1234567890123456789012345678901234567890é",
            json!("Purged By Telescope"),
        ),
        (
            "fractional-limit-boundary",
            200,
            "text/plain",
            "123456789012345678901234567890123456789é",
            json!("123456789012345678901234567890123456789é"),
        ),
    ];
    let origin = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = origin.local_addr().unwrap();
    let wire = tokio::spawn({
        let cases = cases.clone();
        async move {
            for (name, status, media, body, _) in cases {
                let (mut socket, _) = origin.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut buffer = [0; 4096];
                    let size = socket.read(&mut buffer).await.unwrap();
                    assert_ne!(size, 0);
                    request.extend_from_slice(&buffer[..size]);
                }
                assert!(request.starts_with(format!("GET /{name} ").as_bytes()));
                let response = format!(
                    "HTTP/1.1 {status} Response\r\nContent-Type: {media}\r\nContent-Length: {}\r\nLocation: /next\r\nConnection: close\r\n\r\n{body}",
                    body.len(),
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        }
    });
    let mut app = application_with_config(client_response_config);
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.app()
        .make::<Telescope>()
        .await
        .unwrap()
        .hide_response_parameters(&["0.1"]);
    for (name, _, _, _, _) in &cases {
        let request = http::Request::builder()
            .uri("/api/client-response-shape")
            .header("x-origin-url", format!("http://{address}/{name}"));
        let response = app
            .call(request.body(openrust::http::Body::empty()).unwrap())
            .await
            .unwrap();
        response.assert_ok();
    }
    wire.await.unwrap();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::ClientRequest), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), cases.len());
    for (name, status, _, _, expected) in cases {
        let entry = entries
            .iter()
            .find(|entry| {
                entry.content["uri"]
                    .as_str()
                    .unwrap()
                    .ends_with(&format!("/{name}"))
            })
            .unwrap();
        assert_eq!(entry.content["response_status"], json!(status));
        assert_eq!(entry.content["response"], expected, "{name}");
    }
}

#[tokio::test]
async fn actual_http_client_preserves_decoded_json_form_query_redaction_and_native_sql_observation()
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let redirect_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let redirect_address = redirect_listener.local_addr().unwrap();
    let origin = tokio::spawn(async move {
        for index in 0..29 {
            let target = if matches!(index, 17 | 19) {
                &redirect_listener
            } else {
                &listener
            };
            let (mut socket, _) = target.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0_u8; 4096];
                let size = socket.read(&mut buffer).await.unwrap();
                assert!(size > 0);
                bytes.extend_from_slice(&buffer[..size]);
                if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let size = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + size {
                        if matches!(index, 12 | 16 | 18) {
                            assert!(headers.starts_with("post /redirect?"));
                        }
                        if index == 12 {
                            assert!(headers.lines().any(|line| line == "host: custom.test"));
                        }
                        if matches!(index, 13 | 17 | 19) {
                            assert!(headers.starts_with("get /final?destination=value "));
                            assert_eq!(size, 0);
                            assert!(!headers.contains("transfer-encoding:"));
                            let authority = if index == 13 {
                                address
                            } else {
                                redirect_address
                            };
                            let host = format!("host: {authority}");
                            assert!(headers.lines().any(|line| line == host));
                            if index == 13 {
                                assert!(
                                    headers.contains(
                                        "content-type: application/x-www-form-urlencoded"
                                    )
                                );
                            } else {
                                assert!(
                                    headers
                                        .contains("content-type: multipart/form-data; boundary=")
                                );
                            }
                        }
                        if index == 14 {
                            assert!(headers.starts_with("options /redirect?"));
                        }
                        if index == 15 {
                            assert!(headers.starts_with("options /final?destination=value "));
                            assert_eq!(size, 0);
                        }
                        if index == 1 {
                            assert_eq!(
                                &bytes[end + 4..],
                                b"same=form&secret=private&flag=false&count=2&nested.password=form-password"
                            );
                        }
                        if (6..8).contains(&index) || index == 11 {
                            assert!(headers.starts_with("get /outgoing?override=yes "));
                        }
                        if index == 10 {
                            assert!(headers.contains("content-type: text/plain"));
                        }
                        if index == 20 {
                            assert!(headers.starts_with("post /redirect307?"));
                        }
                        if index == 21 {
                            assert!(headers.starts_with("post /final?destination=value "));
                        }
                        if matches!(index, 22 | 23) {
                            assert!(headers.starts_with("post /retry?"));
                            assert_eq!(headers.contains("x-retry: yes"), index == 23);
                        }
                        if matches!(index, 2 | 3 | 16 | 18 | 20..=23) {
                            let body = &bytes[end + 4..];
                            assert!(headers.contains("multipart/form-data; boundary="));
                            assert!(body.windows(1500).any(|bytes| bytes == [255_u8; 1500]));
                            assert!(body.windows(6).any(|bytes| bytes == b"\r\n\xfe\xff\r\n"));
                            let body = String::from_utf8_lossy(body);
                            assert!(body.contains("filename=\"file.bin\""));
                            if matches!(index, 20 | 21) {
                                assert!(body.contains("filename=\"upload.txt\""));
                                assert!(body.contains("file-source-bytes"));
                            }
                            assert!(
                                body.contains(
                                    "name=\"opaque\"; filename=\"label.txt\"\r\n\r\nnative"
                                )
                            );
                            assert!(body.contains("name=\"utf8-bytes\"\r\n\r\nnative-bytes"));
                            assert!(body.contains("name=\"anonymous\"\r\n\r\nnative-stream"));
                            assert!(body.contains("filename=\"raw.bin\""));
                            assert!(body.contains("name=\"renamed\"; filename=\"typed.txt\""));
                            assert!(body.contains("\r\n\r\n1.2345678901235\r\n"));
                            assert!(
                                body.contains("name=\"whole\"\r\ncontent-length: 1\r\n\r\n1\r\n")
                            );
                            let original = "name=\"same\"\r\ncontent-length: 9\r\n\r\nmultipart";
                            assert!(body.contains(original));
                            assert!(
                                body.find(original).unwrap()
                                    < body.find("filename=\"replaced.txt\"").unwrap()
                            );
                        }
                        break;
                    }
                }
            }
            if index == 12 || index == 14 {
                socket.write_all(b"HTTP/1.1 302 Found\r\nLocation: /final?destination=value\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                continue;
            }
            if matches!(index, 16 | 18) {
                let response = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{redirect_address}/final?destination=value\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                continue;
            }
            if index == 20 {
                socket.write_all(b"HTTP/1.1 307 Temporary Redirect\r\nLocation: /final?destination=value\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                continue;
            }
            if index == 22 {
                socket.write_all(b"HTTP/1.1 422 Unprocessable Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                continue;
            }
            let body = r#"{"token":"secret","value":true}"#;
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
        }
    });
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hide_request_parameters(&["secret", "nested.password", "1"]);
    telescope.hide_response_parameters(&["token"]);
    telescope.hide_request_headers(&["x-secret"]);
    let failed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let failed_address = failed.local_addr().unwrap();
    drop(failed);
    let upload_dir = tempfile::tempdir().unwrap();
    let upload_path = upload_dir.path().join("upload.txt");
    std::fs::write(&upload_path, b"file-source-bytes").unwrap();
    let request = http::Request::builder()
        .uri("/api/client")
        .header(
            "x-origin-url",
            format!("http://{address}/outgoing?same=query&query=value"),
        )
        .header(
            "x-failed-url",
            format!("http://{failed_address}/failed?query=value"),
        )
        .header("x-upload-path", upload_path.to_str().unwrap())
        .body(openrust::http::Body::empty())
        .unwrap();
    // Windows 会为每次关闭端口的连接保留约两秒的拒绝等待。
    let (response, ()) = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        tokio::try_join!(app.call(request), async {
            origin
                .await
                .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
        })
    })
    .await
    .unwrap()
    .unwrap();
    response.assert_ok();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::ClientRequest), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 27);
    for (entry, expected) in entries[..4].iter().zip([
        json!({"0":"zero","01":"leading-zero"}),
        json!({"0":"zero","2":"two"}),
        json!({"1":"********","0":"zero"}),
        json!(["last", "********"]),
    ]) {
        assert_eq!(entry.content["payload"], expected);
    }
    let entries = &entries[4..];
    assert_eq!(entries[0].content["payload"], json!([]));
    assert_eq!(entries[0].content["response_status"], 200);
    assert!(
        entries[0].content["headers"]["content-type"]
            .as_str()
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    let entries = &entries[1..];
    let payload = json!({
        "same":{"name":"replaced.txt","size":"0.008KB","headers":[]},
        "secret":"********","enabled":true,"optional":null,"whole":1.0,"renamed":1.234_567_890_123_456_7_f64,
        "file":{"name":"file.bin","size":"1.5KB","headers":{"content-type":"application/octet-stream","x-secret":"********"}},
        "opaque":"native",
        "utf8-bytes":"native-bytes",
        "anonymous":{"name":null,"size":"0.013KB","headers":[]},
        "binary":{"name":"raw.bin","size":"0.002KB","headers":{"content-type":"application/octet-stream","x-secret":"********"}}
    });
    for (entry, (path, status)) in entries[..3].iter().zip([
        ("/retry?same=query&query=value", 200),
        ("/retry?same=query&query=value", 422),
        ("/final?destination=value", 200),
    ]) {
        assert_eq!(entry.content["uri"], format!("http://{address}{path}"));
        assert_eq!(entry.content["method"], "POST");
        assert_eq!(entry.content["response_status"], status);
        let mut expected = payload.clone();
        if path.starts_with("/final") {
            expected["disk"] = json!({
                "name":"upload.txt",
                "size":"0.017KB",
                "headers":{"content-type":"text/plain"}
            });
        }
        assert_eq!(entry.content["payload"], expected);
    }
    assert_eq!(entries[0].content["headers"]["x-retry"], "yes");
    assert!(entries[1].content["headers"].get("x-retry").is_none());
    let entries = &entries[3..];
    let final_url = format!("http://{redirect_address}/final");
    assert_eq!(entries[0].content["uri"], final_url);
    assert_eq!(entries[0].content["method"], "GET");
    assert_eq!(entries[0].content["payload"], json!([]));
    assert_eq!(
        entries[1].content["uri"],
        format!("{final_url}?destination=value")
    );
    assert_eq!(entries[1].content["method"], "GET");
    assert_eq!(entries[1].content["payload"], payload);
    for entry in &entries[..2] {
        assert_eq!(
            entry.content["headers"]["host"],
            redirect_address.to_string()
        );
        assert!(
            entry.content["headers"]["content-type"]
                .as_str()
                .unwrap()
                .starts_with("multipart/form-data; boundary=")
        );
    }
    let entries = &entries[2..];
    assert_eq!(entries[0].content["method"], "OPTIONS");
    assert!(
        entries[0].content["uri"]
            .as_str()
            .unwrap()
            .ends_with("/final?destination=value")
    );
    assert_eq!(entries[0].content["payload"], json!([]));
    let entries = &entries[1..];
    assert_eq!(entries[0].content["method"], "GET");
    assert_eq!(entries[0].content["payload"], json!([]));
    assert!(entries[0].content["response_status"].is_null());
    assert!(
        entries[1].content["uri"]
            .as_str()
            .unwrap()
            .ends_with("/final?destination=value")
    );
    assert_eq!(entries[1].content["method"], "GET");
    assert_eq!(entries[1].content["headers"]["host"], address.to_string());
    assert_eq!(
        entries[1].content["payload"],
        json!({"flag":false,"count":2,"secret":"********"})
    );
    let entries = &entries[2..];
    assert_eq!(entries[0].content["payload"], json!([]));
    assert!(!entries[0].content["uri"].as_str().unwrap().contains('?'));
    let entries = &entries[1..];
    assert_eq!(entries[0].content["payload"], json!({"typed":true}));
    assert_eq!(entries[1].content["payload"], json!({"raw":true}));
    assert_eq!(entries[2].content["method"], "HEAD");
    assert_eq!(entries[2].content["payload"], json!([]));
    let entries = &entries[3..];
    assert!(
        entries[0].content["uri"]
            .as_str()
            .unwrap()
            .ends_with("?override=yes")
    );
    assert_eq!(
        entries[1].content["payload"],
        json!({"same":"query","query":"value"})
    );
    assert!(entries[2].content["response_status"].is_null());
    assert_eq!(entries[2].content["payload"], json!({"failed":true}));
    assert_eq!(entries[3].content["method"], "GET");
    assert_eq!(entries[3].content["payload"], json!([]));
    assert_eq!(entries[4].content["method"], "GET");
    assert_eq!(
        entries[4].content["payload"],
        json!({"same":"query","query":"value"})
    );
    let multipart: Vec<_> = entries
        .iter()
        .filter(|entry| {
            entry.content["headers"]["content-type"]
                .as_str()
                .is_some_and(|value| value.starts_with("multipart/form-data"))
        })
        .collect();
    assert_eq!(multipart.len(), 3);
    assert!(multipart[0].content["response_status"].is_null());
    assert_eq!(multipart[0].content["payload"], json!([]));
    assert_eq!(multipart[1].content["payload"], json!([]));
    assert!(!multipart[1].content["uri"].as_str().unwrap().contains('?'));
    assert_eq!(multipart[2].content["payload"], payload);
    assert_eq!(multipart[2].content["response_status"], 200);
    let entries = &entries[8..];
    assert!(entries.iter().all(|entry| {
        entry.content["response"] == json!({"token":"********","value":true})
            && entry.content["response_status"] == 200
            && entry.content["duration"]
                .as_u64()
                .is_some_and(|ms| ms >= 10)
    }));
    assert!(
        entries
            .iter()
            .all(|entry| entry.content["payload"]["secret"] == "********"
                && entry.content["payload"].get("query").is_none())
    );
    assert_eq!(entries[0].content["payload"]["same"], "form");
    assert_eq!(entries[0].content["payload"]["flag"], false);
    assert_eq!(entries[0].content["payload"]["count"], 2);
    assert_eq!(entries[0].content["payload"]["nested.password"], "********");
    assert_eq!(entries[1].content["payload"]["same"], "body");
    assert_eq!(
        entries[1].content["payload"]["nested"]["password"],
        "********"
    );
    assert_eq!(entries[1].content["payload"]["nested.password"], "********");
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let queries = repository
        .get(Some(EntryType::Query), EntryQueryOptions::default())
        .await
        .unwrap();
    assert!(
        queries
            .iter()
            .any(|entry| entry.content["sql"] == "SELECT 1 AS probe")
    );
}
#[tokio::test]
async fn console_list_uses_upstream_colors_only_in_decorated_output() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let batch = uuid::Uuid::now_v7();

    for (kind, content, expected) in [
        (
            EntryType::Request,
            json!({"method":"GET","uri":"/read","response_status":200}),
            "\x1b[90mGET\x1b[39m\t/read\t\x1b[32m200\x1b[39m",
        ),
        (
            EntryType::Request,
            json!({"method":"POST","uri":"/create","response_status":302}),
            "\x1b[34mPOST\x1b[39m\t/create\t\x1b[34m302\x1b[39m",
        ),
        (
            EntryType::Request,
            json!({"method":"DELETE","uri":"/destroy","response_status":404}),
            "\x1b[31mDELETE\x1b[39m\t/destroy\t\x1b[33m404\x1b[39m",
        ),
        (
            EntryType::ClientRequest,
            json!({"method":"HEAD","uri":"/check","response_status":503}),
            "HEAD\t/check\t\x1b[31m503\x1b[39m",
        ),
        (
            EntryType::Query,
            json!({"sql":"select 1","time":"5","slow":1,"connection":"test"}),
            "5ms\t\x1b[31mYes\x1b[39m\ttest",
        ),
        (
            EntryType::Job,
            json!({"name":"App::Build","queue":"default","status":"pending"}),
            "default\t\x1b[33mpending\x1b[39m",
        ),
        (
            EntryType::Cache,
            json!({"type":"missed","key":"<fg=blue>report</>\u{1b}[5m"}),
            "\x1b[31mMISS\x1b[39m\t<fg=blue>report</> [5m",
        ),
        (
            EntryType::Log,
            json!({"level":"info","message":"info"}),
            "\x1b[34minfo\x1b[39m\tinfo",
        ),
    ] {
        let entry = IncomingEntry::make(content.as_object().unwrap().clone())
            .unwrap()
            .r#type(kind)
            .batch_id(batch);
        let id = entry.uuid.to_string();
        let expected_content = serde_json::Value::Object(entry.content.clone());
        repository.store(&[entry]).await.unwrap();

        let ansi = app
            .artisan(["telescope:list", kind.as_str(), "--ansi"])
            .await;
        ansi.assert_successful();
        let row = ansi
            .stdout
            .lines()
            .find(|line| line.starts_with(&id[..8]))
            .unwrap();
        assert!(row.contains(expected), "{row:?}");
        assert!(!row.contains("\x1b[5m"), "{row:?}");

        let unstyled = app
            .artisan(["telescope:list", kind.as_str(), "--no-ansi"])
            .await;
        unstyled.assert_successful();
        assert!(!unstyled.stdout.contains('\x1b'));
        let json_result = app
            .artisan(["telescope:list", kind.as_str(), "--json", "--ansi"])
            .await;
        json_result.assert_successful();
        let entries: serde_json::Value = serde_json::from_str(&json_result.stdout).unwrap();
        assert_eq!(entries[0]["content"], expected_content);
    }
}

#[tokio::test]
async fn console_show_uses_upstream_colors_for_details_and_batch_fields() {
    let mut app = application();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let batch = uuid::Uuid::now_v7();
    let cases = [
        (
            EntryType::Request,
            json!({"method":"GET","uri":"/observed","response_status":200}),
        ),
        (
            EntryType::Query,
            json!({"sql":"select 1","time":"3","slow":true,"hash":"repeat"}),
        ),
        (
            EntryType::Query,
            json!({"sql":"select 1","time":"1","hash":"repeat"}),
        ),
        (EntryType::Cache, json!({"type":"hit","key":"report"})),
        (
            EntryType::Log,
            json!({"level":"debug","message":"diagnostic"}),
        ),
        (
            EntryType::Job,
            json!({"name":"App::Test","status":"failed"}),
        ),
        (
            EntryType::Exception,
            json!({"class":"App::Error","line":7,"line_preview":{"7":"<fg=green>literal</>\u{1b}[5m"}}),
        ),
        (
            EntryType::ClientRequest,
            json!({"method":"GET","uri":"https://example.test","response_status":503}),
        ),
    ];
    let entries = cases.map(|(kind, content)| {
        let mut entry = IncomingEntry::make(content.as_object().unwrap().clone())
            .unwrap()
            .r#type(kind)
            .batch_id(batch);
        entry.recorded_at = "2026-11-26T12:00:00Z".parse().unwrap();
        entry
    });
    repository.store(&entries).await.unwrap();
    for (entry, expected) in [
        (
            &entries[0],
            &[
                "  \x1b[90m(2026-11-26 12:00:00)\x1b[39m",
                "\x1b[31mSLOW\x1b[39m \x1b[33mDUP\x1b[39m",
                "\x1b[32mHIT\x1b[39m\treport",
                "\x1b[90mdebug\x1b[39m\tdiagnostic",
            ][..],
        ),
        (&entries[1], &["Duration\t3ms  \x1b[31mSLOW\x1b[39m"][..]),
        (&entries[5], &["Status\t\x1b[31mfailed\x1b[39m"][..]),
        (
            &entries[6],
            &[
                "Resolved\t\x1b[33mNo\x1b[39m",
                "\x1b[31m7 >\x1b[39m\t<fg=green>literal</> [5m",
            ][..],
        ),
        (&entries[7], &["Status\t\x1b[31m503\x1b[39m"][..]),
    ] {
        let id = entry.uuid.to_string();
        let ansi = app.artisan(["telescope:show", &id, "--ansi"]).await;
        ansi.assert_successful();
        for expected in expected {
            assert!(
                ansi.stdout.contains(expected),
                "{expected:?} in {:?}",
                ansi.stdout
            );
        }
        assert!(!ansi.stdout.contains("\x1b[5m"));

        let plain = app.artisan(["telescope:show", &id, "--no-ansi"]).await;
        plain.assert_successful();
        assert!(!plain.stdout.contains('\x1b'));
        let json = app
            .artisan(["telescope:show", &id, "--json", "--ansi"])
            .await;
        json.assert_successful();
        let value: serde_json::Value = serde_json::from_str(&json.stdout).unwrap();
        assert_eq!(
            value["entry"]["content"],
            serde_json::Value::Object(entry.content.clone())
        );
    }
}
