use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Db, Value,
        events::{MigrationStarted, QueryExecuted, TransactionBeginning},
        migrations::{Migration, MigrationContext, MigrationEntry, MigrationRegistry},
        query::Record,
        seeder::{Seeder, SeederContext, SeederEntry, SeederRegistry},
    },
    foundation::{
        Application,
        configuration::{ApplicationBuilder, Routing},
    },
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::{ProviderResult, ServiceProvider, facades::Route},
    testing::{
        DatabaseMigrationsOptions, DatabaseTruncationOptions, DatabaseTruncationTables,
        RefreshDatabaseOptions, TestApp,
    },
};
use serde_json::json;
use sqlx::{Connection as _, MySqlConnection};

struct CreateItems;

impl Migration for CreateItems {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("items", |table| {
                    table.id();
                    table.string("name", 40);
                    table.string("description", 80).nullable();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move { context.get_schema_builder().drop_if_exists("items").await })
    }
}

struct SeedItems;

impl Seeder for SeedItems {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .connection()
                .table("items")
                .insert(&[Record::from([("name".into(), "seeded".into())])])
                .await?;
            Ok(())
        })
    }
}

struct OrderedSeeder(&'static str);

impl Seeder for OrderedSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .connection()
                .table("items")
                .insert(&[Record::from([("name".into(), self.0.into())])])
                .await?;
            Ok(())
        })
    }
}

struct InventoryProvider;

struct RefreshSeeder;

struct MigrationEventProbe;

static PAUSE_MIGRATION_EVENT_PROBE: AtomicBool = AtomicBool::new(false);
static MIGRATION_EVENT_PROBE_STARTED: tokio::sync::Notify = tokio::sync::Notify::const_new();
static RESUME_MIGRATION_EVENT_PROBE: tokio::sync::Notify = tokio::sync::Notify::const_new();

impl Seeder for MigrationEventProbe {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut transaction = context.connection().begin_transaction().await?;
            transaction
                .select("SELECT 701 AS selected_probe", &[])
                .await?;
            transaction.roll_back(None).await?;
            if PAUSE_MIGRATION_EVENT_PROBE.swap(false, Ordering::Relaxed) {
                MIGRATION_EVENT_PROBE_STARTED.notify_one();
                RESUME_MIGRATION_EVENT_PROBE.notified().await;
            }
            Db::connection(Some("reports"))?
                .select("SELECT 702 AS other_probe", &[])
                .await?;
            Ok(())
        })
    }
}

static REFRESH_RUNS: AtomicUsize = AtomicUsize::new(0);
static PAUSE_REFRESH_SEEDER: AtomicBool = AtomicBool::new(false);
static REFRESH_SEEDER_STARTED: tokio::sync::Notify = tokio::sync::Notify::const_new();
static RESUME_REFRESH_SEEDER: tokio::sync::Notify = tokio::sync::Notify::const_new();

impl Seeder for RefreshSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            SeedItems.run(context).await?;
            if REFRESH_RUNS.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err(DatabaseError::InvalidConfiguration(
                    "intentional refresh seeder failure",
                ));
            }
            if PAUSE_REFRESH_SEEDER.swap(false, Ordering::Relaxed) {
                REFRESH_SEEDER_STARTED.notify_one();
                RESUME_REFRESH_SEEDER.notified().await;
            }
            Ok(())
        })
    }
}

impl ServiceProvider for InventoryProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
                "database/migrations/0001_create_items.rs",
                &CreateItems,
            )];
            static SEEDERS: &[SeederEntry] = &[
                SeederEntry::new("DatabaseSeeder", &SeedItems),
                SeederEntry::new("FirstSeeder", &OrderedSeeder("first")),
                SeederEntry::new("SecondSeeder", &OrderedSeeder("second")),
                SeederEntry::new("RefreshSeeder", &RefreshSeeder),
                SeederEntry::new("MigrationEventProbe", &MigrationEventProbe),
            ];
            app.container()
                .instance(Arc::new(MigrationRegistry(MIGRATIONS)))?;
            app.container()
                .instance(Arc::new(SeederRegistry(SEEDERS)))?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn mysql_refresh_mutes_only_selected_connection_events_during_preparation()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut app = TestApp::mysql_connections(&[("mysql", &url), ("reports", &url)], |builder| {
        builder.with_providers(vec![Box::new(InventoryProvider)])
    })
    .await?;
    let events = app
        .fake_events(&[
            QueryExecuted::NAME,
            TransactionBeginning::NAME,
            MigrationStarted::NAME,
        ])
        .await?;
    let db = app.database().await?;
    db.enable_query_log();
    let recorded = |sql: &str| {
        events.dispatched(QueryExecuted::NAME).iter().any(|event| {
            event
                .downcast_ref::<QueryExecuted>()
                .is_some_and(|query| query.sql == sql)
        })
    };
    let options = RefreshDatabaseOptions {
        without_migration_events: true,
        seeder: Some("MigrationEventProbe".into()),
        ..Default::default()
    };
    PAUSE_MIGRATION_EVENT_PROBE.store(true, Ordering::Relaxed);
    let refresh = app.refresh_database(options.clone(), |app| {
        Box::pin(async {
            app.database()
                .await?
                .select("SELECT 703 AS callback_probe", &[])
                .await?;
            Ok(())
        })
    });
    let observe = async {
        let ready = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            MIGRATION_EVENT_PROBE_STARTED.notified(),
        )
        .await;
        let result = if ready.is_ok() {
            db.select("SELECT 705 AS concurrent_probe", &[]).await
        } else {
            Err(DatabaseError::InvalidConfiguration(
                "migration event probe did not reach its pause",
            ))
        };
        RESUME_MIGRATION_EVENT_PROBE.notify_one();
        result
    };
    let (refresh, concurrent) = tokio::join!(refresh, observe);
    refresh?;
    concurrent?;
    assert!(!recorded("SELECT 1"));
    assert!(!recorded("SELECT 701 AS selected_probe"));
    assert!(recorded("SELECT 702 AS other_probe"));
    assert!(recorded("SELECT 703 AS callback_probe"));
    assert!(recorded("SELECT 705 AS concurrent_probe"));
    assert!(
        db.get_query_log()
            .iter()
            .any(|entry| entry.query == "SELECT 701 AS selected_probe")
    );
    assert!(events.dispatched(TransactionBeginning::NAME).is_empty());
    assert_eq!(events.dispatched(MigrationStarted::NAME).len(), 1);

    let missing = RefreshDatabaseOptions {
        seeder: Some("MissingSeeder".into()),
        ..options
    };
    app.refresh_database::<()>(missing, |_| {
        Box::pin(async { panic!("failed seed must not run the callback") })
    })
    .await
    .unwrap_err();
    db.select("SELECT 704 AS after_failed_refresh", &[]).await?;
    assert!(recorded("SELECT 704 AS after_failed_refresh"));
    app.artisan(["migrate:fresh", "--seeder=MigrationEventProbe"])
        .await
        .assert_successful();
    assert!(recorded("SELECT 701 AS selected_probe"));
    assert!(!events.dispatched(TransactionBeginning::NAME).is_empty());
    app.close().await?;
    Ok(())
}

