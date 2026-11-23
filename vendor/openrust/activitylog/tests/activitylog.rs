use openrust::{
    config::Repository,
    contracts::foundation::Application as _,
    database::{
        Connection, DatabaseManager, Value,
        migrations::{MigrationEntry, MigrationRegistry, Migrator, RunOptions},
    },
};
use openrust_activitylog::{
    Activity, ActivityRepository, ActivitylogServiceProvider, Redaction, Subject,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

static DATABASE_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fixture {
    app: openrust::foundation::Application,
    _guard: tokio::sync::MutexGuard<'static, ()>,
    connection: Connection,
    migrator: Migrator,
    _root: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let guard = DATABASE_TEST.lock().await;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("test.sqlite");
        std::fs::File::create(&path).unwrap();
        let mut app = openrust::foundation::Application::configure(root.path().into())
            .with_config(fixtureConfig)
            .with_providers(vec![Box::new(ActivitylogServiceProvider)])
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let manager = app.make::<DatabaseManager>().await.unwrap();
        let connection = manager.connection(None).unwrap();
        let entries: &'static [MigrationEntry] = ActivitylogServiceProvider::migrations();
        let migrator = Migrator::new(
            manager,
            Arc::new(MigrationRegistry(entries)),
            "activitylog_test_migrations".into(),
            root.path().into(),
        );
        migrator.install(None).await.unwrap();
        assert_eq!(
            migrator
                .run(None, &[], RunOptions::default())
                .await
                .unwrap()
                .len(),
            1
        );
        // These models use string keys; customize the package's numeric morph columns
        // exactly as applications with non-numeric primary keys must do.
        connection
            .getSchemaBuilder()
            .table("activity_log", |t| {
                t.string("subject_id", 255).nullable().change();
                t.string("causer_id", 255).nullable().change();
            })
            .await
            .unwrap();
        connection
            .getSchemaBuilder()
            .create("business", |t| {
                t.string("id", 36).primary();
                t.string("value", 255);
                t.bigInteger("owner_id").nullable();
                t.text("data").nullable();
                t.string("flag", 10).nullable();
                t.timestamp("removed_on", None).nullable();
            })
            .await
            .unwrap();
        Self {
            app,
            _guard: guard,
            connection,
            migrator,
            _root: root,
        }
    }
    async fn close(self) {
        self.connection
            .getSchemaBuilder()
            .dropIfExists("business")
            .await
            .unwrap();
        assert_eq!(
            self.migrator.reset(None, &[], false).await.unwrap().len(),
            1
        );
        assert!(
            !self
                .connection
                .getSchemaBuilder()
                .hasTable("activity_log")
                .await
                .unwrap()
        );
        self.connection
            .getSchemaBuilder()
            .dropIfExists("activitylog_test_migrations")
            .await
            .unwrap();
        self.connection.disconnect();
    }
}
fn logger(connection: Connection) -> ActivityRepository {
    ActivityRepository::new(connection).withRedaction(
        Redaction::new()
            .logProperties(&["context", "password", "note"])
            .logOnly(&["name", "password", "nested"]),
    )
}
fn activity(id: &str) -> Activity {
    Activity::new("account.updated").event("updated").useLog("accounts")
        .performedOn(Subject::new("account",id).unwrap()).causedBy(Subject::new("user","42").unwrap())
        .withRequestId("request-123")
        .withProperties(json!({"context":{"api_key":"secret-api","nested":[{"Authorization":"Bearer secret-token"}]},"password":"secret-pass","ignored":"secret-ignore"}))
        .withChanges(json!({"name":"old","password":"old-pass"}),json!({"name":"new","password":"new-pass","email":"ignored@example.org"}))
}

