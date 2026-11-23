use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use openrust::{
    config::Repository,
    database::{
        DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult,
        migrations::{
            Migration, MigrationContext, MigrationEntry, MigrationRegistry, Migrator,
            RollbackOptions, RunOptions,
        },
    },
};
use serde_json::json;

struct Definition {
    name: &'static str,
    fail: AtomicBool,
    skip: bool,
}

impl Migration for Definition {
    fn shouldRun<'a>(
        &'a self,
        _: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async { Ok(!self.skip) })
    }

    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            if self.fail.swap(false, Ordering::SeqCst) {
                return Err(DatabaseError::InvalidMigration("test failure"));
            }
            connection
                .getSchemaBuilder()
                .create(self.name, |table| {
                    table.id();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async { connection.getSchemaBuilder().drop(self.name).await })
    }
}

static FIRST: Definition = Definition {
    name: "first",
    fail: AtomicBool::new(false),
    skip: false,
};
static SECOND: Definition = Definition {
    name: "second",
    fail: AtomicBool::new(true),
    skip: false,
};
static THIRD: Definition = Definition {
    name: "third",
    fail: AtomicBool::new(false),
    skip: false,
};
static SKIP: Definition = Definition {
    name: "skipped",
    fail: AtomicBool::new(false),
    skip: true,
};
static ENTRIES: &[MigrationEntry] = &[
    MigrationEntry::new("database/migrations/0002_second.rs", &SECOND),
    MigrationEntry::new("database/migrations/0001_first.rs", &FIRST),
    MigrationEntry::new("database/migrations/0003_third.rs", &THIRD),
    MigrationEntry::new("database/migrations/0004_skipped.rs", &SKIP),
];

struct TransactionProbe {
    table: &'static str,
    transactional: bool,
    fail_up: AtomicBool,
    fail_down: AtomicBool,
}

impl Migration for TransactionProbe {
    fn withinTransaction(&self) -> bool {
        self.transactional
    }

    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .getSchemaBuilder()
                .create(self.table, |table| {
                    table.integer("id");
                })
                .await?;
            context
                .statement(
                    &format!("insert into {} (id) values (?)", self.table),
                    &[7.into()],
                )
                .await?;
            if self.fail_up.swap(false, Ordering::SeqCst) {
                return Err(DatabaseError::InvalidMigration(
                    "transaction probe up failure",
                ));
            }
            Ok(())
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context.getSchemaBuilder().drop(self.table).await?;
            if self.fail_down.swap(false, Ordering::SeqCst) {
                return Err(DatabaseError::InvalidMigration(
                    "transaction probe down failure",
                ));
            }
            Ok(())
        })
    }
}

static PG_ATOMIC: TransactionProbe = TransactionProbe {
    table: "migration_atomic_probe",
    transactional: true,
    fail_up: AtomicBool::new(true),
    fail_down: AtomicBool::new(true),
};
static PG_AUTOCOMMIT: TransactionProbe = TransactionProbe {
    table: "migration_autocommit_probe",
    transactional: false,
    fail_up: AtomicBool::new(true),
    fail_down: AtomicBool::new(false),
};
static PG_ENTRIES: &[MigrationEntry] = &[
    MigrationEntry::new("database/migrations/0001_atomic.rs", &PG_ATOMIC),
    MigrationEntry::new("database/migrations/0002_autocommit.rs", &PG_AUTOCOMMIT),
];

#[tokio::test]
async fn postgres_migration_transactions_roll_back_schema_and_data_before_updating_the_ledger() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MIGRATION_POSTGRES_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MIGRATION_POSTGRES").is_none());
        return;
    };
    assert_migration_transaction_rollback("pgsql", &url, PG_ENTRIES).await;
}