#[tokio::test]
async fn migrations_and_seeders_only_change_their_own_sqlite_fixture()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let configure = |builder: ApplicationBuilder| {
        builder
            .with_config(|_, _| {
                Ok([
                    ("app".into(), json!({"env": "testing"})),
                    (
                        "database".into(),
                        json!({
                            "default": "external",
                            "connections": {"external": {"driver": "unavailable"}},
                            "migrations": {"table": "test_migrations"},
                        }),
                    ),
                ]
                .into())
            })
            .with_providers(vec![Box::new(InventoryProvider)])
    };

    let mut first = TestApp::sqlite(configure)?;
    let mut second = TestApp::sqlite(configure)?;
    let first_root = first.root().to_owned();
    let second_root = second.root().to_owned();
    assert_ne!(first_root, second_root);

    let (first_result, second_result) = tokio::join!(
        first.artisan(["migrate", "--seed"]),
        second.artisan(["migrate", "--seed"]),
    );
    first_result.assert_successful();
    second_result.assert_successful();
    let config = first.app().config()?;
    assert_eq!(config.get("database.default"), Some(json!("sqlite")));
    assert!(config.get("database.connections.external").is_none());
    assert_eq!(
        config.get("database.migrations.table"),
        Some(json!("test_migrations"))
    );

    first
        .assert_database_has(
            "items",
            &[("name", "seeded".into()), ("description", Value::Null)],
        )
        .await?;
    first.assert_database_count("test_migrations", 1).await?;
    first
        .database()
        .await?
        .table("items")
        .insert(&[Record::from([("name".into(), "first only".into())])])
        .await?;
    second
        .assert_database_missing("items", &[("name", "first only".into())])
        .await?;

    let invalid = second
        .artisan(["migrate:fresh", "--seed", "--seeder", "MissingSeeder"])
        .await;
    invalid.assert_exit_code(1);
    assert!(invalid.stderr.contains("MissingSeeder"));
    second
        .artisan(["migrate:fresh", "--seed"])
        .await
        .assert_successful();
    second.assert_database_count("items", 1).await?;
    first.assert_database_count("items", 2).await?;

    drop(first);
    assert!(!first_root.exists());
    assert!(second_root.join("testing.sqlite").is_file());
    drop(second);
    assert!(!second_root.exists());
    Ok(())
}

#[tokio::test]
async fn mysql_test_transaction_shares_http_artisan_and_nested_writes()
-> Result<(), Box<dyn Error + Send + Sync>> {
    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    }
    let configure = |builder: ApplicationBuilder| {
        builder
            .with_config(|_, _| {
                let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
                Ok([(
                    "database".into(),
                    json!({
                        "default": "mysql", "connections": {"mysql": {"url": url}},
                    }),
                )]
                .into())
            })
            .with_providers(vec![Box::new(InventoryProvider)])
            .with_routing(Routing::default().web(|| {
                Route::get("/write", write_item)?;
                Ok(())
            }))
    };
    let mut test = TestApp::create(configure)?;
    let mut observer = TestApp::create(configure)?;
    test.artisan(["migrate", "--seed"])
        .await
        .assert_successful();
    let external = observer.database().await?;

    test.database_transaction(|test| {
        Box::pin(async {
            test.assert_database_count("items", 1).await?;
            test.get("/write")
                .await?
                .assert_status(StatusCode::NO_CONTENT);
            test.artisan(["db:seed"]).await.assert_successful();
            test.assert_database_has("items", &[("name", "http".into())])
                .await?;
            let db = test.database().await?;
            let mut nested = db.begin_transaction().await?;
            nested
                .table("items")
                .insert(&[Record::from([("name".into(), "nested".into())])])
                .await?;
            nested.commit().await?;
            test.assert_database_count("items", 4).await?;
            assert!(
                db.statement("CREATE TABLE must_not_exist (id INT)", &[])
                    .await
                    .is_err()
            );
            Ok(())
        })
    })
    .await?;
    assert_eq!(external.table("items").count("*").await?, 1);
    test.assert_database_missing("items", &[("name", "http".into())])
        .await?;

    let error = test
        .database_transaction(|test| {
            Box::pin(async {
                test.artisan(["db:seed"]).await.assert_successful();
                Err::<(), Box<dyn Error + Send + Sync>>("callback failed".into())
            })
        })
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "callback failed");
    assert_eq!(external.table("items").count("*").await?, 1);

    let interrupted = test
        .database_transaction(|test| {
            Box::pin(async {
                let db = test.database().await?;
                let mut abandoned = db.begin_transaction().await?;
                abandoned
                    .table("items")
                    .insert(&[Record::from([("name".into(), "abandoned".into())])])
                    .await?;
                drop(abandoned);
                assert!(db.table("items").count("*").await.is_err());
                Ok(())
            })
        })
        .await
        .unwrap_err();
    assert!(interrupted.to_string().contains("transaction"));
    assert_eq!(external.table("items").count("*").await?, 1);

    {
        let (written, ready) = tokio::sync::oneshot::channel();
        let test = test.database_transaction(|test| {
            Box::pin(async move {
                test.get("/write")
                    .await?
                    .assert_status(StatusCode::NO_CONTENT);
                written.send(()).unwrap();
                std::future::pending::<Result<(), Box<dyn Error + Send + Sync>>>().await
            })
        });
        tokio::pin!(test);
        tokio::select! {
            result = &mut test => panic!("test transaction returned before cancellation: {result:?}"),
            result = ready => result?,
        }
    }
    assert_eq!(external.table("items").count("*").await?, 1);
    Ok(())
}