#[tokio::test]
async fn persistence_atomicity_identity_concurrency_pagination_and_retention() {
    let f = Fixture::new().await;
    let logger = Arc::new(logger(f.connection.clone()));
    let first = logger.log(activity("AccountA")).await.unwrap().unwrap();
    assert_eq!(first.properties["password"], "[REDACTED]");
    assert_eq!(
        first.properties["context"]["nested"][0]["Authorization"],
        "[REDACTED]"
    );
    assert!(first.properties.get("ignored").is_none());
    assert_eq!(
        first.attribute_changes["attributes"]["password"],
        "[REDACTED]"
    );
    assert!(first.attribute_changes["attributes"].get("email").is_none());
    assert_eq!(
        logger
            .query()
            .forSubject(&Subject::new("account", "accounta").unwrap())
            .get()
            .await
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        logger
            .query()
            .forSubject(&Subject::new("account", "AccountA").unwrap())
            .inLog("accounts")
            .forEvent("updated")
            .causedBy(&Subject::new("user", "42").unwrap())
            .for_request("request-123")
            .get()
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        logger
            .query()
            .inLog("accounts' OR 1=1 --")
            .get()
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        logger
            .query()
            .inLogs(["missing", "accounts"])
            .get()
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        logger
            .query()
            .inLogs(Vec::<String>::new())
            .get()
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        logger
            .query()
            .inLogs(["missing", "accounts"])
            .inLog("other")
            .get()
            .await
            .unwrap()
            .is_empty()
    );
    let logger2 = logger.clone();
    assert!(
        f.connection
            .transaction(
                move |tx| {
                    let logger = logger2.clone();
                    Box::pin(async move {
                        tx.table("business")
                            .insert(&[[
                                ("id".into(), "rollback".into()),
                                ("value".into(), "rollback".into()),
                            ]
                            .into_iter()
                            .collect()])
                            .await?;
                        logger.logIn(tx, activity("rollback")).await?;
                        Err::<(), _>(openrust::database::DatabaseError::InvalidConfiguration(
                            "forced rollback",
                        ))
                    })
                },
                1
            )
            .await
            .is_err()
    );
    assert_eq!(f.connection.table("business").count("*").await.unwrap(), 0);
    assert_eq!(logger.query().get().await.unwrap().len(), 1);
    let signal = Arc::new(tokio::sync::Notify::new());
    let ready = signal.clone();
    let connection = f.connection.clone();
    let cancelled = logger.clone();
    let task = tokio::spawn(async move {
        connection
            .transaction(
                move |tx| {
                    let logger = cancelled.clone();
                    let ready = ready.clone();
                    Box::pin(async move {
                        logger.logIn(tx, activity("cancelled")).await?;
                        ready.notify_one();
                        std::future::pending::<()>().await;
                        Ok(())
                    })
                },
                1,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), signal.notified())
        .await
        .unwrap();
    task.abort();
    let _ = task.await;
    assert!(
        logger
            .query()
            .forSubject(&Subject::new("account", "cancelled").unwrap())
            .get()
            .await
            .unwrap()
            .is_empty()
    );
    let mut tasks = Vec::new();
    for n in 0..8 {
        let logger = logger.clone();
        tasks.push(tokio::spawn(async move {
            logger
                .log(activity(&format!("parallel-{n}")))
                .await
                .unwrap()
                .unwrap()
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(logger.query().get().await.unwrap().len(), 9);
    let page = logger.query().limit(4).get().await.unwrap();
    let next = logger
        .query()
        .before(page.last().unwrap().id.unwrap())
        .limit(4)
        .get()
        .await
        .unwrap();
    assert_eq!(next.len(), 4);
    assert!(next.iter().all(|r| page.iter().all(|p| p.id != r.id)));
    assert!(logger.query().limit(1001).get().await.is_err());
    assert!(logger.query().before(0).get().await.is_err());
    f.connection
        .table("activity_log")
        .r#where("id", "=", first.id.unwrap())
        .update(BTreeMap::from([(
            "created_at".into(),
            Value::from("1970-01-01 00:00:01"),
        )]))
        .await
        .unwrap();
    assert_eq!(logger.cleanBefore(2000, 1).await.unwrap(), 1);
    assert_eq!(logger.cleanBefore(2000, 1).await.unwrap(), 0);
    assert!(logger.cleanBefore(i64::MAX, 10).await.is_err());
    assert!(logger.cleanBefore(2000, 0).await.is_err());
    assert_eq!(logger.query().get().await.unwrap().len(), 8);
    assert!(
        f.migrator
            .run(None, &[], RunOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    f.close().await;
}

#[tokio::test]
async fn explicit_redaction_limits_disabled_and_debug_redaction() {
    let path = tempfile::tempdir().unwrap();
    let config = Arc::new(Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"testing","connections":{"testing":{"driver":"sqlite","database":path.path().join("nonexistent")}}}),
    )])));
    let connection = DatabaseManager::new(config, path.path())
        .connection(None)
        .unwrap();
    let logger = ActivityRepository::new(connection.clone());
    let prepared = logger.prepare(activity("42")).unwrap().unwrap();
    assert_eq!(prepared.record().properties, activity("42").properties);
    assert_eq!(prepared.record().attribute_changes, activity("42").changes);
    let secret =
        Activity::new("sensitive description").withProperties(json!({"secret":"top-secret"}));
    assert!(!format!("{secret:?}").contains("top-secret"));
    let mut subject = Subject::new("user", "42").unwrap();
    subject.id.clear();
    assert!(
        logger
            .prepare(Activity::new("operation").performedOn(subject))
            .is_err()
    );
    assert!(logger.prepare(Activity::new("\nvalid text")).is_ok());
    assert!(
        logger
            .prepare(Activity::new("operation").withProperties(json!([1, 2])))
            .is_err()
    );
    let logger = logger.withRedaction(Redaction::new().logProperties(&["note"]));
    assert!(
        logger
            .prepare(Activity::new("operation").withProperties(json!({"note":"x".repeat(17000)})))
            .is_err()
    );
    let mut nested = json!({"v":0});
    for _ in 0..18 {
        nested = json!({"n":nested});
    }
    assert!(
        logger
            .prepare(Activity::new("operation").withProperties(json!({"note":nested})))
            .is_err()
    );
    assert!(
        ActivityRepository::new(connection)
            .enabled(false)
            .log(activity("42"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn fluent_entries_reset_causers_anonymous_status_and_explicit_timestamp() {
    use openrust_activitylog::{ActivityLogStatus, ActivityLogger, CauserResolver};
    let f = Fixture::new().await;
    let actor = Subject::new("user", "7").unwrap();
    let status = ActivityLogStatus::default();
    let mut logger = ActivityLogger::new(f.connection.clone());
    logger
        .setLogStatus(status.clone())
        .setCauserResolver(CauserResolver::new(Some(actor.clone())));
    let first = logger
        .on(Subject::new("account", "24").unwrap())
        .setEvent("created")
        .inLog("business")
        .withProperty("password", "explicit-value")
        .withChanges(json!({"attributes":{"name":"new"}}))
        .createdAt(jiff::Timestamp::from_millisecond(1234).unwrap())
        .log("created account")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.causer, Some(actor.clone()));
    // Native timestamps use whole seconds by default.
    assert_eq!(first.created_at, 1000);
    assert_eq!(first.properties["password"], "explicit-value");
    assert_eq!(
        first.attribute_changes,
        json!({"attributes":{"name":"new"}})
    );
    let anonymous = logger
        .byAnonymous()
        .log("anonymous")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(anonymous.causer, None);
    assert_eq!(anonymous.subject, None);
    assert_eq!(anonymous.log_name.as_deref(), Some("default"));
    assert_eq!(anonymous.properties, json!({}));
    assert!(anonymous.created_at > 1234);
    let again = logger
        .by(None)
        .unwrap()
        .log("default actor")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again.causer, Some(actor));
    let mut entry = logger.activity();
    status
        .withoutLogging(|| async {
            assert!(entry.log("suppressed").await.unwrap().is_none());
            status
                .withoutLogging(|| async {
                    assert!(status.disabled());
                })
                .await;
            assert!(status.disabled());
        })
        .await;
    assert!(!status.disabled());
    assert!(entry.log("after suppression").await.unwrap().is_some());
    assert_eq!(
        f.connection.table("activity_log").count("*").await.unwrap(),
        4
    );
    f.close().await;
}

#[allow(non_snake_case)]
fn fixtureConfig(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> std::result::Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    let path = root.join("test.sqlite");
    let driver = std::env::var("ACTIVITYLOG_TEST_DRIVER").unwrap_or("sqlite".into());
    let db = match driver.as_str() {
        "sqlite" => {
            json!({"driver":"sqlite","database":path,"busy_timeout":10000,"max_connections":4})
        }
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap(),"max_connections":4})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            assert!(std::env::var("OPENRUST_ACCEPTANCE_MYSQL_CONTAINER").is_ok());
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),"max_connections":4})
        }
        _ => panic!("unsupported isolated driver"),
    };
    Ok(BTreeMap::from([(
        "database".into(),
        json!({"default":"testing","connections":{"testing":db}}),
    )]))
}