static SQL_SERVER_ATOMIC: TransactionProbe = TransactionProbe {
    table: "migration_atomic_probe",
    transactional: true,
    fail_up: AtomicBool::new(true),
    fail_down: AtomicBool::new(true),
};
static SQL_SERVER_AUTOCOMMIT: TransactionProbe = TransactionProbe {
    table: "migration_autocommit_probe",
    transactional: false,
    fail_up: AtomicBool::new(true),
    fail_down: AtomicBool::new(false),
};
static SQL_SERVER_ENTRIES: &[MigrationEntry] = &[
    MigrationEntry::new("database/migrations/0001_atomic.rs", &SQL_SERVER_ATOMIC),
    MigrationEntry::new(
        "database/migrations/0002_autocommit.rs",
        &SQL_SERVER_AUTOCOMMIT,
    ),
];

#[tokio::test]
async fn sql_server_migration_transactions_roll_back_schema_and_data_before_updating_the_ledger() {
    let Ok(url) = std::env::var("OPENRUST_TEST_SQL_SERVER_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_SQL_SERVER").is_none());
        return;
    };
    assert_migration_transaction_rollback("sqlsrv", &url, SQL_SERVER_ENTRIES).await;
}

async fn assert_migration_transaction_rollback(
    connection: &str,
    url: &str,
    entries: &'static [MigrationEntry],
) {
    let root = tempfile::tempdir().unwrap();
    let manager = Arc::new(DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":connection,"connections":{(connection):{"url":url}}}),
        )]))),
        root.path(),
    ));
    let db = manager.connection(None).unwrap();
    let migrator = Migrator::new(
        manager,
        Arc::new(MigrationRegistry(entries)),
        "transaction_history".into(),
        root.path().to_path_buf(),
    );
    let mut schema = db.getSchemaBuilder();
    let atomic = [root.path().join(entries[0].path)];
    let autocommit = [root.path().join(entries[1].path)];

    let failed_up = migrator
        .run(None, &atomic, RunOptions::default())
        .await
        .err()
        .expect("atomic migration must fail");
    assert!(
        matches!(failed_up, DatabaseError::MigrationFailed { .. }),
        "{failed_up:?}"
    );
    assert!(!schema.hasTable("migration_atomic_probe").await.unwrap());
    assert_eq!(db.table("transaction_history").count("*").await.unwrap(), 0);
    migrator
        .run(None, &atomic, RunOptions::default())
        .await
        .unwrap();
    assert_eq!(
        db.table("migration_atomic_probe").count("*").await.unwrap(),
        1
    );

    assert!(matches!(
        migrator
            .rollback(None, &atomic, RollbackOptions::default())
            .await,
        Err(DatabaseError::MigrationFailed { .. })
    ));
    assert_eq!(
        db.table("migration_atomic_probe").count("*").await.unwrap(),
        1
    );
    assert_eq!(db.table("transaction_history").count("*").await.unwrap(), 1);
    migrator
        .rollback(None, &atomic, RollbackOptions::default())
        .await
        .unwrap();
    assert!(!schema.hasTable("migration_atomic_probe").await.unwrap());

    assert!(matches!(
        migrator.run(None, &autocommit, RunOptions::default()).await,
        Err(DatabaseError::MigrationFailed { .. })
    ));
    assert_eq!(
        db.table("migration_autocommit_probe")
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert_eq!(db.table("transaction_history").count("*").await.unwrap(), 0);
    schema.drop("migration_autocommit_probe").await.unwrap();
    schema.drop("transaction_history").await.unwrap();
}