fn write_item(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        async {
            Db::connection(None)?
                .table("items")
                .insert(&[Record::from([("name".into(), "http".into())])])
                .await
        }
        .await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}

#[tokio::test]
async fn managed_mysql_test_schemas_keep_ownership_and_close_their_pools()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let configure =
        |builder: ApplicationBuilder| builder.with_providers(vec![Box::new(InventoryProvider)]);
    let mut server = MySqlConnection::connect(&url).await?;
    let (first, second) = tokio::join!(
        TestApp::mysql(&url, configure),
        TestApp::mysql(&url, configure)
    );
    let mut first = first?;
    let mut second = second?;
    first
        .artisan(["migrate", "--seed"])
        .await
        .assert_successful();
    second.artisan(["migrate"]).await.assert_successful();
    first.assert_database_count("items", 1).await?;
    second.assert_database_count("items", 0).await?;
    let first_db = first.database().await?;
    let second_db = second.database().await?;
    let first_name = first_db.get_database_name().to_owned();
    let second_name = second_db.get_database_name().to_owned();
    assert_ne!(first_name, second_name);
    first
        .artisan(["migrate:fresh", "--seed"])
        .await
        .assert_successful();
    first.assert_database_count("items", 1).await?;
    first.close().await?;
    assert!(!schema_exists(&mut server, &first_name).await?);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            first_db.table("items").count("*"),
        )
        .await?
        .is_err()
    );
    assert!(schema_exists(&mut server, &second_name).await?);

    let mut selected = url::Url::parse(&url)?;
    selected.set_path(&format!("/{second_name}"));
    let foreign = MySqlConnection::connect(selected.as_str()).await?;
    let busy = second.close().await.unwrap_err();
    assert!(busy.to_string().contains("busy"), "{busy}");
    assert!(schema_exists(&mut server, &second_name).await?);
    foreign.close().await?;

    let mut changed = TestApp::mysql(&url, configure).await?;
    let connection = changed.database().await?;
    let name = connection.get_database_name().to_owned();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP PROCEDURE `{name}`.`openrust_fixture_owner`"
    )))
    .execute(&mut server)
    .await?;
    connection.disconnect();
    assert!(connection.select("SELECT 1", &[]).await.is_err());
    let rejected = changed.close().await.unwrap_err();
    assert!(
        rejected.to_string().contains("ownership marker"),
        "{rejected}"
    );
    assert!(schema_exists(&mut server, &name).await?);
    Ok(())
}