#[derive(Clone)]
struct Business(
    openrust::database::eloquent::ModelState,
    openrust_activitylog::models::LoggingState,
);
impl openrust::database::eloquent::Model for Business {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static METADATA: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("business")
                .incrementing(false)
                .fillable(&["id", "value"])
                .timestamps(None, None);
        &METADATA
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state, Default::default())
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
}
impl openrust_activitylog::models::concerns::LogsActivity for Business {
    fn activitylogState(&self) -> &openrust_activitylog::models::LoggingState {
        &self.1
    }

    fn getActivitylogOptions(&self) -> openrust_activitylog::LogOptions {
        openrust_activitylog::LogOptions::defaults()
            .logFillable()
            .logExcept(&["id"])
            .logOnlyDirty()
            .dontLogEmptyChanges()
            .useLogName("business")
            .setDescriptionForEvent(|event| format!("business {event}: :subject.value"))
    }
}

#[tokio::test]
async fn eloquent_events_select_changes_preserve_transaction_rollback_and_scope_causer() {
    use openrust::database::eloquent::Model;
    use openrust_activitylog::{ActivityLogger, CauserResolver, models::concerns::LogsActivity};
    let f = Fixture::new().await;
    let repository = f.app.make::<ActivityRepository>().await.unwrap();
    Business::bootLogsActivity(&f.app, repository.clone()).unwrap();
    let mut logger = ActivityLogger::fromRepository(repository.clone());
    logger.setCauserResolver(CauserResolver::new(Some(
        Subject::new("user", "actor").unwrap(),
    )));
    logger
        .scope(async {
            let mut model = Business::newInstanceOn(
                &f.connection,
                BTreeMap::from([
                    ("id".into(), "one".into()),
                    ("value".into(), "before".into()),
                ]),
            )
            .unwrap();
            model.save().await.unwrap();
            let created = repository.query().get().await.unwrap();
            assert_eq!(created.len(), 1);
            assert_eq!(created[0].event.as_deref(), Some("created"));
            assert_eq!(
                created[0].attribute_changes,
                json!({"attributes":{"value":"before"}})
            );
            assert_eq!(created[0].description, "business created: before");
            assert_eq!(created[0].causer.as_ref().unwrap().id, "actor");
            model
                .update(BTreeMap::from([("value".into(), "after".into())]))
                .await
                .unwrap();
            let updated = repository.query().get().await.unwrap();
            assert_eq!(updated.len(), 2);
            assert_eq!(
                updated[0].attribute_changes,
                json!({"old":{"value":"before"},"attributes":{"value":"after"}})
            );
            model.save().await.unwrap();
            assert_eq!(repository.query().get().await.unwrap().len(), 2);
            let connection = f.connection.clone();
            let rollback = connection
                .transaction(
                    move |tx| {
                        let mut model = model.clone();
                        Box::pin(async move {
                            model
                                .updateInTransaction(
                                    BTreeMap::from([("value".into(), "rolled back".into())]),
                                    tx,
                                )
                                .await?;
                            Err::<(), _>(openrust::database::DatabaseError::InvalidConfiguration(
                                "force rollback",
                            ))
                        })
                    },
                    1,
                )
                .await;
            assert!(rollback.is_err());
            assert_eq!(repository.query().get().await.unwrap().len(), 2);
            assert_eq!(
                f.connection.table("business").value("value").await.unwrap(),
                Value::from("after")
            );
            let mut suppressed = Business::newInstanceOn(
                &f.connection,
                BTreeMap::from([
                    ("id".into(), "two".into()),
                    ("value".into(), "not logged".into()),
                ]),
            )
            .unwrap();
            logger
                .withoutLogging(|| async {
                    suppressed.save().await.unwrap();
                })
                .await;
            assert_eq!(repository.query().get().await.unwrap().len(), 2);
            suppressed.delete().await.unwrap();
            let deleted = repository.query().get().await.unwrap();
            assert_eq!(deleted.len(), 3);
            assert_eq!(deleted[0].event.as_deref(), Some("deleted"));
            assert_eq!(
                deleted[0].attribute_changes,
                json!({"old":{"value":"not logged"}})
            );
        })
        .await;
    f.close().await;
}

