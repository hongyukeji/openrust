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
    Ok(())
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
                middleware.skip_trim_strings_when(|request| {
                    request.path().trim_start_matches('/') == "api/unread"
                });
                middleware.convert_empty_strings_to_null(|request| {
                    request.path().trim_start_matches('/') == "api/unread"
                });
            })
            .with_routing(
                openrust::foundation::configuration::Routing::default()
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
    let entry = IncomingEntry::make(
        json!({"level":"error","message":message})
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
    assert!(!short.stdout.contains("TAIL"));
    let full = app.artisan(["telescope:show", &id, "--full"]).await;
    full.assert_successful();
    assert!(full.stdout.contains(&message));
    let json = app.artisan(["telescope:show", &id, "--json"]).await;
    json.assert_successful();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(json.stdout.trim()).unwrap()["entry"]["content"]
            ["message"],
        message
    );
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
                        if matches!(index, 13 | 17 | 19) {
                            assert!(headers.starts_with("get /final?destination=value "));
                            assert_eq!(size, 0);
                            assert!(!headers.contains("transfer-encoding:"));
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