#[tokio::test]
async fn mysql_recovers_a_fixture_after_process_kill() -> Result<(), Box<dyn Error + Send + Sync>> {
    use std::{fs, path::Path, time::Duration};

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let configure =
        |builder: ApplicationBuilder| builder.with_providers(vec![Box::new(InventoryProvider)]);
    const CHILD_DIRECTORY: &str = "OPENRUST_TEST_MYSQL_RECOVERY_CHILD";
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let directory = Path::new(&directory);
        let mut fixture = TestApp::mysql_with_recovery(&url, directory, configure).await?;
        fixture
            .artisan(["migrate", "--seed"])
            .await
            .assert_successful();
        fixture.assert_database_count("items", 1).await?;
        let database = fixture.database().await?;
        fs::write(directory.join("schema.tmp"), database.get_database_name())?;
        fs::rename(directory.join("schema.tmp"), directory.join("ready"))?;
        std::future::pending::<()>().await;
        unreachable!();
    }

    let journal = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(journal.path(), fs::Permissions::from_mode(0o700))?;
    }
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "mysql_recovers_a_fixture_after_process_kill",
            "--nocapture",
        ])
        .env(CHILD_DIRECTORY, journal.path())
        .kill_on_drop(true)
        .spawn()?;
    let ready = journal.path().join("ready");
    let name = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if ready.exists() {
                return fs::read_to_string(&ready);
            }
            if let Some(status) = child.try_wait()? {
                return Err(std::io::Error::other(format!(
                    "fixture child exited early: {status}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    let receipt = journal.path().join(format!("{name}.json"));
    let original = fs::read(&receipt)?;
    let mut server = MySqlConnection::connect(&url).await?;
    assert!(schema_exists(&mut server, &name).await?);
    let held = TestApp::recover_mysql(&url, &receipt).await.unwrap_err();
    assert!(held.to_string().contains("lease is already held"), "{held}");

    let mut selected = url::Url::parse(&url)?;
    selected.set_path(&format!("/{name}"));
    let foreign = MySqlConnection::connect(selected.as_str()).await?;
    child.kill().await?;
    assert!(!child.wait().await?.success());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let free: Option<i64> = sqlx::query_scalar("SELECT IS_FREE_LOCK(?)")
                .bind(&name)
                .fetch_one(&mut server)
                .await?;
            if free == Some(1) {
                return Ok::<_, sqlx::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    let busy = TestApp::recover_mysql(&url, &receipt).await.unwrap_err();
    assert!(busy.to_string().contains("busy"), "{busy}");
    foreign.close().await?;
    assert_eq!(fs::read(&receipt)?, original);
    assert!(schema_exists(&mut server, &name).await?);

    sqlx::raw_sql("CREATE USER 'fixture_no_schema'@'%' IDENTIFIED BY 'test-only'")
        .execute(&mut server)
        .await?;
    let mut restricted = url::Url::parse(&url)?;
    restricted.set_username("fixture_no_schema").unwrap();
    restricted.set_password(Some("test-only")).unwrap();
    restricted.set_path("/");
    let invisible = TestApp::recover_mysql(restricted.as_str(), &receipt)
        .await
        .unwrap_err();
    assert!(
        invisible.to_string().contains("could not verify"),
        "{invisible}"
    );
    assert_eq!(fs::read(&receipt)?, original);
    sqlx::raw_sql("DROP USER 'fixture_no_schema'@'%'")
        .execute(&mut server)
        .await?;

    let mut altered: serde_json::Value = serde_json::from_slice(&original)?;
    altered["server"] = json!("00000000-0000-0000-0000-000000000000");
    fs::write(&receipt, serde_json::to_vec(&altered)?)?;
    let other = TestApp::recover_mysql(&url, &receipt).await.unwrap_err();
    assert!(other.to_string().contains("another server"), "{other}");
    altered = serde_json::from_slice(&original)?;
    altered["token"] = json!("00000000000000000000000000000000");
    fs::write(&receipt, serde_json::to_vec(&altered)?)?;
    let rejected = TestApp::recover_mysql(&url, &receipt).await.unwrap_err();
    assert!(
        rejected.to_string().contains("ownership marker"),
        "{rejected}"
    );
    assert!(receipt.exists() && schema_exists(&mut server, &name).await?);

    let mut independent = TestApp::mysql_with_recovery(&url, journal.path(), configure).await?;
    let other_name = independent.database().await?.get_database_name().to_owned();
    let independent_receipt = journal.path().join(format!("{other_name}.json"));
    assert!(independent_receipt.exists());
    fs::write(&receipt, &original)?;
    TestApp::recover_mysql(&url, &receipt).await?;
    assert!(!receipt.exists() && !schema_exists(&mut server, &name).await?);
    assert!(schema_exists(&mut server, &other_name).await?);
    // Replay a receipt left behind after the database was already dropped.
    fs::write(&receipt, original)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&receipt, fs::Permissions::from_mode(0o600))?;
    }
    TestApp::recover_mysql(&url, &receipt).await?;
    TestApp::recover_mysql(&url, &receipt).await?;
    assert!(!receipt.exists());
    independent.close().await?;
    assert!(!independent_receipt.exists() && !schema_exists(&mut server, &other_name).await?);
    Ok(())
}

async fn schema_exists(server: &mut MySqlConnection, name: &str) -> Result<bool, sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        "SELECT CAST(COUNT(*) AS SIGNED) FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
    )
    .bind(name)
    .fetch_one(server)
    .await?;
    Ok(count != 0)
}

#[tokio::test]
async fn mysql_database_truncation_preserves_schema_filters_and_reseeds()
-> Result<(), Box<dyn Error + Send + Sync>> {
    struct TruncationConfig;

    impl ServiceProvider for TruncationConfig {
        fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                let config = app.config()?;
                config.set("database.connections.mysql.prefix", json!("fixture_"))?;
                config.set("database.migrations.table", json!("history"))?;
                Ok(())
            })
        }
    }

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut test = TestApp::mysql_connections(&[("mysql", &url), ("reports", &url)], |builder| {
        builder.with_providers(vec![
            Box::new(InventoryProvider),
            Box::new(TruncationConfig),
        ])
    })
    .await?;
    test.artisan(["migrate", "--database=reports", "--seed"])
        .await
        .assert_successful();
    let manager = test.app().make::<DatabaseManager>().await?;
    let mysql = manager.connection(None)?;
    let reports = manager.connection(Some("reports"))?;
    assert_eq!(mysql.get_table_prefix(), "fixture_");
    let options = DatabaseTruncationOptions {
        seed: true,
        seeder: Some("SecondSeeder".into()),
        connections_to_truncate: vec!["mysql".into(), "reports".into()],
        tables: DatabaseTruncationTables {
            except: vec!["fixture_keep".into()],
            ..Default::default()
        },
        tables_by_connection: [(
            "reports".into(),
            DatabaseTruncationTables {
                only: vec![format!("{}.items", reports.get_database_name())],
                except: vec!["items".into()],
            },
        )]
        .into(),
        ..Default::default()
    };
    test.truncate_database_tables(options.clone()).await?;
    test.assert_database_has("items", &[("id", 1_i64.into()), ("name", "second".into())])
        .await?;
    test.assert_database_count_on("items", 1, Some("reports"))
        .await?;
    mysql
        .unprepared("create table fixture_keep (id int primary key)")
        .await?;
    mysql
        .unprepared("insert into fixture_keep values (1)")
        .await?;
    mysql.unprepared("create table fixture_children (id int primary key, item_id bigint unsigned, foreign key (item_id) references fixture_items (id))").await?;
    mysql
        .unprepared("insert into fixture_children values (1, 1)")
        .await?;
    mysql
        .unprepared("create table fixture_empty (id int auto_increment primary key)")
        .await?;
    mysql
        .unprepared("insert into fixture_empty values (null)")
        .await?;
    mysql.unprepared("delete from fixture_empty").await?;
    reports
        .unprepared("create table notes (id int primary key)")
        .await?;
    reports.unprepared("insert into notes values (1)").await?;
    test.seed().await?;

    let events = test.fake_events(&[QueryExecuted::NAME]).await?;
    test.truncate_database_tables(options.clone()).await?;
    assert!(events.dispatched(QueryExecuted::NAME).iter().all(|event| {
        !event
            .downcast_ref::<QueryExecuted>()
            .unwrap()
            .sql
            .starts_with("truncate table ")
    }));
    test.assert_database_count("items", 1).await?;
    test.assert_database_has("items", &[("id", 1_i64.into()), ("name", "second".into())])
        .await?;
    test.assert_database_count("keep", 1).await?;
    test.assert_database_count("history", 1).await?;
    test.assert_database_empty("children").await?;
    mysql
        .unprepared("insert into fixture_empty values (null)")
        .await?;
    test.assert_database_has("empty", &[("id", 2_i64.into())])
        .await?;
    test.assert_database_empty_on("items", Some("reports"))
        .await?;
    test.assert_database_count_on("notes", 1, Some("reports"))
        .await?;
    assert!(
        mysql
            .unprepared("insert into fixture_children values (1, 999)")
            .await
            .is_err()
    );

    let mut failing = options.clone();
    failing.seeder = Some("UnknownSeeder".into());
    let error = test.truncate_database_tables(failing).await.unwrap_err();
    assert!(
        error.to_string().contains("db:seed exited with 1"),
        "{error}"
    );
    test.assert_database_empty("items").await?;
    test.truncate_database_tables(options.clone()).await?;
    test.assert_database_has("items", &[("id", 1_i64.into()), ("name", "second".into())])
        .await?;
    test.assert_database_count("keep", 1).await?;
    test.assert_database_count_on("notes", 1, Some("reports"))
        .await?;
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let started = entered.clone();
    let allowed = release.clone();
    let pause_once = AtomicBool::new(true);
    mysql.before_executing(move |sql, _, _, _| {
        let pause = sql.starts_with("truncate table ") && pause_once.swap(false, Ordering::Relaxed);
        let started = started.clone();
        let allowed = allowed.clone();
        Box::pin(async move {
            if pause {
                started.notify_one();
                allowed.notified().await;
            }
            Ok(())
        })
    });
    let mut pending = Box::pin(test.truncate_database_tables(options));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::select! {
            () = entered.notified() => {}
            result = &mut pending => panic!("truncation finished before cancellation: {result:?}"),
        }
    })
    .await?;
    drop(pending);
    let mut closing = Box::pin(test.close());
    let waiting = tokio::time::timeout(std::time::Duration::from_millis(25), &mut closing).await;
    release.notify_one();
    assert!(
        waiting.is_err(),
        "fixture closed before its truncation task finished"
    );
    closing.await
}

