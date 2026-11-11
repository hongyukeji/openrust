use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use openrust::{
    config::{ConfigError, Environment, Repository},
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
        migrations::{Migration, MigrationContext, MigrationEntry, MigrationRegistry},
        query::Record,
        seeder::{Seeder, SeederContext, SeederEntry, SeederRegistry},
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::DB},
};
use serde_json::{Value as Json, json};

struct Create;
impl Migration for Create {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            context
                .getSchemaBuilder()
                .create("seed_items", |table| {
                    table.id();
                    table.string("name", 60);
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async { context.getSchemaBuilder().dropIfExists("seed_items").await })
    }
}

struct DatabaseSeeder;
impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            context.call(&["FirstSeeder", "SecondSeeder"]).await?;
            context
                .callOnce(&["Database\\Seeders\\FirstSeeder"])
                .await?;
            context.callSilent(&["SilentSeeder"]).await?;
            context
                .callWith(
                    "ParameterSeeder",
                    Record::from([("name".into(), "parameter".into())]),
                )
                .await
        })
    }
}

struct Insert(&'static str);
impl Seeder for Insert {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            let name = context
                .parameters()
                .get("name")
                .cloned()
                .unwrap_or(self.0.into());
            assert_eq!(
                DB::connection(None)?.getName(),
                context.connection().getName()
            );
            DB::table("seed_items")?
                .insert(&[Record::from([("name".into(), name)])])
                .await?;
            Ok(())
        })
    }
}

struct Failing;
impl Seeder for Failing {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            assert_eq!(
                DB::connection(None)?.getName(),
                context.connection().getName()
            );
            Err(DatabaseError::InvalidSeeder("fixture failure"))
        })
    }
}

struct Recursive;
impl Seeder for Recursive {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.call(&["RecursiveSeeder"]))
    }
}

struct Provider;
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
                "database/migrations/0001_create_seed_items.rs",
                &Create,
            )];
            static SEEDERS: &[SeederEntry] = &[
                SeederEntry::new("DatabaseSeeder", &DatabaseSeeder),
                SeederEntry::new("FirstSeeder", &Insert("first")),
                SeederEntry::new("SecondSeeder", &Insert("second")),
                SeederEntry::new("SilentSeeder", &Insert("silent")),
                SeederEntry::new("ParameterSeeder", &Insert("fallback")),
                SeederEntry::new("FailingSeeder", &Failing),
                SeederEntry::new("RecursiveSeeder", &Recursive),
            ];
            app.container()
                .instance(Arc::new(MigrationRegistry(MIGRATIONS)))?;
            app.container()
                .instance(Arc::new(SeederRegistry(SEEDERS)))?;
            Ok(())
        })
    }
}

fn load(_: &Path, _: &Environment) -> Result<BTreeMap<String, Json>, ConfigError> {
    let alternate = std::env::var("SEED_FIXTURE_MYSQL").map_or_else(
        |_| json!({"driver":"sqlite","database":"database/alternate.sqlite"}),
        |url| json!({"url":url}),
    );
    let environment = std::env::var("SEED_FIXTURE_ENV").unwrap_or_else(|_| "local".into());
    Ok(BTreeMap::from([
        ("app".into(), json!({"env":environment})),
        (
            "database".into(),
            json!({"default":"sqlite","migrations":{"table":"seed_migrations"},"connections":{
                "sqlite":{"driver":"sqlite","database":"database/database.sqlite"},
                "alternate":alternate
            }}),
        ),
    ]))
}