#[tokio::test]
async fn mysql_migration_repository_projects_history_and_selects_the_latest_batch_on_the_writer() {
    use openrust::database::{
        events::QueryRole,
        migrations::{DatabaseMigrationRepository, MigrationRepositoryInterface},
    };

    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let mut reader = url::Url::parse(&url).unwrap();
    reader.set_path("/openrust_missing_ledger_replica");
    let manager = Arc::new(DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"mysql","connections":{"mysql":{
                "url":url, "read":{"url":reader.as_str()}, "prefix":"repo_"
            }}}),
        )]))),
        ".",
    ));
    let connection = manager.connection(None).unwrap();
    assert!(connection.select("select 1", &[]).await.is_err());
    let mut repository = DatabaseMigrationRepository::new(manager, None, "history");
    repository.create_repository().await.unwrap();
    assert!(repository.get_last().await.unwrap().is_empty());
    for (migration, batch) in [("alpha", 2), ("older", 1), ("zeta", 2)] {
        repository.log(migration, batch).await.unwrap();
    }

    connection.enableQueryLog();
    let latest = repository.get_last().await.unwrap();
    assert_eq!(
        latest
            .iter()
            .map(|row| (row.migration.as_str(), row.batch))
            .collect::<Vec<_>>(),
        [("zeta", 2), ("alpha", 2)]
    );
    assert!(latest.iter().all(|row| row.id > 0));
    let history = repository.get_ran().await.unwrap();
    assert_eq!(history, ["older", "alpha", "zeta"]);
    let batches = repository.get_migration_batches().await.unwrap();
    assert_eq!(
        batches
            .iter()
            .map(|(name, batch)| (name.as_str(), *batch))
            .collect::<Vec<_>>(),
        [("older", 1), ("alpha", 2), ("zeta", 2)]
    );
    let log = connection.getQueryLog();
    assert_eq!(log.len(), 3);
    assert!(
        log.iter()
            .all(|entry| entry.read_write_type == QueryRole::Write)
    );
    assert!(log[0].query.contains("`batch` = (select max(batch)"));
    assert!(
        log[1]
            .query
            .starts_with("select `migration` from `repo_history`")
    );
    assert!(
        log[2]
            .query
            .starts_with("select `migration`, `batch` from `repo_history`")
    );
}

#[tokio::test]
async fn mysql_artisan_excludes_named_pending_migrations_per_application() {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        testing::TestApp,
    };

    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    }
    const FIRST_PATH: &str = "database/migrations/0001_create_first.rs";
    static EXCLUSIONS: &[MigrationEntry] = &[
        MigrationEntry::new(FIRST_PATH, &FIRST),
        MigrationEntry::new("database/migrations/0002_create_third.rs", &THIRD),
    ];
    let create = || {
        TestApp::create(|builder| {
            builder
                .with_config(|_, _| {
                    let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
                    Ok([(
                        "database".into(),
                        json!({
                            "default": "mysql",
                            "connections": {"mysql": {"url": url, "prefix": "excluded_"}},
                            "migrations": {"table": "history"}
                        }),
                    )]
                    .into())
                })
                .with_migrations(EXCLUSIONS)
        })
        .unwrap()
    };
    let mut app = create();
    app.boot().await.unwrap();
    let migrator = app.app().container().get::<Migrator>("migrator").unwrap();
    migrator.withoutMigrations(&[FIRST_PATH]);

    let planned = app.artisan(["migrate", "--pretend"]).await;
    planned.assertSuccessful();
    assert!(planned.stdout.contains("0002_create_third"));
    assert!(!planned.stdout.contains("0001_create_first"));
    let mut other = create();
    let independent = other
        .artisan(["migrate", "--pretend", "--path", FIRST_PATH])
        .await;
    independent.assertSuccessful();
    assert!(independent.stdout.contains("0001_create_first"));

    migrator.withoutMigrations(&["0001_create_first"]);
    let excluded = app.artisan(["migrate", "--path", FIRST_PATH]).await;
    excluded.assertSuccessful();
    assert!(excluded.stdout.contains("Nothing to migrate"));
    app.artisan(["migrate"])
        .await
        .assertSuccessful()
        .expectsOutput("0002_create_third DONE");
    app.artisan(["migrate:status"])
        .await
        .assertSuccessful()
        .expectsOutput("0001_create_first Pending")
        .expectsOutput("0002_create_third [1] Ran");
    migrator.withoutMigrations(&[]);
    app.artisan(["migrate", "--path", FIRST_PATH])
        .await
        .assertSuccessful()
        .expectsOutput("0001_create_first DONE");
    migrator.withoutMigrations(&["0001_create_first"]);
    app.artisan(["migrate:rollback", "--step=1"])
        .await
        .assertSuccessful()
        .expectsOutput("0001_create_first DONE");
    app.artisan(["migrate:status"])
        .await
        .assertSuccessful()
        .expectsOutput("0001_create_first Pending")
        .expectsOutput("0002_create_third [1] Ran");
}