#[tokio::test]
async fn mysql_database_migrations_rebuilds_and_rolls_back_the_owned_schema()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut test = TestApp::mysql_connections(&[("mysql", &url), ("reports", &url)], |builder| {
        builder
            .with_providers(vec![Box::new(InventoryProvider)])
            .with_routing(Routing::default().web(|| {
                Route::get("/write", write_item)?;
                Ok(())
            }))
    })
    .await?;
    test.artisan(["migrate"]).await.assert_successful();
    test.artisan(["migrate", "--database=reports", "--seed"])
        .await
        .assert_successful();
    let primary = test.database().await?;
    primary
        .unprepared("CREATE VIEW migration_probe AS SELECT 1 AS n")
        .await?;
    test.run_database_migrations(
        DatabaseMigrationsOptions {
            drop_views: true,
            seed: true,
            seeder: Some("SecondSeeder".into()),
        },
        |test| {
            Box::pin(async {
                test.assert_database_has("items", &[("name", "second".into())])
                    .await?;
                test.assert_database_count("items", 1).await?;
                let db = test.database().await?;
                assert!(!db.get_schema_builder().has_view("migration_probe").await?);
                test.get("/write")
                    .await?
                    .assert_status(StatusCode::NO_CONTENT);
                assert_eq!(
                    tokio::spawn(async move { db.table("items").count("*").await }).await??,
                    2
                );

                let nested = test
                    .run_database_migrations(DatabaseMigrationsOptions::default(), |_| {
                        Box::pin(async { Ok(()) })
                    })
                    .await
                    .unwrap_err();
                assert!(nested.to_string().contains("still active"), "{nested}");
                Ok(())
            })
        },
    )
    .await?;
    assert!(!primary.get_schema_builder().has_table("items").await?);
    test.assert_database_count_on("items", 1, Some("reports"))
        .await?;

    let error = test
        .run_database_migrations(
            DatabaseMigrationsOptions {
                seed: true,
                ..Default::default()
            },
            |test| {
                Box::pin(async {
                    test.assert_database_count("items", 1).await?;
                    Err::<(), _>(std::io::Error::other("migration test callback failed").into())
                })
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "migration test callback failed");
    assert!(!primary.get_schema_builder().has_table("items").await?);

    let error = test
        .run_database_migrations::<()>(
            DatabaseMigrationsOptions {
                seeder: Some("UnknownSeeder".into()),
                ..Default::default()
            },
            |_| Box::pin(async { panic!("the callback ran after failed preparation") }),
        )
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("migrate:fresh exited with 1"),
        "{error}"
    );
    assert!(!primary.get_schema_builder().has_table("items").await?);
    use futures_util::FutureExt as _;
    let panicked = std::panic::AssertUnwindSafe(
        test.run_database_migrations::<()>(DatabaseMigrationsOptions::default(), |_| {
            Box::pin(async { panic!("original test panic") })
        }),
    )
    .catch_unwind()
    .await
    .unwrap_err();
    assert_eq!(
        panicked.downcast_ref::<&str>(),
        Some(&"original test panic")
    );
    assert!(!primary.get_schema_builder().has_table("items").await?);
    test.run_database_migrations(DatabaseMigrationsOptions::default(), |test| {
        Box::pin(async { test.assert_database_empty("items").await })
    })
    .await?;
    assert!(!primary.get_schema_builder().has_table("items").await?);
    test.close().await
}

