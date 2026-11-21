use openrust::support::facades::Facade;
use openrust_telescope::{
    EntryType, IncomingEntry, Telescope, TelescopeServiceProvider,
    storage::{DatabaseEntriesRepository, EntryQueryOptions},
};
use serde_json::json;

fn routes() -> Result<(), openrust::routing::RoutingError> {
    for path in ["/probe", "/ignore/probe"] {
        openrust::support::facades::Route::get(path, handler)?;
    }
    openrust::support::facades::Route::get("/retry", retry)?;
    openrust::support::facades::Route::get("/logs", log_handler)?;
    openrust::support::facades::Route::get("/jobs", job_handler)?;
    openrust::support::facades::Route::group(openrust::routing::RouteGroup::new(), |router| {
        router.r#match(&[openrust::http::Method::POST], "/input", input_handler)?;
        router.r#match(&[openrust::http::Method::POST], "/unread", handler)?;
        Ok(())
    })?;
    openrust::support::facades::Route::get("/mail", mail_handler)?;
    openrust::support::facades::Route::get("/native", native_handler)?;
    openrust::support::facades::Route::get("/async", async_handler)?;
    openrust::support::facades::Route::get("/redis", redis_handler)?;
    openrust::support::facades::Route::get("/client", client_handler)?;
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.get("/api/redis").await.unwrap().assertOk();
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
            .getSchemaBuilder()
            .create("cache", |table| {
                table.string("key", 255).primary();
                table.mediumText("value");
                table.integer("expiration");
            })
            .await
            .unwrap();
        connection
            .getSchemaBuilder()
            .create("cache_locks", |table| {
                table.string("key", 255).primary();
                table.string("owner", 255);
                table.integer("expiration");
            })
            .await
            .unwrap();
    }
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.get("/api/probe").await.unwrap().assertOk();
    app.artisan(["telescope:pause"]).await.assertSuccessful();
    app.get("/api/probe").await.unwrap().assertOk();
    app.artisan(["telescope:resume"]).await.assertSuccessful();
    app.get("/api/probe").await.unwrap().assertOk();
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
        let telescope = openrust_telescope::facades::Telescope::getFacadeRoot().unwrap();
        telescope.recordEvent(
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
fn application() -> openrust::testing::TestApp {
    let app = openrust::testing::TestApp::create(|builder| builder
        .withConfig(|base, _| Ok(std::collections::BTreeMap::from([
            ("app".into(), json!({"env":"testing","url":"http://localhost","debug":true,"key":"01234567890123456789012345678901","cipher":"AES-256-CBC"})),
            ("database".into(), json!({"default":"testing","connections":{"testing":database_config(base)},"redis":{"default":{"url":std::env::var("TELESCOPE_TEST_REDIS_URL").ok()},"options":{"prefix":"telescope-test:"}}})),
            ("cache".into(), json!({"default":std::env::var("TELESCOPE_TEST_CACHE_DRIVER").unwrap_or("file".into()),"stores":{"file":{"driver":"file","path":base.join("cache/data"),"lock_path":base.join("cache/data")},"database":{"driver":"database","connection":"testing","table":"cache","lock_table":"cache_locks"},"redis":{"driver":"redis","connection":"default","lock_connection":"default"}}})),
            ("session".into(),json!({"driver":"array"})),
            ("auth".into(),json!({"defaults":{"guard":"probe"},"guards":{"probe":{"driver":"cached"}}})),
            ("queue".into(),json!({"default":"sync","connections":{"sync":{"driver":"sync"},"async":{"driver":"database","connection":"testing","table":"jobs","queue":"default","retry_after":30}},"batching":{"database":"testing","table":"job_batches"}})),
            ("mail".into(),json!({"default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"sender@example.org","name":"Sender"}})),
            ("telescope".into(), json!({"ignore_paths":["api/ignore/*"]})),
        ])))
        .withProviders(vec![Box::new(TelescopeServiceProvider)])
        .withMiddleware(|middleware| {
            middleware.skipTrimStringsWhen(|request|request.path().trim_start_matches('/')=="api/unread");
            middleware.convertEmptyStringsToNull(|request|request.path().trim_start_matches('/')=="api/unread");
        })
        .withRouting(openrust::foundation::configuration::Routing::default().api(routes).commands(commands))
    ).unwrap();
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
        Gate::defineGuest("observed", |_, arguments| {
            Box::pin(async move {
                assert_eq!(arguments[0].downcast_ref::<String>().unwrap(), "actual");
                Ok(openrust::auth::access::Response::allow())
            })
        })
        .unwrap();
        assert!(
            Gate::forUser(None)
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
        let connection = openrust::database::DB::connection(None).unwrap();
        connection
            .getSchemaBuilder()
            .create("observed_models", |table| {
                table.id();
                table.string("title", None);
            })
            .await
            .unwrap();
        let mut model = ObservedModel::queryOn(&connection)
            .make([("title".into(), "first".into())].into())
            .unwrap();
        model.save().await.unwrap();
        let id = model.getKey().unwrap().clone();
        model.setAttribute("title", "updated");
        model.save().await.unwrap();
        ObservedModel::queryOn(&connection)
            .findOrFail(id.clone())
            .await
            .unwrap();
        ObservedModel::queryOn(&connection)
            .findOrFail(id)
            .await
            .unwrap();
        model.delete().await.unwrap();
        openrust::support::dump(&json!({"real":"dump"}))
            .await
            .unwrap();
        Ok(openrust::http::Response::from(
            openrust::support::facades::View::getFacadeRoot()
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.auth(|_| Box::pin(async { Ok(true) }));
    api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/dumps",
        json!({}),
    )
    .await
    .assertOk();
    app.get("/api/native")
        .await
        .unwrap()
        .assertOk()
        .assertContent("observed view");
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hideRequestParameters(&["secret", "nested.password"]);
    api_call(&mut app,"POST","/api/input?query=value&same=query",json!({"secret":"private","same":"body","nested":{"password":"hidden","public":"visible"},"empty":false})).await.assertOk();
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
    response.assertOk();
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.get("/api/mail").await.unwrap().assertOk();
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
        preview.assertOk();
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
        download.assertOk();
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
    deep.assertOk();
    assert!(String::from_utf8_lossy(deep.body()).contains("base=\"/telescope\""));
}
#[tokio::test]
async fn provider_boot_is_lazy_and_native_lifecycles_flush_isolated_collectors() {
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
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
            .assertOk()
            .assertContent("ok");
    }
    let events = repository
        .get(Some(EntryType::Event), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_ne!(events[0].batchId, events[1].batchId);
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
    app.get("/api/ignore/probe").await.unwrap().assertOk();
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.get("/api/probe").await.unwrap().assertOk();
    let result = app.artisan(["telescope:list", "event", "--json"]).await;
    result.assertSuccessful();
    let entries: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["content"]["name"], "probe");
    let id = entries[0]["id"].as_str().unwrap();
    let result = app
        .artisan(["telescope:show", id, "--json", "--type=cache,request"])
        .await;
    result.assertSuccessful();
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
    app.artisan(["telescope:pause"]).await.assertSuccessful();
    app.get("/api/probe").await.unwrap().assertOk();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    assert_eq!(
        repository
            .get(Some(EntryType::Event), EntryQueryOptions::default())
            .await
            .unwrap()
            .len(),
        1
    );
    app.artisan(["telescope:resume"]).await.assertSuccessful();
    app.get("/api/probe").await.unwrap().assertOk();
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
        .assertSuccessful();
    app.artisan(["telescope:clear"]).await.assertSuccessful();
    assert!(
        repository
            .get(Some(EntryType::Event), EntryQueryOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    app.artisan(["telescope:publish"]).await.assertSuccessful();
    let config = app.root().join("config/telescope.rs");
    assert!(config.exists());
    std::fs::write(&config, "// custom").unwrap();
    app.artisan(["telescope:publish"]).await.assertSuccessful();
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "// custom");
    app.artisan(["telescope:publish", "--force"])
        .await
        .assertSuccessful();
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
        let telescope = openrust_telescope::facades::Telescope::getFacadeRoot().unwrap();
        let entry =
            IncomingEntry::make(json!({"name":"pending"}).as_object().unwrap().clone()).unwrap();
        let id = entry.uuid;
        telescope.recordEvent(entry);
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.get("/api/retry").await.unwrap().assertOk();
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
        openrust::log::Log::logException(
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.get("/api/logs").await.unwrap().assertOk();
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
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
        .assertStatus(openrust::http::StatusCode::FORBIDDEN);
    telescope.auth(|_| Box::pin(async { Ok(true) }));
    app.get("/telescope").await.unwrap().assertOk();
    app.get("/api/probe").await.unwrap().assertOk();
    let listed = api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/events",
        json!({"take":1}),
    )
    .await;
    listed.assertOk();
    let listed: serde_json::Value = listed.json().unwrap();
    assert_eq!(listed["status"], "enabled");
    assert_eq!(listed["entries"].as_array().unwrap().len(), 1);
    let id = listed["entries"][0]["id"].as_str().unwrap();
    let shown = app
        .get(&format!("/telescope/telescope-api/events/{id}"))
        .await
        .unwrap();
    shown.assertOk();
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
    .assertOk();
    let tags = app
        .get("/telescope/telescope-api/monitored-tags")
        .await
        .unwrap();
    tags.assertOk();
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
    .assertOk();
    api_call(
        &mut app,
        "POST",
        "/telescope/telescope-api/toggle-recording",
        json!({}),
    )
    .await
    .assertOk();
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
    .assertOk();
    api_call(
        &mut app,
        "DELETE",
        "/telescope/telescope-api/entries",
        json!({}),
    )
    .await
    .assertOk();
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
                    source: Box::new(std::io::Error::other("queue probe failed")),
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
        .onConnection("async")
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let connection = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    connection
        .getSchemaBuilder()
        .create("jobs", |table| {
            table.id();
            table.string("queue", 255).index();
            table.longText("payload");
            table.unsignedInteger("attempts");
            table.unsignedBigInteger("reserved_at").nullable();
            table.unsignedBigInteger("available_at");
            table.unsignedBigInteger("created_at");
        })
        .await
        .unwrap();
    connection
        .getSchemaBuilder()
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
    app.get("/api/async").await.unwrap().assertOk();
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
        .assertSuccessful();
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.app()
        .make::<openrust::queue::QueueManager>()
        .await
        .unwrap()
        .register("telescope.probe", std::sync::Arc::new(QueueProbe))
        .unwrap();
    app.get("/api/jobs").await.unwrap().assertOk();
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
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
    telescope.filter(|entry| entry.hasMonitoredTag());
    app.get("/api/probe").await.unwrap().assertOk();
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
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
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
    .batchId(uuid::Uuid::now_v7());
    let id = entry.uuid.to_string();
    repository.store(&[entry]).await.unwrap();
    let listed = app.artisan(["telescope:list", "log"]).await;
    listed.assertSuccessful();
    assert!(listed.stdout.contains("Level\tMessage"));
    assert!(!listed.stdout.contains("TAIL"));
    let short = app.artisan(["telescope:show", &id]).await;
    short.assertSuccessful();
    assert!(!short.stdout.contains("TAIL"));
    let full = app.artisan(["telescope:show", &id, "--full"]).await;
    full.assertSuccessful();
    assert!(full.stdout.contains(&message));
    let json = app.artisan(["telescope:show", &id, "--json"]).await;
    json.assertSuccessful();
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
        openrust::auth::AuthIdentifier::try_from(self.getKey().unwrap()).unwrap()
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
            support::facades::{Auth, DB},
        };
        if request.path().ends_with("identity") {
            let connection = DB::connection(None).unwrap();
            let mut actor = ObservedModel::newInstanceOn(&connection, Default::default()).unwrap();
            actor.setAttribute("id", 42_i64);
            actor.setAttribute("name", "Observed actor");
            actor.setAttribute("email", "actor@example.org");
            actor.setAttribute("password", "never-exported");
            Auth::guard(request, None)
                .unwrap()
                .set_user(Box::new(actor));
        }
        openrust_telescope::facades::Telescope::getFacadeRoot()
            .unwrap()
            .recordEvent(
                IncomingEntry::make(json!({"name":"identity"}).as_object().unwrap().clone())
                    .unwrap(),
            );
        Ok(openrust::http::Response::from("observed"))
    })
}
#[tokio::test]
async fn authenticated_snapshots_use_resolved_identity_without_lookup_or_cross_request_leakage() {
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let auth = app
        .app()
        .make::<openrust::auth::AuthManager>()
        .await
        .unwrap();
    auth.registerModel::<ObservedModel>("observed");
    auth.viaRequest("cached", |_| {
        panic!("recording must never authenticate or look up a user")
    });
    app.get("/api/identity").await.unwrap().assertOk();
    app.get("/api/guest").await.unwrap().assertOk();
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
        use openrust::{http::client::Http, support::facades::DB};
        DB::connection(None)
            .unwrap()
            .select("SELECT 1 AS probe", &[])
            .await
            .unwrap();
        let url = request.headers()["x-origin-url"].to_str().unwrap();
        let data = json!({"same":"body","secret":"private","nested":{"password":"hidden"}});
        assert_eq!(Http::post(url, &data).await.unwrap().status().as_u16(), 200);
        assert_eq!(
            Http::pendingRequest()
                .unwrap()
                .asForm()
                .post(url, &json!({"same":"form","secret":"private"}))
                .await
                .unwrap()
                .status()
                .as_u16(),
            200
        );
        Ok(openrust::http::Response::from("observed"))
    })
}
#[tokio::test]
async fn actual_http_client_preserves_decoded_json_form_query_redaction_and_native_sql_observation()
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
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
                        break;
                    }
                }
            }
            let body = r#"{"token":"secret","value":true}"#;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).as_bytes()).await.unwrap();
        }
    });
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let telescope = app.app().make::<Telescope>().await.unwrap();
    telescope.hideRequestParameters(&["secret", "nested.password"]);
    telescope.hideResponseParameters(&["token"]);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.call(
            http::Request::builder()
                .uri("/api/client")
                .header(
                    "x-origin-url",
                    format!("http://{address}/outgoing?same=query&query=value"),
                )
                .body(openrust::http::Body::empty())
                .unwrap(),
        ),
    )
    .await
    .unwrap()
    .unwrap()
    .assertOk();
    origin.await.unwrap();
    let repository = app.app().make::<DatabaseEntriesRepository>().await.unwrap();
    let entries = repository
        .get(Some(EntryType::ClientRequest), EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|entry| entry.content["response"]
        == json!({"token":"********","value":true})
        && entry.content["response_status"] == 200
        && entry.content["duration"].as_f64().unwrap() > 0.0));
    assert!(
        entries
            .iter()
            .all(|entry| entry.content["payload"]["secret"] == "********"
                && entry.content["payload"]["query"] == "value")
    );
    assert_eq!(entries[0].content["payload"]["same"], "form");
    assert_eq!(entries[1].content["payload"]["same"], "body");
    assert_eq!(
        entries[1].content["payload"]["nested"]["password"],
        "********"
    );
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