#[tokio::test]
async fn mysql_artisan_status_distinguishes_initial_skips_from_pending() {
    use openrust::testing::TestApp;

    if std::env::var_os("OPENRUST_TEST_MYSQL_URL").is_none() {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    }
    const FIRST_PATH: &str = "database/migrations/0001_initial_skip.rs";
    const SECOND_PATH: &str = "database/migrations/0002_ready.rs";
    static STATUS_ENTRIES: &[MigrationEntry] = &[
        MigrationEntry::new(FIRST_PATH, &SKIP),
        MigrationEntry::new(SECOND_PATH, &FIRST),
        MigrationEntry::new("database/migrations/0003_later_skip.rs", &SKIP),
    ];
    let mut app = TestApp::create(|builder| {
        builder
            .with_config(|_, _| {
                let url = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
                Ok([(
                    "database".into(),
                    json!({
                        "default": "mysql",
                        "connections": {"mysql": {"url": url, "prefix": "status_"}},
                        "migrations": {"table": "history"}
                    }),
                )]
                .into())
            })
            .with_migrations(STATUS_ENTRIES)
    })
    .unwrap();

    app.artisan(["migrate:status", "--pending=17"])
        .await
        .assertExitCode(1);
    app.artisan(["migrate:install"]).await.assertExitCode(0);
    let status = app.artisan(["migrate:status"]).await;
    status
        .assertExitCode(0)
        .expectsOutput("0001_initial_skip Skipped")
        .expectsOutput("0002_ready Pending")
        .expectsOutput("0003_later_skip Pending");
    let pending = app.artisan(["migrate:status", "--pending=17"]).await;
    pending
        .assertExitCode(17)
        .expectsOutput("0002_ready Pending");
    assert!(!pending.stdout.contains("0001_initial_skip"));
    let filtered = app
        .artisan(["migrate:status", "--pending=17", "--path", FIRST_PATH])
        .await;
    filtered.assertExitCode(0);
    assert!(filtered.stdout.contains("No pending migrations"));
    app.artisan(["migrate", "--path", SECOND_PATH])
        .await
        .assertExitCode(0);
    app.artisan(["migrate:status"])
        .await
        .assertExitCode(0)
        .expectsOutput("0002_ready [1] Ran")
        .expectsOutput("0003_later_skip Skipped");
    let finished = app.artisan(["migrate:status", "--pending=17"]).await;
    finished.assertExitCode(0);
    assert!(finished.stdout.contains("No pending migrations"));
}

#[tokio::test]
async fn failed_migration_leaves_prior_batch_and_retry_is_serialized() {
    let manager = Arc::new(DatabaseManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"memory","connections":{"memory":{"driver":"sqlite","database":":memory:"}}}),
        )]))),
        ".",
    ));
    let db = manager.connection(None).unwrap();
    let migrator = Migrator::new(
        manager,
        Arc::new(MigrationRegistry(ENTRIES)),
        "history".into(),
        ".".into(),
    );
    assert!(
        matches!(migrator.run(None, &[], RunOptions::default()).await, Err(DatabaseError::MigrationFailed { migration, .. }) if migration == "0002_second")
    );
    let batch = || db.table("history").orderBy("migration", "asc");
    assert_eq!(
        batch().pluck("migration").await.unwrap(),
        ["0001_first".into()]
    );
    let options = || RunOptions {
        step: true,
        ..RunOptions::default()
    };
    let (one, two) = tokio::join!(
        migrator.run(None, &[], options()),
        migrator.run(None, &[], options())
    );
    let mut ran: Vec<_> = one
        .unwrap()
        .into_iter()
        .chain(two.unwrap())
        .filter(|result| result.name != "0004_skipped")
        .map(|result| result.name)
        .collect();
    ran.sort();
    assert_eq!(ran, ["0002_second", "0003_third"]);
    assert_eq!(
        batch().pluck("batch").await.unwrap(),
        [1.into(), 2.into(), 3.into()]
    );
    migrator
        .rollback(
            None,
            &[],
            RollbackOptions {
                batch: Some(2),
                ..RollbackOptions::default()
            },
        )
        .await
        .unwrap();
    assert!(!db.getSchemaBuilder().hasTable("second").await.unwrap());
    assert!(db.getSchemaBuilder().hasTable("third").await.unwrap());
    assert!(!db.getSchemaBuilder().hasTable("skipped").await.unwrap());
    assert_eq!(batch().pluck("batch").await.unwrap(), [1.into(), 3.into()]);
}