#[tokio::test]
async fn tap_actions_placeholders_null_log_name_and_ordinary_large_properties() {
    use openrust_activitylog::{ActivityLogger, actions::LogActivityAction};
    let f = Fixture::new().await;
    let mut logger = ActivityLogger::new(f.connection.clone());
    let action = LogActivityAction::default();
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let values = observed.clone();
    action.beforeLogging(move |activity| {
        values
            .lock()
            .unwrap()
            .push(activity.description.clone().unwrap());
        activity.properties["hook"] = json!(true);
        Ok(())
    });
    logger.setLogActivityAction(action.clone());
    logger
        .tap(
            |activity, event| {
                assert_eq!(event, Some("updated"));
                activity.description =
                    Some(":properties.name changed. :unknown.keep :properties.missing".into());
            },
            Some("updated"),
        )
        .unwrap();
    let record = logger
        .inLog(None::<String>)
        .withProperty("name", "Alice")
        .withProperty("large", "x".repeat(20_000))
        .log("ignored fallback")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.description,
        "Alice changed. :unknown.keep :properties.missing"
    );
    assert_eq!(record.log_name, None);
    assert_eq!(record.properties["hook"], true);
    assert_eq!(observed.lock().unwrap()[0], record.description);
    action.clearBeforeLoggingCallbacks();
    assert!(
        logger
            .log("")
            .await
            .unwrap()
            .unwrap()
            .description
            .is_empty()
    );
    f.close().await;
}

