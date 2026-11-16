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
                        assert!(e.migration.getConnection().is_none());
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
            .withConfig(config)
            .withMigrations(ENTRIES)
            .withProviders(vec![Box::new(Observer(received.clone()))])
            .create()
            .unwrap();
        let arguments: Vec<_> = args.iter().map(std::ffi::OsString::from).collect();
        async move { application.handleCommand(&arguments).await.unwrap() }
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