#[tokio::test]
async fn destructive_migration_prohibition_is_local_to_the_application() {
    use openrust::{
        foundation::Application,
        http::HttpFuture,
        support::{ProviderResult, ServiceProvider, facades::DB},
        testing::TestApp,
    };

    struct ProhibitDestructiveCommands;

    impl ServiceProvider for ProhibitDestructiveCommands {
        fn boot(&mut self, _: &Application) -> HttpFuture<'_, ProviderResult> {
            Box::pin(async {
                DB::prohibitDestructiveCommands(true)?;
                Ok(())
            })
        }
    }

    let create = |prohibit| {
        TestApp::create(|builder| {
            let builder = builder.with_config(|_, _| {
                Ok([(
                    "database".into(),
                    json!({
                        "default": "sqlite",
                        "connections": {"sqlite": {"driver": "sqlite", "database": ":memory:"}},
                    }),
                )]
                .into())
            });
            if prohibit {
                builder.with_providers(vec![Box::new(ProhibitDestructiveCommands)])
            } else {
                builder
            }
        })
        .unwrap()
    };

    let mut protected = create(true);
    protected
        .artisan(["migrate", "--force"])
        .await
        .assertSuccessful();
    let mut independent = create(false);
    independent
        .artisan(["migrate", "--force"])
        .await
        .assertSuccessful();
    for command in [
        "db:wipe",
        "migrate:fresh",
        "migrate:refresh",
        "migrate:reset",
        "migrate:rollback",
    ] {
        let result = protected.artisan([command, "--force"]).await;
        result.assertExitCode(1);
        assert!(result.stdout.contains("This command is prohibited"));
    }
    independent
        .artisan(["db:wipe", "--force"])
        .await
        .assertSuccessful();
    protected
        .artisan(["migrate:status"])
        .await
        .assertSuccessful();
}