#[tokio::test]
async fn buffer_retries_failed_flush_and_serializes_concurrent_flushes() {
    use openrust_activitylog::{ActivityBuffer, ActivityLogger};
    let f = Fixture::new().await;
    let repository = Arc::new(ActivityRepository::new(f.connection.clone()));
    let buffer = Arc::new(ActivityBuffer::new(repository.clone()));
    let mut logger = ActivityLogger::fromRepository(repository.clone());
    logger.withBuffer(buffer.clone());
    assert_eq!(logger.log("first").await.unwrap().unwrap().id, None);
    logger.log("second").await.unwrap();
    assert_eq!(buffer.count(), 2);
    assert!(buffer.hasPending());
    assert!(repository.query().get().await.unwrap().is_empty());
    f.connection
        .getSchemaBuilder()
        .rename("activity_log", "activity_log_unavailable")
        .await
        .unwrap();
    assert!(buffer.flush().await.is_err());
    assert_eq!(buffer.count(), 2);
    f.connection
        .getSchemaBuilder()
        .rename("activity_log_unavailable", "activity_log")
        .await
        .unwrap();
    let (first, second) = tokio::join!(buffer.flush(), buffer.flush());
    assert_eq!(first.unwrap() + second.unwrap(), 2);
    assert!(!buffer.hasPending());
    let records = repository.query().get().await.unwrap();
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|record| record.id.is_some_and(|id| id > 0))
    );
    assert_eq!(logger.flush().await.unwrap(), 0);
    f.close().await;
}

#[derive(Clone)]
struct SoftBusiness(
    openrust::database::eloquent::ModelState,
    openrust_activitylog::models::LoggingState,
);
impl openrust::database::eloquent::Model for SoftBusiness {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static METADATA: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("business")
                .incrementing(false)
                .fillable(&["id", "value", "data", "flag"])
                .timestamps(None, None)
                .softDeletes("removed_on");
        &METADATA
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state, Default::default())
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
    fn casts() -> &'static [(
        &'static str,
        openrust::database::eloquent::casts::AttributeCast,
    )] {
        use openrust::database::eloquent::casts::AttributeCast;
        &[
            ("data", AttributeCast::Json),
            ("flag", AttributeCast::Boolean),
        ]
    }
}
impl openrust_activitylog::models::concerns::LogsActivity for SoftBusiness {
    fn activitylogState(&self) -> &openrust_activitylog::models::LoggingState {
        &self.1
    }

