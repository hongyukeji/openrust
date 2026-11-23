#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use openrust::{contracts::foundation::Application as _, foundation::Application};
use openrust_activitylog::{ActivityRepository, ActivitylogServiceProvider};
use serde_json::json;

#[tokio::test]
async fn package_boot_is_lazy_defaults_are_isolated_and_application_configuration_wins() {
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .withProviders(vec![Box::new(ActivitylogServiceProvider)])
        .withConfig(|_, _| {
            Ok(std::collections::BTreeMap::from([(
                "activitylog".into(),
                json!({"enabled":false,"properties":["project"]}),
            )]))
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    assert_eq!(
        app.config().unwrap().get("activitylog.enabled"),
        Some(json!(false))
    );
    assert_eq!(
        app.config().unwrap().get("activitylog.clean_after_days"),
        Some(json!(365))
    );
    let logger = app.make::<ActivityRepository>().await.unwrap();
    let again = app.make::<ActivityRepository>().await.unwrap();
    assert!(std::sync::Arc::ptr_eq(&logger, &again));
    assert!(
        logger
            .prepare(openrust_activitylog::Activity::new("operation"))
            .unwrap()
            .is_none()
    );
    let mut other = Application::configure(root.path().to_owned())
        .withProviders(vec![Box::new(ActivitylogServiceProvider)])
        .create()
        .unwrap();
    other.boot().await.unwrap();
    assert_eq!(
        other.config().unwrap().get("activitylog.enabled"),
        Some(json!(true))
    );
    assert!(!std::sync::Arc::ptr_eq(
        &logger,
        &other.make::<ActivityRepository>().await.unwrap()
    ));
}

fn routes() -> std::result::Result<(), openrust::routing::RoutingError> {
    for path in ["/buffer/ok", "/buffer/failed", "/buffer/cancel"] {
        openrust::support::facades::Route::get(path, buffered_activity)?;
    }
    openrust::support::facades::Route::get("/activity", handler)?;
    openrust::support::facades::Route::get("/activity/disabled", disabled)?;
    Ok(())
}
fn handler(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<
    '_,
    std::result::Result<openrust::http::Response, openrust::http::HttpError>,
> {
    Box::pin(async move {
        let mut logger = openrust_activitylog::facades::Activitylog::logger()
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        logger
            .log("system.probe")
            .await
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        let repository = openrust_activitylog::facades::Activitylog::repository()
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        let records = repository
            .query()
            .get()
            .await
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        Ok(openrust::http::Response::new(
            http::StatusCode::OK,
            openrust::http::Body::from(records.len().to_string()),
        ))
    })
}
fn application() -> openrust::testing::TestApp {
    let app=openrust::testing::TestApp::create(|builder|builder
        .withConfig(|base,_|Ok(std::collections::BTreeMap::from([
            ("app".into(),json!({"env":"testing"})),
            ("database".into(),json!({"default":"testing","connections":{"testing":{"driver":"sqlite","database":base.join("activity.sqlite")}}})),
        ])))
        .withProviders(vec![Box::new(ActivitylogServiceProvider)])
        .withRouting(openrust::foundation::configuration::Routing::default().api(routes).commands(buffer_commands))
    ).unwrap();
    std::fs::File::create(app.root().join("activity.sqlite")).unwrap();
    app
}

#[tokio::test]
async fn http_and_artisan_share_migrations_and_keep_applications_isolated() {
    let mut one = application();
    let mut two = application();
    for app in [&mut one, &mut two] {
        app.artisan(["migrate", "--force", "--no-interaction"])
            .await
            .assertSuccessful();
        app.artisan(["help", "activitylog:clean"])
            .await
            .assertSuccessful();
    }
    one.get("/api/activity/disabled")
        .await
        .unwrap()
        .assertOk()
        .assertContent("disabled");
    one.get("/api/activity")
        .await
        .unwrap()
        .assertOk()
        .assertContent("1");
    one.get("/api/activity")
        .await
        .unwrap()
        .assertOk()
        .assertContent("2");
    two.get("/api/activity")
        .await
        .unwrap()
        .assertOk()
        .assertContent("1");
    one.artisan(["activitylog:clean", "--days=365"])
        .await
        .assertSuccessful();
    assert_ne!(
        one.artisan(["activitylog:clean", "--days=0"]).await.status,
        0
    );
    assert_ne!(
        one.artisan(["activitylog:clean", "--limit=1001"])
            .await
            .status,
        0
    );
    one.artisan([
        "vendor:publish",
        "--tag=activitylog-config",
        "--no-interaction",
    ])
    .await
    .assertSuccessful();
    let published = one.root().join("config/activitylog.rs");
    assert!(published.exists());
    std::fs::write(&published, "// customized").unwrap();
    one.artisan([
        "vendor:publish",
        "--tag=activitylog-config",
        "--no-interaction",
    ])
    .await
    .assertSuccessful();
    assert_eq!(
        std::fs::read_to_string(&published).unwrap(),
        "// customized"
    );
    one.artisan([
        "vendor:publish",
        "--tag=activitylog-config",
        "--force",
        "--no-interaction",
    ])
    .await
    .assertSuccessful();
    assert!(
        std::fs::read_to_string(&published)
            .unwrap()
            .contains("clean_after_days")
    );
    one.artisan(["migrate:status"]).await.assertSuccessful();
}

fn disabled(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<
    '_,
    std::result::Result<openrust::http::Response, openrust::http::HttpError>,
> {
    Box::pin(async {
        let error = || openrust::http::HttpError::InvalidRequest;
        let mut logger =
            openrust_activitylog::facades::Activitylog::logger().map_err(|_| error())?;
        logger.disableLogging();
        assert!(
            logger
                .log("suppressed")
                .await
                .map_err(|_| error())?
                .is_none()
        );
        let mut another = openrust_activitylog::activity().map_err(|_| error())?;
        assert!(
            another
                .log("also suppressed")
                .await
                .map_err(|_| error())?
                .is_none()
        );
        Ok(openrust::http::Response::new(
            http::StatusCode::OK,
            openrust::http::Body::from("disabled"),
        ))
    })
}

#[tokio::test]
async fn cleaner_selects_log_names_validates_days_and_reuses_production_confirmation() {
    use openrust_activitylog::Activity;
    let mut app = application();
    app.artisan(["migrate", "--force", "--no-interaction"])
        .await
        .assertSuccessful();
    let repository = app.app().make::<ActivityRepository>().await.unwrap();
    for name in ["accounts", "other"] {
        let mut activity = Activity::new("expired").useLog(name);
        activity.created_at = Some(0);
        repository.log(activity).await.unwrap();
    }
    let result = app
        .artisan(["activitylog:clean", "accounts", "--days=1"])
        .await;
    result.assertSuccessful();
    assert!(
        result
            .stdout
            .contains("Deleted 1 record(s) from the activity log.")
    );
    let remaining = repository.query().get().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].log_name.as_deref(), Some("other"));
    for days in ["0", "-1", "abc", "1.5"] {
        let arg = format!("--days={days}");
        let result = app.artisan(["activitylog:clean", arg.as_str()]).await;
        assert_eq!(result.status, 1);
        assert!(
            result
                .stdout
                .contains("The days option must be a positive integer.")
        );
    }
    app.app()
        .config()
        .unwrap()
        .set("app.env", json!("production"))
        .unwrap();
    assert_eq!(
        app.artisan(["activitylog:clean", "--no-interaction"])
            .await
            .status,
        1
    );
    assert_eq!(repository.query().get().await.unwrap().len(), 1);
    app.artisan(["activitylog:clean", "--force", "--no-interaction"])
        .await
        .assertSuccessful();
    assert!(repository.query().get().await.unwrap().is_empty());
}

#[derive(Clone)]
struct Actor(openrust::database::eloquent::ModelState);
impl openrust::database::eloquent::Model for Actor {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("actors");
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
impl openrust::contracts::auth::Authenticatable for Actor {
    fn getAuthIdentifierName(&self) -> &str {
        "id"
    }
    fn getAuthIdentifier(&self) -> openrust::auth::AuthIdentifier {
        use openrust::database::eloquent::Model;
        openrust::auth::AuthIdentifier::try_from(self.getKey().unwrap()).unwrap()
    }
    fn getAuthPasswordName(&self) -> &str {
        "password"
    }
    fn getAuthPassword(&self) -> Option<&str> {
        None
    }
    fn getRememberToken(&self) -> Option<&str> {
        None
    }
    fn setRememberToken(&mut self, _: Option<String>) {}
    fn getRememberTokenName(&self) -> &str {
        "remember_token"
    }
}
fn authenticated_activity(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::{
            contracts::auth::Factory as _,
            database::eloquent::Model,
            support::facades::{Auth, DB},
        };
        let auth = Auth::manager().unwrap();
        if !request.path().ends_with("guest") {
            let connection = DB::connection(None).unwrap();
            for (guard, id) in [("first", 1_i64), ("second", 2_i64)] {
                let mut actor = Actor::newInstanceOn(&connection, Default::default()).unwrap();
                actor.setAttribute("id", id);
                actor.setAttribute("name", format!("Actor {id}"));
                auth.guard(request, Some(guard))
                    .unwrap()
                    .set_user(Box::new(actor));
            }
        }
        let mut entry = openrust_activitylog::activity().unwrap();
        if request.path().ends_with("anonymous") {
            entry.byAnonymous();
        }
        if request.path().ends_with("lookup") {
            entry.byId(5_u64).await.unwrap();
        } else if request.path().ends_with("missing") {
            assert!(entry.byId(999_u64).await.is_err());
            return Ok(openrust::http::Response::from("missing rejected"));
        }
        entry.log("request :causer.name").await.unwrap();
        Ok(openrust::http::Response::from("logged"))
    })
}
fn authenticated_routes() -> Result<(), openrust::routing::RoutingError> {
    for path in ["/identity", "/guest", "/anonymous", "/lookup", "/missing"] {
        openrust::support::facades::Route::get(path, authenticated_activity)?;
    }
    Ok(())
}
#[tokio::test]
async fn default_auth_causer_uses_configured_guard_real_morph_and_request_isolation() {
    use openrust::database::eloquent::relations::{MorphMap, MorphMapEntry};
    let mut app = openrust::testing::TestApp::create(|builder| builder
        .withConfig(|base, _| Ok(std::collections::BTreeMap::from([
            ("app".into(), json!({"env":"testing"})),
            ("database".into(), json!({"default":"testing","connections":{"testing":{"driver":"sqlite","database":base.join("activity.sqlite")}}})),
            ("activitylog".into(), json!({"default_auth_driver":"second"})),
            ("auth".into(), json!({"defaults":{"guard":"first"},"guards":{"first":{"driver":"cached"},"second":{"driver":"cached","provider":"actors"}},"providers":{"actors":{"driver":"eloquent","model":"actors"}}})),
        ])))
        .withProviders(vec![Box::new(ActivitylogServiceProvider)])
        .withRouting(openrust::foundation::configuration::Routing::default().api(authenticated_routes))
    ).unwrap();
    std::fs::File::create(app.root().join("activity.sqlite")).unwrap();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let auth = app
        .app()
        .make::<openrust::auth::AuthManager>()
        .await
        .unwrap();
    auth.registerModel::<Actor>("actors");
    auth.viaRequest("cached", |_| {
        panic!("already resolved identity must not perform another lookup")
    });
    MorphMap::enforceMorphMap(app.app(), [MorphMapEntry::model::<Actor>("staff")], false).unwrap();
    let repository = app.app().make::<ActivityRepository>().await.unwrap();
    for path in [
        "/api/identity",
        "/api/anonymous",
        "/api/guest",
        "/api/identity",
    ] {
        app.get(path).await.unwrap().assertOk();
    }
    let rows = repository.query().get().await.unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[0].causer,
        Some(openrust_activitylog::Subject::new("staff", "2").unwrap())
    );
    assert!(rows[1].causer.is_none() && rows[2].causer.is_none());
    assert_eq!(rows[3].causer, rows[0].causer);
    assert_eq!(rows[0].description, "request Actor 2");
    assert!(auth.resolvedUser(Some("second")).is_none());
    let connection = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    connection
        .getSchemaBuilder()
        .create("actors", |table| {
            table.id();
            table.string("name", 255);
        })
        .await
        .unwrap();
    connection
        .table("actors")
        .insert(&[std::collections::BTreeMap::from([
            ("id".to_owned(), openrust::database::Value::from(5_i64)),
            (
                "name".to_owned(),
                openrust::database::Value::from("Lookup actor"),
            ),
        ])])
        .await
        .unwrap();
    app.get("/api/lookup").await.unwrap().assertOk();
    app.get("/api/missing").await.unwrap().assertOk();
    let rows = repository.query().get().await.unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].description, "request Lookup actor");
    assert_eq!(rows[0].causer.as_ref().unwrap().id, "5");
}