#[tokio::test]
async fn mysql_database_migrations_closes_an_interrupted_test_after_rollback()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use openrust::database::events::{MigrationMethod, MigrationsEnded};

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut test = TestApp::mysql(&url, |builder| {
        builder.with_providers(vec![Box::new(InventoryProvider)])
    })
    .await?;
    let events = test.fake_events(&[MigrationsEnded::NAME]).await?;
    let entered = Arc::new(tokio::sync::Notify::new());
    let signal = entered.clone();
    let mut pending = Box::pin(test.run_database_migrations(
        DatabaseMigrationsOptions::default(),
        move |_| {
            Box::pin(async move {
                signal.notify_one();
                std::future::pending::<Result<(), Box<dyn Error + Send + Sync>>>().await
            })
        },
    ));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::select! {
            () = entered.notified() => {}
            result = &mut pending => panic!("test finished before cancellation: {result:?}"),
        }
    })
    .await?;
    drop(pending);
    events.assertDispatchedTimes(MigrationsEnded::NAME, 1);
    test.close().await?;
    let methods: Vec<_> = events
        .dispatched(MigrationsEnded::NAME)
        .iter()
        .map(|event| event.downcast_ref::<MigrationsEnded>().unwrap().method)
        .collect();
    assert_eq!(methods, [MigrationMethod::Up, MigrationMethod::Down]);
    Ok(())
}

#[tokio::test]
async fn mysql_test_seeders_follow_artisan_order_and_rollback_boundaries()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut app = TestApp::mysql_connections(&[("primary", &url), ("reports", &url)], |builder| {
        builder.with_providers(vec![Box::new(InventoryProvider)])
    })
    .await?;
    app.artisan(["migrate"]).await.assert_successful();
    app.artisan(["migrate", "--database=reports"])
        .await
        .assert_successful();

    app.seed().await?;
    app.seed_with(&[]).await?;
    app.seed_on("reports", Some(&[])).await?;
    app.assert_database_count("items", 1).await?;
    app.assert_database_empty_on("items", Some("reports"))
        .await?;

    app.database_transaction_on(&["primary", "reports"], |test| {
        Box::pin(async {
            test.seed_with(&["Database\\Seeders\\SecondSeeder", "FirstSeeder"])
                .await?;
            let order = test
                .database()
                .await?
                .select("SELECT name FROM items ORDER BY id", &[])
                .await?;
            assert_eq!(
                order
                    .iter()
                    .map(|row| row.get("name").unwrap())
                    .collect::<Vec<_>>(),
                vec![
                    &Value::from("seeded"),
                    &Value::from("second"),
                    &Value::from("first")
                ],
            );
            test.seed_on("reports", None).await?;
            let error = test
                .seed_on(
                    "reports",
                    Some(&["FirstSeeder", "UnknownSeeder", "SecondSeeder"]),
                )
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("db:seed UnknownSeeder exited with 1"),
                "{error}"
            );
            test.assert_database_has_on("items", &[("name", "first".into())], Some("reports"))
                .await?;
            test.assert_database_missing_on("items", &[("name", "second".into())], Some("reports"))
                .await?;
            test.assert_database_count_on("items", 2, Some("reports"))
                .await
        })
    })
    .await?;
    app.assert_database_count("items", 1).await?;
    app.assert_database_empty_on("items", Some("reports"))
        .await?;

    // With only the default connection in a test transaction, reports stays independent.
    app.database_transaction(|test| {
        Box::pin(async {
            test.seed_with(&["FirstSeeder"]).await?;
            test.seed_on("reports", Some(&["SecondSeeder"])).await?;
            Err::<(), _>(std::io::Error::other("stop the test callback").into())
        })
    })
    .await
    .unwrap_err();
    app.assert_database_missing("items", &[("name", "first".into())])
        .await?;
    app.assert_database_has_on("items", &[("name", "second".into())], Some("reports"))
        .await?;
    app.close().await
}

#[tokio::test]
async fn mysql_counts_queries_through_teardown_on_the_selected_connections()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };

    let mut app = TestApp::mysql_connections(&[("primary", &url), ("reports", &url)], |builder| {
        builder
            .with_providers(vec![Box::new(InventoryProvider)])
            .with_routing(Routing::default().web(|| {
                Route::get("/write", write_item)?;
                Ok(())
            }))
    })
    .await?;
    app.artisan(["migrate"]).await.assert_successful();
    app.artisan(["migrate", "--database=reports"])
        .await
        .assert_successful();
    let primary = app.database().await?;
    let manager = app.app().make::<DatabaseManager>().await?;
    let reader = manager.connection(Some("reports::read"))?;
    primary.select("SELECT 1", &[]).await?;
    assert!(
        app.expects_database_query_count(0, Some("unknown"))
            .await
            .is_err()
    );

    let mut isolated = TestApp::mysql(&url, |builder| builder).await?;
    isolated.expects_database_query_count(1, None).await?;

    let events = app.fake_events(&[QueryExecuted::NAME]).await?;
    app.expects_database_query_count(4, None).await?;
    app.expects_database_query_count(2, Some("primary")).await?;
    app.expects_database_query_count(1, Some("reports")).await?;
    app.expects_database_query_count(1, Some("reports::read"))
        .await?;
    app.get("/write")
        .await?
        .assert_status(StatusCode::NO_CONTENT);
    app.artisan(["db:seed", "--database=reports"])
        .await
        .assert_successful();
    reader.select("SELECT 1", &[]).await?;
    let preview = primary
        .pretend(|connection| {
            Box::pin(async {
                connection.select("SELECT 2", &[]).await?;
                Ok(())
            })
        })
        .await?;
    assert_eq!(preview.len(), 1);
    assert!(
        primary
            .select("SELECT * FROM absent_query_counter_table", &[])
            .await
            .is_err()
    );
    isolated.database().await?.select("SELECT 3", &[]).await?;
    events.assertDispatchedTimes(QueryExecuted::NAME, 4);
    app.close().await?;
    isolated.close().await
}