    fn getActivitylogOptions(&self) -> openrust_activitylog::LogOptions {
        openrust_activitylog::LogOptions::defaults()
            .logOnly(&["value", "data->nested->name", "flag"])
            .useAttributeRawValues(&["flag"])
            .logOnlyDirty()
            .dontLogEmptyChanges()
    }
}

#[tokio::test]
async fn model_json_raw_casts_and_custom_soft_delete_column_restore() {
    use openrust::database::eloquent::Model;
    use openrust_activitylog::models::concerns::LogsActivity;
    let f = Fixture::new().await;
    let repository = f.app.make::<ActivityRepository>().await.unwrap();
    SoftBusiness::bootLogsActivity(&f.app, repository.clone()).unwrap();
    let mut model = SoftBusiness::newInstanceOn(
        &f.connection,
        BTreeMap::from([
            ("id".into(), "soft".into()),
            ("value".into(), "name".into()),
            (
                "data".into(),
                Value::Json(json!({"nested":{"name":"Alice","secret":"excluded"}})),
            ),
            ("flag".into(), Value::Text("1".into())),
        ]),
    )
    .unwrap();
    model.save().await.unwrap();
    let rows = repository.query().get().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].attribute_changes,
        json!({"attributes":{"value":"name","data":{"nested":{"name":"Alice"}},"flag":"1"}})
    );
    model.delete().await.unwrap();
    assert!(model.trashed());
    assert_eq!(repository.query().get().await.unwrap().len(), 2);
    model.restore().await.unwrap();
    assert!(!model.trashed());
    let rows = repository.query().get().await.unwrap();
    assert_eq!(rows.len(), 3, "restore must not also log updated");
    assert_eq!(rows[0].event.as_deref(), Some("restored"));
    assert_eq!(rows[1].event.as_deref(), Some("deleted"));
    f.close().await;
}

