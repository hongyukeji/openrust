#![allow(
    dead_code,
    reason = "Fixtures shared by independent integration test binaries"
)]
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use openrust::{
    config::Repository,
    contracts::config::Repository as Config,
    database::{
        Connection, DatabaseManager,
        migrations::{MigrationEntry, MigrationRegistry, Migrator, RunOptions},
    },
};
use openrust_settings::{
    CreateSettingsTable, DatabaseSettingsRepository, Settings, SettingsError as Error,
    SettingsGroup, SettingsResult,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Serialize, Deserialize)]
pub struct Site {
    pub name: String,
    pub enabled: bool,
    pub count: u64,
    pub description: Option<String>,
}
impl SettingsGroup for Site {
    fn group() -> &'static str {
        "site"
    }
    fn defaults(config: &dyn Config) -> SettingsResult<Self> {
        Ok(Self {
            name: config
                .get("app.name")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or("Default".into()),
            enabled: true,
            count: 5,
            description: Some("Description".into()),
        })
    }
    fn validate(&self) -> SettingsResult<()> {
        if self.name.len() > 300 || self.count > 100 {
            return Err(Error::InvalidValue);
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct DeliverySettings {
    pub enabled: bool,
}
impl SettingsGroup for DeliverySettings {
    fn group() -> &'static str {
        "module_delivery_options"
    }
    fn defaults(_: &dyn Config) -> SettingsResult<Self> {
        Ok(Self { enabled: true })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Secrets {
    pub credentials: serde_json::Value,
}
impl SettingsGroup for Secrets {
    fn group() -> &'static str {
        "module_delivery"
    }
    fn defaults(_: &dyn Config) -> SettingsResult<Self> {
        Ok(Self {
            credentials: json!({"key":""}),
        })
    }
    fn encrypted() -> &'static [&'static str] {
        &["credentials"]
    }
}

pub struct Fixture {
    pub connection: Connection,
    pub repository: Arc<DatabaseSettingsRepository>,
    pub config: Repository,
    pub migrator: Migrator,
    pub table: String,
    ledger: String,
    _root: tempfile::TempDir,
}
impl Fixture {
    pub async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::File::create(root.path().join("test.sqlite")).unwrap();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let table = format!(
            "settings_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        let driver = std::env::var("SETTINGS_TEST_DRIVER").unwrap_or("sqlite".into());
        let db_config = match driver.as_str() {
            "sqlite" => {
                json!({"driver":"sqlite","database":root.path().join("test.sqlite"),"busy_timeout":10000,"max_connections":4})
            }
            "pgsql" => {
                assert_eq!(
                    std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                    Ok("1")
                );
                assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
                json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").expect("isolated PostgreSQL required"),"max_connections":4})
            }
            "mysql" => {
                assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
                assert!(std::env::var("OPENRUST_ACCEPTANCE_MYSQL_CONTAINER").is_ok());
                json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").expect("isolated MySQL required"),"max_connections":4})
            }
            _ => panic!("unsupported test driver"),
        };
        let config = Repository::new(BTreeMap::from([
            (
                "database".into(),
                json!({"default":"testing","connections":{"testing":db_config}}),
            ),
            ("app".into(), json!({"name":"Default"})),
        ]));
        let manager = Arc::new(DatabaseManager::new(
            Arc::new(Repository::new(config.all())),
            root.path(),
        ));
        let connection = manager.connection(None).unwrap();
        let name: &'static str = Box::leak(table.clone().into_boxed_str());
        let migration = Box::leak(Box::new(CreateSettingsTable::new(name)));
        let entries = Box::leak(Box::new([MigrationEntry::new(
            "database/migrations/2026_11_21_000000_create_settings_table.rs",
            migration,
        )]));
        let ledger = format!("{table}_migrations");
        let migrator = Migrator::new(
            manager,
            Arc::new(MigrationRegistry(entries)),
            ledger.clone(),
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
        let repository = Arc::new(
            DatabaseSettingsRepository::new(connection.clone())
                .withTable(&table)
                .unwrap(),
        );
        Self {
            connection,
            repository,
            config,
            migrator,
            table,
            ledger,
            _root: root,
        }
    }
    pub async fn site(&self) -> Settings<Site> {
        Settings::load(self.repository.clone(), &self.config, None)
            .await
            .unwrap()
    }
    pub async fn close(self) {
        self.connection
            .get_schema_builder()
            .drop_if_exists(&self.table)
            .await
            .unwrap();
        self.connection
            .get_schema_builder()
            .drop_if_exists(&self.ledger)
            .await
            .unwrap();
        self.connection.disconnect();
    }
}