fn buffered_activity(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let repository = openrust_activitylog::facades::Activitylog::repository().unwrap();
        let before = repository.query().get().await.unwrap().len();
        let mut first = openrust_activitylog::activity().unwrap();
        let mut second = openrust_activitylog::activity().unwrap();
        assert!(
            first
                .log("first buffered")
                .await
                .unwrap()
                .unwrap()
                .id
                .is_none()
        );
        assert!(
            second
                .log("second buffered")
                .await
                .unwrap()
                .unwrap()
                .id
                .is_none()
        );
        assert_eq!(repository.query().get().await.unwrap().len(), before);
        if request.path().ends_with("cancel") {
            std::future::pending::<()>().await;
        }
        if request.path().ends_with("failed") {
            return Err(openrust::http::abort(
                http::StatusCode::UNPROCESSABLE_ENTITY,
                "invalid",
                Default::default(),
            ));
        }
        Ok(openrust::http::Response::from("buffered"))
    })
}

#[tokio::test]
async fn automatic_buffer_flushes_success_and_error_original_scope_but_not_cancelled_request() {
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.app()
        .config()
        .unwrap()
        .set("activitylog.buffer.enabled", json!(true))
        .unwrap();
    let repository = app.app().make::<ActivityRepository>().await.unwrap();
    assert!(repository.query().get().await.unwrap().is_empty());
    app.get("/api/buffer/ok").await.unwrap().assertOk();
    assert_eq!(repository.query().get().await.unwrap().len(), 2);
    let failed = app.get("/api/buffer/failed").await.unwrap();
    failed.assertStatus(http::StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(repository.query().get().await.unwrap().len(), 4);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(30),
            app.get("/api/buffer/cancel")
        )
        .await
        .is_err()
    );
    assert_eq!(repository.query().get().await.unwrap().len(), 4);
    app.get("/api/buffer/ok").await.unwrap().assertOk();
    assert_eq!(repository.query().get().await.unwrap().len(), 6);
}