#[derive(Clone)]
struct Owner(openrust::database::eloquent::ModelState);
impl openrust::database::eloquent::Model for Owner {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("activity_owners")
                .timestamps(None, None)
                .guarded(&[]);
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
#[derive(Clone)]
struct RelatedBusiness(
    openrust::database::eloquent::ModelState,
    openrust_activitylog::models::LoggingState,
);
impl openrust::database::eloquent::Model for RelatedBusiness {
    fn metadata() -> &'static openrust::database::eloquent::ModelMetadata {
        static META: openrust::database::eloquent::ModelMetadata =
            openrust::database::eloquent::ModelMetadata::new("business")
                .incrementing(false)
                .timestamps(None, None)
                .guarded(&[]);
        &META
    }
    fn from_state(state: openrust::database::eloquent::ModelState) -> Self {
        Self(state, Default::default())
    }
    fn state(&self) -> &openrust::database::eloquent::ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut openrust::database::eloquent::ModelState {
        &mut self.0
    }
    fn namedRelations() -> openrust::database::eloquent::NamedRelations<Self> {
        openrust::database::eloquent::NamedRelations::default().nested("owner", || {
            openrust::database::eloquent::relations::BelongsTo::<Self, Owner>::new("owner_id", "id")
        })
    }
}
impl openrust_activitylog::models::concerns::LogsActivity for RelatedBusiness {
    fn activitylogState(&self) -> &openrust_activitylog::models::LoggingState {
        &self.1
    }
    fn getActivitylogOptions(&self) -> openrust_activitylog::LogOptions {
        openrust_activitylog::LogOptions::defaults()
            .logOnly(&["owner.name"])
            .logOnlyDirty()
    }
}
#[tokio::test]
async fn lazy_related_attributes_keep_old_foreign_key_and_instance_logging_switches() {
    use openrust::database::eloquent::Model;
    use openrust_activitylog::models::concerns::LogsActivity;
    let f = Fixture::new().await;
    f.connection
        .getSchemaBuilder()
        .create("activity_owners", |table| {
            table.id();
            table.string("name", 255);
        })
        .await
        .unwrap();
    f.connection
        .table("activity_owners")
        .insert(&[
            BTreeMap::from([
                ("id".into(), Value::from(1_i64)),
                ("name".into(), Value::from("first owner")),
            ]),
            BTreeMap::from([
                ("id".into(), Value::from(2_i64)),
                ("name".into(), Value::from("second owner")),
            ]),
        ])
        .await
        .unwrap();
    let repository = Arc::new(ActivityRepository::new(f.connection.clone()));
    RelatedBusiness::bootLogsActivity(&f.app, repository.clone()).unwrap();
    let mut model = RelatedBusiness::newInstanceOn(&f.connection, Default::default()).unwrap();
    model.setAttribute("id", "related");
    model.setAttribute("value", "value");
    model.setAttribute("owner_id", 1_i64);
    model.save().await.unwrap();
    assert_eq!(
        repository.query().get().await.unwrap()[0].attribute_changes["attributes"]["owner.name"],
        "first owner"
    );
    model.setAttribute("owner_id", 2_i64);
    model.save().await.unwrap();
    let changes = &repository.query().get().await.unwrap()[0].attribute_changes;
    assert_eq!(changes["old"]["owner.name"], "first owner");
    assert_eq!(changes["attributes"]["owner.name"], "second owner");
    model.disableLogging();
    let mut other = model.clone();
    other.enableLogging();
    assert!(!model.enableLoggingModelsEvents());
    assert!(other.enableLoggingModelsEvents());
    model.setAttribute("owner_id", 1_i64);
    model.save().await.unwrap();
    assert_eq!(repository.query().get().await.unwrap().len(), 2);
    model.enableLogging().setAttribute("owner_id", Value::Null);
    model.save().await.unwrap();
    let records = repository.query().get().await.unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(
        records[0].attribute_changes["old"]["owner.name"],
        "first owner"
    );
    assert!(records[0].attribute_changes["attributes"]["owner.name"].is_null());
    let mut transactional =
        RelatedBusiness::newInstanceOn(&f.connection, Default::default()).unwrap();
    transactional.setAttribute("id", "transactional");
    transactional.setAttribute("value", "value");
    transactional.setAttribute("owner_id", 3_i64);
    f.connection
        .transaction(
            move |tx| {
                let mut model = transactional.clone();
                Box::pin(async move {
                    tx.table("activity_owners")
                        .insert(&[BTreeMap::from([
                            ("id".to_owned(), Value::from(3_i64)),
                            ("name".to_owned(), Value::from("Uncommitted owner")),
                        ])])
                        .await?;
                    model.saveInTransaction(tx).await?;
                    Ok(())
                })
            },
            1,
        )
        .await
        .unwrap();
    let records = repository.query().get().await.unwrap();
    assert_eq!(records.len(), 4);
    assert_eq!(
        records[0].attribute_changes["attributes"]["owner.name"],
        "Uncommitted owner"
    );
    f.connection
        .getSchemaBuilder()
        .dropIfExists("activity_owners")
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn native_activity_model_casts_and_soft_deleted_subject_configuration() {
    use openrust::database::eloquent::{Model, relations::Relation};
    use openrust_activitylog::{ActivityLogger, models::Activity as ActivityModel};
    let f = Fixture::new().await;
    let mut subject = SoftBusiness::newInstanceOn(&f.connection, Default::default()).unwrap();
    subject.setAttribute("id", "soft-subject");
    subject.setAttribute("value", "value");
    subject.save().await.unwrap();
    let mut logger = ActivityLogger::new(f.connection.clone());
    let record = logger
        .onModel(&subject)
        .unwrap()
        .byModel(&subject)
        .unwrap()
        .withProperties(json!({"nested":{"items":["value"]}}))
        .log("subject :subject.value")
        .await
        .unwrap()
        .unwrap();
    let entry = ActivityModel::queryOn(&f.connection)
        .find(record.id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        entry.getProperty("nested.items.0", json!(null)).unwrap(),
        "value"
    );
    assert_eq!(
        entry.getProperty("missing", json!("default")).unwrap(),
        "default"
    );
    assert_eq!(record.getProperty("nested.items.0", json!(null)), "value");
    assert_eq!(record.description, "subject value");
    f.app
        .call(
            openrust::container::Callback::new(&[], |_| async {
                let relation = entry.subject::<SoftBusiness>().unwrap();
                assert!(
                    relation
                        .getResults(&entry)
                        .await
                        .unwrap()
                        .unwrap()
                        .downcast_ref::<SoftBusiness>()
                        .is_some()
                );
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    subject.delete().await.unwrap();
    f.app
        .call(
            openrust::container::Callback::new(&[], |_| async {
                assert!(
                    entry
                        .subject::<SoftBusiness>()
                        .unwrap()
                        .getResults(&entry)
                        .await
                        .unwrap()
                        .is_none()
                );
                f.app
                    .config()
                    .unwrap()
                    .set("activitylog.include_soft_deleted_subjects", json!(true))
                    .unwrap();
                assert!(
                    entry
                        .subject::<SoftBusiness>()
                        .unwrap()
                        .getResults(&entry)
                        .await
                        .unwrap()
                        .is_some()
                );
                assert!(
                    entry
                        .causer::<SoftBusiness>()
                        .unwrap()
                        .getResults(&entry)
                        .await
                        .unwrap()
                        .is_none()
                );
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    f.close().await;
}

#[tokio::test]
async fn buffered_model_events_wait_for_commit_and_discard_rolled_back_savepoints() {
    use openrust::database::eloquent::Model;
    use openrust_activitylog::{ActivityBuffer, ActivityLogger, models::concerns::LogsActivity};
    let f = Fixture::new().await;
    let repository = f.app.make::<ActivityRepository>().await.unwrap();
    Business::bootLogsActivity(&f.app, repository.clone()).unwrap();
    let buffer = Arc::new(ActivityBuffer::new(repository.clone()));
    let mut logger = ActivityLogger::fromRepository(repository.clone());
    logger.withBuffer(buffer.clone());
    logger
        .scope(async {
            for commit in [false, true] {
                let pending = buffer.clone();
                let result = f
                    .connection
                    .transaction(
                        move |tx| {
                            let pending = pending.clone();
                            Box::pin(async move {
                                let mut model = Business::newInstanceOn(
                                    tx.getConnection(),
                                    BTreeMap::from([
                                        (
                                            "id".into(),
                                            if commit { "committed" } else { "rolled-back" }.into(),
                                        ),
                                        ("value".into(), "outer".into()),
                                    ]),
                                )?;
                                model.saveInTransaction(tx).await?;
                                assert_eq!(pending.count(), 0);
                                assert_eq!(pending.flush().await.unwrap(), 0);
                                let mut rolled = model.clone();
                                let result = tx
                                    .transaction(|nested| {
                                        Box::pin(async move {
                                            rolled
                                                .updateInTransaction(
                                                    BTreeMap::from([(
                                                        "value".into(),
                                                        "nested".into(),
                                                    )]),
                                                    nested,
                                                )
                                                .await?;
                                            Err::<(), _>(
                                                openrust::database::DatabaseError::InvalidQuery(
                                                    "rollback savepoint",
                                                ),
                                            )
                                        })
                                    })
                                    .await;
                                assert!(result.is_err());
                                tx.transaction(|nested| {
                                    Box::pin(async move {
                                        let mut inner = Business::newInstanceOn(
                                            nested.getConnection(),
                                            BTreeMap::from([
                                                ("id".into(), "nested-committed".into()),
                                                ("value".into(), "inner".into()),
                                            ]),
                                        )?;
                                        inner.saveInTransaction(nested).await?;
                                        Ok(())
                                    })
                                })
                                .await?;
                                model
                                    .updateInTransaction(
                                        BTreeMap::from([("value".into(), "last".into())]),
                                        tx,
                                    )
                                    .await?;
                                assert_eq!(pending.count(), 0);
                                if commit {
                                    Ok(())
                                } else {
                                    Err(openrust::database::DatabaseError::InvalidQuery(
                                        "rollback transaction",
                                    ))
                                }
                            })
                        },
                        1,
                    )
                    .await;
                assert_eq!(result.is_ok(), commit);
                assert_eq!(buffer.count(), 3 * usize::from(commit));
            }
        })
        .await;
    assert_eq!(buffer.flush().await.unwrap(), 3);
    let rows = repository.query().get().await.unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows.iter()
            .rev()
            .map(|row| row.attribute_changes["attributes"]["value"]
                .as_str()
                .unwrap())
            .collect::<Vec<_>>(),
        ["outer", "inner", "last"]
    );
    assert_eq!(rows[0].subject.as_ref().unwrap().id, "committed");
    f.close().await;
}