#[tokio::test]
async fn seeder_command_fixture() {
    let Some(root) = std::env::var_os("SEED_FIXTURE_ROOT") else {
        return;
    };
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("SEED_FIXTURE_ARGS").unwrap()).unwrap();
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let mut app = Application::configure(root.into())
        .withConfig(load)
        .withProviders(vec![Box::new(Provider)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().get::<DatabaseManager>("db").unwrap();
    let status = app.handleCommand(&args).await.unwrap();
    assert_eq!(manager.getDefaultConnection().unwrap(), "sqlite");
    std::process::exit(status);
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "openrust-seeding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("database")).unwrap();
        for file in ["database.sqlite", "alternate.sqlite"] {
            fs::File::create(root.join("database").join(file)).unwrap();
        }
        Self(root)
    }
    fn fixture(&self, environment: &str, mysql: Option<&str>, args: &[&str]) -> Output {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "seeder_command_fixture", "--nocapture"])
            .env("SEED_FIXTURE_ROOT", &self.0)
            .env("SEED_FIXTURE_ENV", environment)
            .env("SEED_FIXTURE_ARGS", serde_json::to_string(args).unwrap());
        if let Some(url) = mysql {
            command.env("SEED_FIXTURE_MYSQL", url);
        } else {
            command.env_remove("SEED_FIXTURE_MYSQL");
        }
        command.output().unwrap()
    }
    fn connection(&self, mysql: Option<&str>, file: &str) -> Connection {
        let config = mysql.map_or_else(
            || json!({"driver":"sqlite","database":file}),
            |url| json!({"url":url}),
        );
        let manager = DatabaseManager::new(
            Arc::new(Repository::new(BTreeMap::from([(
                "database".into(),
                json!({"default":"verify","connections":{"verify":config}}),
            )]))),
            &self.0,
        );
        manager.connection(None).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

async fn verify(mysql: Option<&str>, allow_fresh: bool) {
    let root = Root::new();
    let db = root.connection(mysql, "database/alternate.sqlite");
    for table in ["seed_items", "seed_migrations"] {
        db.getSchemaBuilder().dropIfExists(table).await.unwrap();
    }
    let run = |args: &[&str]| root.fixture("local", mysql, args);
    let pretend = success(run(&[
        "migrate",
        "--database=alternate",
        "--pretend",
        "--seed",
    ]));
    assert!(!pretend.contains("Seeding database"));
    assert!(!db.getSchemaBuilder().hasTable("seed_items").await.unwrap());
    let seeded = success(run(&["migrate", "--database=alternate", "--seed"]));
    assert!(seeded.contains("FirstSeeder DONE") && seeded.contains("ParameterSeeder DONE"));
    assert!(!seeded.contains("SilentSeeder"));
    let rows = || db.table("seed_items").orderBy("id", "asc");
    assert_eq!(
        rows().pluck("name").await.unwrap(),
        ["first", "second", "silent", "parameter"].map(Value::from)
    );
    let default = root.connection(None, "database/database.sqlite");
    assert!(
        !default
            .getSchemaBuilder()
            .hasTable("seed_items")
            .await
            .unwrap()
    );

    let refused = root.fixture(
        "production",
        mysql,
        &["db:seed", "--database=alternate", "--no-interaction"],
    );
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(rows().count("*").await.unwrap(), 4);
    success(root.fixture(
        "production",
        mysql,
        &[
            "db:seed",
            "FirstSeeder",
            "--class=UnknownSeeder",
            "--database=alternate",
            "--force",
        ],
    ));
    assert_eq!(rows().count("*").await.unwrap(), 5);

    for (name, expected) in [
        ("UnknownSeeder", "not registered"),
        ("FailingSeeder", "fixture failure"),
        ("RecursiveSeeder", "recursive call"),
    ] {
        let output = run(&["db:seed", "--database=alternate", "--class", name]);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }

    success(run(&[
        "migrate:refresh",
        "--database=alternate",
        "--seeder=Database\\Seeders\\SecondSeeder",
    ]));
    assert_eq!(rows().pluck("name").await.unwrap(), ["second".into()]);
    if allow_fresh {
        success(run(&[
            "migrate:fresh",
            "--database=alternate",
            "--seed",
            "--seeder=FirstSeeder",
        ]));
        assert_eq!(rows().pluck("name").await.unwrap(), ["first".into()]);
    }
    for table in ["seed_items", "seed_migrations"] {
        db.getSchemaBuilder().drop(table).await.unwrap();
    }
}

#[tokio::test]
async fn artisan_seeders_use_the_selected_sqlite_database() {
    verify(None, true).await;
}

#[tokio::test]
async fn artisan_seeders_write_to_real_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify(Some(&url), false).await;
}

#[tokio::test]
async fn mysql_fresh_seeds_the_selected_database() {
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_WIPE_URL") {
        verify(Some(&url), true).await;
    }
}

#[test]
fn seed_generator_preserves_existing_files_until_forced() {
    let root = Root::new();
    let command = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .env("APP_BASE_PATH", &root.0)
            .env("APP_ENV", "local")
            .env("DB_CONNECTION", "sqlite")
            .env_remove("DB_URL")
            .args(["artisan"])
            .args(args)
            .output()
            .unwrap()
    };
    success(command(&["db:seed"]));
    let args = ["make:seeder", "Admin/APIUserSeeder"];
    assert!(success(command(&args)).contains("database/seeders/admin/api_user_seeder.rs"));
    let file = root.0.join("database/seeders/admin/api_user_seeder.rs");
    let generated = fs::read_to_string(&file).unwrap();
    fs::write(&file, "// existing\n").unwrap();
    assert_eq!(command(&args).status.code(), Some(1));
    assert_eq!(fs::read_to_string(&file).unwrap(), "// existing\n");
    success(command(&["make:seeder", "Admin/APIUserSeeder", "--force"]));
    assert_eq!(fs::read_to_string(file).unwrap(), generated);
    assert_eq!(
        command(&["make:seeder", "../Outside"]).status.code(),
        Some(1)
    );
}