#[derive(Clone)]
struct CustomActivity(openrust::database::eloquent::ModelState);
impl openrust::database::eloquent::Model for CustomActivity {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("custom_activity_log").guarded(&[]);
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
#[tokio::test]
async fn configured_model_and_actions_resolve_native_named_bindings() {
    use openrust::contracts::container::Container as _;
    use openrust_activitylog::{
        Activity,
        actions::{CleanActivityLogAction, LogActivityAction},
        models::ActivityModel,
    };
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    let manager = app
        .app()
        .make::<openrust::database::DatabaseManager>()
        .await
        .unwrap();
    let connection = manager.connection(None).unwrap();
    connection
        .getSchemaBuilder()
        .rename("activity_log", "custom_activity_log")
        .await
        .unwrap();
    use openrust::database::eloquent::{Model as _, concerns::ModelEvent};
    CustomActivity::registerModelEvent(app.app(), ModelEvent::Creating, CustomizeActivity).unwrap();
    let config = app.app().config().unwrap();
    config
        .set("activitylog.activity_model", json!("application.activity"))
        .unwrap();
    config
        .set("activitylog.actions.log_activity", json!("application.log"))
        .unwrap();
    config
        .set("activitylog.actions.clean_log", json!("application.clean"))
        .unwrap();
    app.app()
        .container()
        .instanceNamed(
            "application.activity",
            std::sync::Arc::new(ActivityModel::of::<CustomActivity>()),
        )
        .unwrap();
    let action = LogActivityAction::default();
    action.beforeLogging(|activity| {
        activity.description = Some("custom action".into());
        Ok(())
    });
    app.app()
        .container()
        .instanceNamed("application.log", std::sync::Arc::new(action))
        .unwrap();
    app.get("/api/activity")
        .await
        .unwrap()
        .assertOk()
        .assertContent("1");
    let repository = app.app().make::<ActivityRepository>().await.unwrap();
    assert_eq!(
        repository.query().get().await.unwrap()[0].description,
        "custom action from model"
    );
    let mut expired = Activity::new("expired");
    expired.created_at = Some(0);
    repository.log(expired).await.unwrap();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = calls.clone();
    let cleaner =
        CleanActivityLogAction::new(repository.clone()).using(move |repository, days, log| {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                CleanActivityLogAction::new(repository)
                    .execute(days, log.as_deref())
                    .await
            }
        });
    app.app()
        .container()
        .instanceNamed("application.clean", std::sync::Arc::new(cleaner))
        .unwrap();
    app.artisan(["activitylog:clean", "--days=1"])
        .await
        .assertSuccessful();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(repository.query().get().await.unwrap().len(), 1);
    assert!(
        !connection
            .getSchemaBuilder()
            .hasTable("activity_log")
            .await
            .unwrap()
    );
}