#[tokio::test]
async fn mysql_query_count_mismatch_still_cleans_up_the_fixture()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut fixture = TestApp::mysql(&url, |builder| builder).await?;
    let name = fixture.database().await?.get_database_name().to_owned();
    fixture.expects_database_query_count(3, None).await?;
    let error = fixture.close().await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "Expected 3 database queries on the [mysql] connection. 0 occurred."
    );
    let mut server = MySqlConnection::connect(&url).await?;
    assert!(!schema_exists(&mut server, &name).await?);

    let mut dropped = TestApp::mysql(&url, |builder| builder).await?;
    dropped.expects_database_query_count(0, None).await?;
    dropped.database().await?.select("SELECT 1", &[]).await?;
    let panic =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(dropped))).unwrap_err();
    assert_eq!(
        panic.downcast_ref::<String>().map(String::as_str),
        Some("Expected 0 database queries on the [mysql] connection. 1 occurred.")
    );
    Ok(())
}

#[tokio::test]
async fn mysql_database_assertions_select_connections_and_include_soft_deleted_models()
-> Result<(), Box<dyn Error + Send + Sync>> {
    use futures_util::FutureExt as _;
    use openrust::database::eloquent::{Model, ModelMetadata, ModelState};

    struct Report(ModelState);

    impl Model for Report {
        fn metadata() -> &'static ModelMetadata {
            static METADATA: ModelMetadata = ModelMetadata::new("items")
                .connection("reports")
                .primary_key("name")
                .soft_deletes("archived_at");
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

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut app = TestApp::mysql_connections(&[("primary", &url), ("reports", &url)], |builder| {
        builder.with_providers(vec![Box::new(InventoryProvider)])
    })
    .await?;
    app.artisan(["migrate"]).await.assert_successful();
    app.artisan(["migrate", "--database=reports"])
        .await
        .assert_successful();
    let primary = app.database().await?;
    let reports = app
        .app()
        .make::<DatabaseManager>()
        .await?
        .connection(Some("reports"))?;
    primary
        .get_schema_builder()
        .table("items", |table| {
            table.soft_deletes();
        })
        .await?;
    reports
        .get_schema_builder()
        .table("items", |table| {
            table.soft_deletes_with(Some("archived_at"), None);
        })
        .await?;

    app.assert_database_empty("items").await?;
    app.assert_database_empty_on("items", Some("reports"))
        .await?;
    let name = [("name", "report".into())];
    let row = Record::from([("name".into(), "report".into())]);
    reports
        .table("items")
        .insert(std::slice::from_ref(&row))
        .await?;
    primary.table("items").insert(&[row]).await?;
    app.assert_database_count_on("items", 1, Some("reports"))
        .await?;
    app.assert_database_has_on("items", &[("description", Value::Null)], Some("reports"))
        .await?;
    app.assert_database_missing_on("items", &[("name", "absent".into())], Some("reports"))
        .await?;
    app.assert_not_soft_deleted("items", &name).await?;

    let mut model = Report::from_state(ModelState::default());
    model.set_attribute("name", "report");
    app.assert_model_exists(&model).await?;
    app.assert_not_soft_deleted_model(&model).await?;
    reports
        .table("items")
        .r#where("name", "=", "report")
        .update(Record::from([(
            "archived_at".into(),
            "2026-11-17 10:00:00".into(),
        )]))
        .await?;
    app.assert_soft_deleted_model(&model).await?;
    app.assert_model_exists(&model).await?;
    app.assert_not_soft_deleted("items", &name).await?;
    primary
        .table("items")
        .r#where("name", "=", "report")
        .update(Record::from([(
            "deleted_at".into(),
            "2026-11-17 10:00:00".into(),
        )]))
        .await?;
    app.assert_soft_deleted("items", &name).await?;

    reports
        .table("items")
        .r#where("name", "=", "report")
        .delete()
        .await?;
    app.assert_model_missing(&model).await?;
    app.assert_database_empty_on("items", Some("reports"))
        .await?;
    assert!(
        std::panic::AssertUnwindSafe(app.assert_not_soft_deleted_model(&model))
            .catch_unwind()
            .await
            .is_err()
    );
    app.assert_database_count("items", 1).await?;
    app.close().await
}

#[tokio::test]
async fn mysql_refresh_rolls_back_every_selected_connection()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut app = TestApp::mysql_connections(&[("primary", &url), ("reports", &url)], |builder| {
        builder
            .with_providers(vec![Box::new(InventoryProvider)])
            .with_routing(Routing::default().web(|| {
                Route::get("/write", write_item)?;
                Ok(())
            }))
    })
    .await?;
    app.artisan(["migrate", "--database=reports", "--seed"])
        .await
        .assert_successful();
    let options = RefreshDatabaseOptions {
        seed: true,
        connections_to_transact: vec!["primary".into(), "reports".into()],
        ..Default::default()
    };
    let manager = app.app().make::<DatabaseManager>().await?;
    let primary = manager.connection(None)?;
    let reports = manager.connection(Some("reports"))?;
    assert_ne!(primary.get_database_name(), reports.get_database_name());
    app.refresh_database(options, |app| {
        Box::pin(async {
            app.get("/write")
                .await?
                .assert_status(StatusCode::NO_CONTENT);
            app.artisan(["db:seed", "--database=reports"])
                .await
                .assert_successful();
            app.assert_database_count("items", 2).await?;
            let manager = app.app().make::<DatabaseManager>().await?;
            assert_eq!(
                manager
                    .connection(Some("reports::read"))?
                    .table("items")
                    .count("*")
                    .await?,
                2
            );
            Ok(())
        })
    })
    .await?;
    assert_eq!(primary.table("items").count("*").await?, 1);
    assert_eq!(reports.table("items").count("*").await?, 1);

    app.database_transaction_on::<()>(&["primary", "primary::read"], |_| {
        Box::pin(async { panic!("duplicate connection selection must not run the callback") })
    })
    .await
    .unwrap_err();

    let failure = app
        .database_transaction_on(&["primary", "reports"], |app| {
            Box::pin(async {
                app.get("/write")
                    .await?
                    .assert_status(StatusCode::NO_CONTENT);
                let manager = app.app().make::<DatabaseManager>().await?;
                let reports = manager.connection(Some("reports"))?;
                drop(reports.begin_transaction().await?);
                Ok(())
            })
        })
        .await
        .unwrap_err();
    assert!(failure.to_string().contains("transaction"));
    assert_eq!(primary.table("items").count("*").await?, 1);
    assert_eq!(reports.table("items").count("*").await?, 1);

    app.database_transaction_on(&["reports"], |app| {
        Box::pin(async {
            app.get("/write")
                .await?
                .assert_status(StatusCode::NO_CONTENT);
            app.artisan(["db:seed", "--database=reports"])
                .await
                .assert_successful();
            Ok(())
        })
    })
    .await?;
    assert_eq!(primary.table("items").count("*").await?, 2);
    assert_eq!(reports.table("items").count("*").await?, 1);
    app.close().await?;
    Ok(())
}

