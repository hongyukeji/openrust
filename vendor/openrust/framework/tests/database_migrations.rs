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