#[tokio::test]
async fn mysql_artisan_dispatches_migration_and_schema_lifecycle_events() {
    use std::{path::Path, sync::Mutex};

    use openrust::{
        config::{ConfigError, Environment, Value},
        contracts::{
            database::events::MigrationEvent,
            events::{Event as EventPayload, Listener},
        },
        database::events::{
            DatabaseRefreshed, MigrationEnded, MigrationMethod, MigrationOptions, MigrationSkipped,
            MigrationStarted, MigrationsEnded, MigrationsPruned, MigrationsStarted,
            NoPendingMigrations, SchemaDumped, SchemaLoaded,
        },
        events::{Delivery, EventContext, EventFuture, EventResult, ListenerResponse},
        foundation::Application,
        http::HttpFuture,
        support::{
            ProviderResult, ServiceProvider,
            facades::{DB, Event},
        },
    };

    if std::env::var_os("OPENRUST_TEST_MIGRATION_EVENTS").is_none() {
        return;
    }

    static FIRST: Definition = Definition {
        name: "events_first",
        fail: AtomicBool::new(false),
        skip: false,
    };
    static SECOND: Definition = Definition {
        name: "events_second",
        fail: AtomicBool::new(true),
        skip: false,
    };
    static SKIPPED: Definition = Definition {
        name: "events_skipped",
        fail: AtomicBool::new(false),
        skip: true,
    };
    const FIRST_PATH: &str = "database/migrations/0001_events_first.rs";
    static ENTRIES: &[MigrationEntry] = &[
        MigrationEntry::new(FIRST_PATH, &FIRST),
        MigrationEntry::new("database/migrations/0002_events_second.rs", &SECOND),
        MigrationEntry::new("database/migrations/0003_events_skipped.rs", &SKIPPED),
    ];

    fn config(_: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
        let mysql = std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap();
        Ok(BTreeMap::from([(
            "database".into(),
            json!({
                "default": "mysql", "connections": {"mysql":{"url":mysql}}, "migrations":{"table":"history"}
            }),
        )]))
    }

    #[derive(Clone)]
    struct Observer(Arc<Mutex<Vec<String>>>);

    impl ServiceProvider for Observer {
        fn boot(&mut self, _: &Application) -> HttpFuture<'_, ProviderResult> {
            Box::pin(async move {
                let listener = Arc::new(self.clone());
                for event in [
                    MigrationEvent::NAME,
                    SchemaDumped::NAME,
                    SchemaLoaded::NAME,
                    MigrationsPruned::NAME,
                ] {
                    Event::listen(event, listener.clone(), Delivery::Sync)?;
                }
                Ok(())
            })
        }
    }

    impl Listener for Observer {
        fn handle<'a>(
            &'a self,
            event: &'a dyn EventPayload,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let receipt = match event.name() {
                    MigrationsStarted::NAME => {
                        let e = event.downcast_ref::<MigrationsStarted>().unwrap();
                        let pretend = match e.options {
                            MigrationOptions::Up(options) => options.pretend,
                            MigrationOptions::Down(options) => options.pretend,
                        };
                        format!("batch:start:{:?}:{pretend}", e.method)
                    }
                    MigrationsEnded::NAME => {
                        let e = event.downcast_ref::<MigrationsEnded>().unwrap();
                        format!("batch:end:{:?}", e.method)
                    }
                    MigrationStarted::NAME => {
                        let e = event.downcast_ref::<MigrationStarted>().unwrap();
                        let config = <openrust::support::facades::Config as openrust::support::facades::Facade>::get_facade_root().unwrap();
                        assert!(
                            e.migration
                                .getConnection(config.as_ref())
                                .unwrap()
                                .is_none()
                        );
                        format!("start:{:?}:{}", e.method, e.name)
                    }
                    MigrationEnded::NAME => {
                        let e = event.downcast_ref::<MigrationEnded>().unwrap();
                        let db = DB::connection(None).unwrap();
                        let found = db
                            .table("history")
                            .r#where("migration", "=", e.name.as_str())
                            .exists()
                            .await
                            .unwrap();
                        assert_eq!(found, e.method == MigrationMethod::Down);
                        format!("end:{:?}:{}", e.method, e.name)
                    }
                    MigrationSkipped::NAME => {
                        let e = event.downcast_ref::<MigrationSkipped>().unwrap();
                        format!("skip:{}", e.migrationName)
                    }
                    NoPendingMigrations::NAME => {
                        let e = event.downcast_ref::<NoPendingMigrations>().unwrap();
                        format!("empty:{:?}", e.method)
                    }
                    DatabaseRefreshed::NAME => {
                        let e = event.downcast_ref::<DatabaseRefreshed>().unwrap();
                        let db = DB::connection(None).unwrap();
                        assert_eq!(db.table("history").count("*").await.unwrap(), 2);
                        format!(
                            "refresh:{}:{}",
                            e.database.as_deref().unwrap_or("default"),
                            e.seeding
                        )
                    }
                    SchemaDumped::NAME => {
                        let e = event.downcast_ref::<SchemaDumped>().unwrap();
                        assert_eq!(e.connection.getName(), "mysql");
                        assert!(e.path.is_file());
                        format!("dump:{}", e.connectionName)
                    }
                    SchemaLoaded::NAME => {
                        let e = event.downcast_ref::<SchemaLoaded>().unwrap();
                        assert_eq!(e.connection.table("history").count("*").await.unwrap(), 2);
                        assert!(e.path.is_file());
                        format!("load:{}", e.connectionName)
                    }
                    MigrationsPruned::NAME => {
                        let e = event.downcast_ref::<MigrationsPruned>().unwrap();
                        assert!(e.path.join("mod.rs").is_file());
                        assert!(!e.path.join("0001_events_first.rs").exists());
                        assert_eq!(e.connection.getName(), "mysql");
                        format!("prune:{}", e.connectionName)
                    }
                    name => panic!("unexpected event {name}"),
                };
                self.0.lock().unwrap().push(receipt);
                Ok(None)
            })
        }
    }

    let root = tempfile::tempdir().unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let run = |args: &[&str]| {
        let application = Application::configure(root.path().to_owned())
            .with_config(config)
            .with_migrations(ENTRIES)
            .with_providers(vec![Box::new(Observer(received.clone()))])
            .create()
            .unwrap();
        let arguments: Vec<_> = args.iter().map(std::ffi::OsString::from).collect();
        async move { application.handle_command(&arguments).await.unwrap() }
    };
    let take = || std::mem::take(&mut *received.lock().unwrap());

    assert_eq!(
        run(&["migrate", "--pretend", "--path", FIRST_PATH]).await,
        0
    );
    assert_eq!(take(), ["batch:start:Up:true", "batch:end:Up"]);
    assert_eq!(run(&["migrate"]).await, 1);
    assert_eq!(
        take(),
        [
            "batch:start:Up:false",
            "start:Up:0001_events_first",
            "end:Up:0001_events_first",
            "start:Up:0002_events_second",
        ]
    );
    assert_eq!(run(&["migrate"]).await, 0);
    assert_eq!(
        take(),
        [
            "batch:start:Up:false",
            "start:Up:0002_events_second",
            "end:Up:0002_events_second",
            "skip:0003_events_skipped",
            "batch:end:Up",
        ]
    );
    assert_eq!(run(&["migrate", "--path", FIRST_PATH]).await, 0);
    assert_eq!(take(), ["empty:Up"]);

    let migrations = root.path().join("database/migrations");
    std::fs::create_dir_all(&migrations).unwrap();
    std::fs::write(root.path().join(FIRST_PATH), "fixture").unwrap();
    std::fs::write(migrations.join("mod.rs"), "fixture").unwrap();
    assert_eq!(run(&["schema:dump", "--prune"]).await, 0);
    assert_eq!(take(), ["dump:mysql", "prune:mysql"]);
    assert_eq!(run(&["migrate:rollback", "--pretend"]).await, 0);
    assert_eq!(take(), ["batch:start:Down:true", "batch:end:Down"]);
    assert_eq!(run(&["migrate:rollback", "--step=2"]).await, 0);
    assert_eq!(
        take(),
        [
            "batch:start:Down:false",
            "start:Down:0002_events_second",
            "end:Down:0002_events_second",
            "start:Down:0001_events_first",
            "end:Down:0001_events_first",
            "batch:end:Down",
        ]
    );
    assert_eq!(run(&["migrate:rollback"]).await, 0);
    assert_eq!(take(), ["empty:Down"]);

    assert_eq!(run(&["migrate", "--path", FIRST_PATH]).await, 0);
    assert_eq!(take(), ["load:mysql", "empty:Up"]);
    assert_eq!(
        run(&["migrate:fresh", "--database=mysql", "--path", FIRST_PATH]).await,
        0
    );
    assert_eq!(take(), ["load:mysql", "empty:Up", "refresh:mysql:false"]);
    assert_eq!(
        run(&[
            "migrate:refresh",
            "--path",
            FIRST_PATH,
            "--seeder=MissingSeeder"
        ])
        .await,
        1
    );
    assert_eq!(take().last().unwrap(), "refresh:default:true");
}