#[tokio::test]
async fn mysql_multi_connection_setup_failure_unwinds_opened_sessions()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut app =
        TestApp::mysql_connections(&[("unavailable", &url), ("primary", &url)], |builder| {
            builder
        })
        .await?;
    app.boot().await?;
    let manager = app.app().make::<DatabaseManager>().await?;
    let name = manager
        .connection(Some("unavailable"))?
        .get_database_name()
        .to_owned();
    let mut server = MySqlConnection::connect(&url).await?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP PROCEDURE `{name}`.`openrust_fixture_owner`"
    )))
    .execute(&mut server)
    .await?;
    assert!(
        app.database_transaction_on::<()>(&["primary", "unavailable"], |_| {
            Box::pin(async { panic!("partial transaction setup must not run the callback") })
        })
        .await
        .is_err()
    );
    let primary = manager.connection(Some("primary"))?;
    primary.select("SELECT 1", &[]).await?;
    let primary_name = primary.get_database_name().to_owned();
    assert!(
        app.close()
            .await
            .unwrap_err()
            .to_string()
            .contains("ownership marker")
    );
    assert!(!schema_exists(&mut server, &primary_name).await?);
    assert!(schema_exists(&mut server, &name).await?);
    Ok(())
}

#[tokio::test]
async fn mysql_refresh_database_prepares_once_and_retries_after_interruption()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return Ok(());
    };
    let mut test = TestApp::mysql(&url, |builder| {
        builder
            .with_providers(vec![Box::new(InventoryProvider)])
            .with_routing(Routing::default().web(|| {
                Route::get("/write", write_item)?;
                Ok(())
            }))
    })
    .await?;
    let options = RefreshDatabaseOptions {
        seed: true,
        seeder: Some("RefreshSeeder".into()),
        ..Default::default()
    };
    let failure = test
        .refresh_database::<()>(options.clone(), |_| {
            Box::pin(async { panic!("test callback ran before the baseline was prepared") })
        })
        .await
        .unwrap_err();
    assert!(
        failure
            .to_string()
            .contains("intentional refresh seeder failure"),
        "{failure}"
    );

    test.refresh_database(options.clone(), |test| {
        Box::pin(async {
            test.assert_database_count("items", 1).await?;
            test.get("/write")
                .await?
                .assert_status(StatusCode::NO_CONTENT);
            test.assert_database_count("items", 2).await
        })
    })
    .await?;
    let error = test
        .refresh_database(options.clone(), |test| {
            Box::pin(async {
                test.assert_database_count("items", 1).await?;
                test.get("/write")
                    .await?
                    .assert_status(StatusCode::NO_CONTENT);
                Err::<(), Box<dyn Error + Send + Sync>>("test callback error".into())
            })
        })
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "test callback error");
    assert_eq!(REFRESH_RUNS.load(Ordering::Relaxed), 2);
    test.assert_database_count("items", 1).await?;

    let disrupted = test
        .refresh_database(options.clone(), |test| {
            Box::pin(async {
                let connection = test.database().await?;
                let abandoned = connection.begin_transaction().await?;
                drop(abandoned);
                Ok(())
            })
        })
        .await
        .unwrap_err();
    assert!(disrupted.to_string().contains("transaction"));
    test.refresh_database(options.clone(), |test| {
        Box::pin(async { test.assert_database_count("items", 1).await })
    })
    .await?;
    assert_eq!(REFRESH_RUNS.load(Ordering::Relaxed), 3);

    test.database()
        .await?
        .statement("CREATE VIEW refresh_probe AS SELECT id FROM items", &[])
        .await?;
    test.refresh_database(
        RefreshDatabaseOptions {
            drop_views: true,
            ..options
        },
        |test| {
            Box::pin(async {
                test.assert_database_count("items", 1).await?;
                assert!(
                    test.database()
                        .await?
                        .table("refresh_probe")
                        .count("*")
                        .await
                        .is_err()
                );
                Ok(())
            })
        },
    )
    .await?;
    assert_eq!(REFRESH_RUNS.load(Ordering::Relaxed), 4);

    let options = RefreshDatabaseOptions {
        seed: true,
        seeder: Some("RefreshSeeder".into()),
        ..Default::default()
    };
    PAUSE_REFRESH_SEEDER.store(true, Ordering::Relaxed);
    {
        let pending = test.refresh_database::<()>(options.clone(), |_| {
            Box::pin(async { panic!("cancelled test callback should never run") })
        });
        tokio::pin!(pending);
        tokio::select! {
            result = &mut pending => panic!("preparation returned before cancellation: {result:?}"),
            () = REFRESH_SEEDER_STARTED.notified() => {}
            () = tokio::time::sleep(std::time::Duration::from_secs(10)) => panic!("seeder did not start"),
        }
    }
    RESUME_REFRESH_SEEDER.notify_one();
    test.refresh_database(options, |test| {
        Box::pin(async { test.assert_database_count("items", 1).await })
    })
    .await?;
    assert_eq!(REFRESH_RUNS.load(Ordering::Relaxed), 5);

    test.refresh_database(
        RefreshDatabaseOptions {
            seed: true,
            ..Default::default()
        },
        |test| Box::pin(async { test.assert_database_count("items", 1).await }),
    )
    .await?;
    test.close().await?;
    Ok(())
}