struct CustomizeActivity;
impl openrust::database::eloquent::concerns::ModelEventListener<CustomActivity>
    for CustomizeActivity
{
    fn handle<'a>(
        &'a self,
        _: openrust::database::eloquent::concerns::ModelEvent,
        model: &'a mut CustomActivity,
        _: &'a openrust::database::Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        use openrust::database::eloquent::Model as _;
        Box::pin(async move {
            let description = model
                .state()
                .getAttributes()
                .get("description")
                .and_then(|value| match value {
                    openrust::database::Value::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .unwrap()
                .to_owned();
            model.setAttribute("description", format!("{description} from model"));
            Ok(openrust::database::eloquent::concerns::ModelEventResponse::Continue)
        })
    }
}

struct BufferCommand(openrust::console::CommandDefinition, i32);
impl openrust::console::Command for BufferCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a openrust::console::Input,
        _: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            let repository = openrust_activitylog::facades::Activitylog::repository().unwrap();
            let before = repository.query().get().await.unwrap().len();
            let mut logger = openrust_activitylog::activity().unwrap();
            assert!(
                logger
                    .log("command buffered")
                    .await
                    .unwrap()
                    .unwrap()
                    .id
                    .is_none()
            );
            assert_eq!(repository.query().get().await.unwrap().len(), before);
            if self.1 < 0 {
                return Err(openrust::console::ConsoleError::new("probe error"));
            }
            Ok(self.1)
        })
    }
}
fn buffer_commands() -> Result<(), openrust::console::ConsoleError> {
    for (name, status) in [
        ("buffer:ok", 0),
        ("buffer:nonzero", 3),
        ("buffer:error", -1),
    ] {
        openrust::support::facades::Artisan::add(Box::new(BufferCommand(
            openrust::console::CommandDefinition::new(clap::Command::new(name)),
            status,
        )))?;
    }
    Ok(())
}
#[tokio::test]
async fn automatic_buffer_flushes_success_nonzero_and_failed_artisan_commands() {
    let mut app = application();
    app.artisan(["migrate", "--force"]).await.assertSuccessful();
    app.app()
        .config()
        .unwrap()
        .set("activitylog.buffer.enabled", json!(true))
        .unwrap();
    let repository = app.app().make::<ActivityRepository>().await.unwrap();
    for (index, command) in ["buffer:ok", "buffer:nonzero", "buffer:error"]
        .into_iter()
        .enumerate()
    {
        let result = app.artisan([command]).await;
        assert_eq!(result.status == 0, index == 0);
        assert_eq!(repository.query().get().await.unwrap().len(), index + 1);
    }
}
