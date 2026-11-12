use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::Arc,
};

use openrust::{
    bus::DatabaseBatchRepository,
    config::Repository,
    database::{Connection, DatabaseManager},
};
use serde_json::json;

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-migrations-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("database")).unwrap();
        fs::File::create(path.join("database/database.sqlite")).unwrap();
        Self(path)
    }

    fn run(&self, environment: &str, mysql: Option<&str>, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
        command
            .current_dir(&self.0)
            .env("APP_BASE_PATH", &self.0)
            .env("APP_ENV", environment);
        if let Some(url) = mysql {
            command.env("DB_CONNECTION", "mysql").env("DB_URL", url);
        } else {
            command
                .env("DB_CONNECTION", "sqlite")
                .env_remove("DB_URL")
                .env("DB_DATABASE", self.0.join("database/database.sqlite"));
        }
        command.arg("artisan").args(args).output().unwrap()
    }

    fn connection(&self, mysql: Option<&str>) -> Connection {
        let config = mysql.map_or_else(
            || json!({"driver":"sqlite","database":"database/database.sqlite"}),
            |url| json!({"url":url}),
        );
        let manager = DatabaseManager::new(
            Arc::new(Repository::new(BTreeMap::from([(
                "database".into(),
                json!({"default":"verify","connections":{"verify":config}}),
            )]))),
            self.0.clone(),
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

async fn verify(mysql: Option<&str>) {
    let root = Root::new();
    let db = root.connection(mysql);
    let mut schema = db.getSchemaBuilder();
    for name in [
        "job_batches_continuations",
        "job_batches_additions",
        "job_batches_outbox",
        "job_batches_jobs",
        "job_batches",
        "failed_jobs",
        "jobs",
        "cache_locks",
        "cache",
        "sessions",
        "password_reset_tokens",
        "users",
        "migrations",
    ] {
        schema.dropIfExists(name).await.unwrap();
    }
    let run = |args: &[&str]| root.run("local", mysql, args);

    assert_eq!(run(&["migrate:status"]).status.code(), Some(1));
    let cancelled = root.run("production", mysql, &["migrate", "--no-interaction"]);
    assert_eq!(cancelled.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&cancelled.stdout).contains("cancelled"));
    assert!(!schema.hasTable("migrations").await.unwrap());

    let preview = success(run(&["migrate", "--pretend"]));
    assert!(preview.contains("create table") && preview.contains("users"));
    assert!(preview.contains("job_batches"));
    assert!(!schema.hasTable("users").await.unwrap());
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 0);
    assert!(success(run(&["migrate:status", "--pending"])).contains("Pending"));

    let path = "database/migrations/0001_01_01_000000_create_users_table.rs";
    let database = if mysql.is_some() { "mysql" } else { "sqlite" };
    assert!(
        success(root.run(
            "production",
            mysql,
            &["migrate", "--force", "--database", database, "--path", path]
        ))
        .contains("DONE")
    );
    assert!(success(run(&["migrate:status"])).contains("[1] Ran"));
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 1);
    assert!(schema.hasTable("users").await.unwrap());
    assert!(schema.hasTable("sessions").await.unwrap());
    assert!(success(run(&["migrate"])).contains("DONE"));
    assert!(schema.hasTable("cache").await.unwrap());
    assert!(schema.hasTable("cache_locks").await.unwrap());
    assert!(schema.hasTable("jobs").await.unwrap());
    assert!(schema.hasTable("failed_jobs").await.unwrap());
    assert!(schema.hasTable("job_batches").await.unwrap());
    verify_batches(db.clone()).await;
    assert!(success(run(&["migrate"])).contains("Nothing to migrate"));
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 9);
    assert!(
        success(run(&["migrate:rollback", "--pretend", "--batch", "1"])).contains("drop table")
    );
    assert!(schema.hasTable("users").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("job_batches_continuations").await.unwrap());
    assert!(schema.hasTable("job_batches_additions").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("job_batches_additions").await.unwrap());
    assert!(schema.hasTable("job_batches_outbox").await.unwrap());
    let batches = DatabaseBatchRepository::new(db.clone(), "job_batches").unwrap();
    let pending = batches
        .store("legacy pending", Default::default())
        .await
        .unwrap();
    db.table("job_batches_outbox")
        .insert(&[[
            ("batch_id".into(), pending.id.as_str().into()),
            ("connection".into(), "redis".into()),
            ("queue".into(), "default".into()),
            ("payloads".into(), "[]".into()),
        ]
        .into_iter()
        .collect()])
        .await
        .unwrap();
    let upgraded = success(run(&["migrate"]));
    assert!(upgraded.contains("create_job_batches_additions_table"));
    assert!(upgraded.contains("create_job_batches_continuations_table"));
    assert_eq!(
        db.table("job_batches_outbox")
            .r#where("batch_id", "=", pending.id.as_str())
            .count("*")
            .await
            .unwrap(),
        1
    );
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(batches.delete(&pending.id).await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("job_batches_outbox").await.unwrap());
    assert!(schema.hasTable("job_batches_jobs").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("job_batches_jobs").await.unwrap());
    assert!(schema.hasTable("job_batches").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("job_batches").await.unwrap());
    assert!(schema.hasTable("failed_jobs").await.unwrap());
    let upgrade = success(run(&["migrate"]));
    assert!(upgrade.contains("create_job_batches_table"));
    assert!(upgrade.contains("create_job_batches_jobs_table"));
    assert!(upgrade.contains("create_job_batches_outbox_table"));
    assert!(upgrade.contains("create_job_batches_additions_table"));
    assert!(upgrade.contains("create_job_batches_continuations_table"));
    assert!(!upgrade.contains("create_failed_jobs_table"));
    assert!(schema.hasTable("job_batches").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("failed_jobs").await.unwrap());
    assert!(schema.hasTable("jobs").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("jobs").await.unwrap());
    assert!(schema.hasTable("cache").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("cache").await.unwrap());
    assert!(schema.hasTable("users").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("users").await.unwrap());
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 0);

    success(run(&["migrate:refresh"]));
    assert!(schema.hasTable("users").await.unwrap());
    if mysql.is_none() {
        db.unprepared(
            "create table extra (id integer); create view extra_view as select id from extra",
        )
        .await
        .unwrap();
        success(run(&["migrate:fresh", "--drop-views"]));
        assert!(!schema.hasTable("extra").await.unwrap());
        assert!(db.select("select * from extra_view", &[]).await.is_err());
        assert!(schema.hasTable("users").await.unwrap());
    }
    success(run(&["migrate:reset"]));
    assert!(!schema.hasTable("users").await.unwrap());
    assert!(!schema.hasTable("jobs").await.unwrap());
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 0);
}

async fn verify_batches(db: Connection) {
    let repository = DatabaseBatchRepository::new(db, "job_batches").unwrap();
    let options =
        serde_json::from_value(json!({"allowFailures": true, "application": "example"})).unwrap();
    let batch = repository.store("reports", options).await.unwrap();
    assert_eq!(
        repository.find(&batch.id).await.unwrap(),
        Some(batch.clone())
    );
    assert!(batch.allowsFailures());
    assert!(!batch.finished());
    assert_eq!(batch.progress(), 0);

    let batch_id = &batch.id;
    repository.incrementTotalJobs(batch_id, 8).await.unwrap();
    let mut jobs = tokio::task::JoinSet::new();
    for index in 0..8 {
        let repository = repository.clone();
        let id = batch.id.clone();
        jobs.spawn(async move {
            repository
                .decrementPendingJobs(&id, &format!("job-{index}"))
                .await
                .unwrap()
        });
    }
    while let Some(outcome) = jobs.join_next().await {
        assert!(outcome.unwrap().is_some());
    }
    let batch = repository.markAsFinished(batch_id).await.unwrap().unwrap();
    assert_eq!(batch.pending_jobs, 0);
    assert_eq!(batch.progress(), 100);
    let json = serde_json::to_value(&batch).unwrap();
    assert_eq!(json["processedJobs"], 8);
    assert_eq!(json["progress"], 100);
    assert!(json["createdAt"].as_str().unwrap().ends_with('Z'));
    assert!(json.get("failedJobIds").is_none());
    assert!(batch.finished());
    assert!(
        repository
            .decrementPendingJobs(batch_id, "underflow")
            .await
            .is_err()
    );
    assert_eq!(repository.find(batch_id).await.unwrap(), Some(batch));

    let pending = repository
        .incrementTotalJobs(batch_id, 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pending.pending_jobs, 2);
    assert!(!pending.finished());
    for _ in 0..2 {
        let failure = repository
            .incrementFailedJobs(batch_id, "job-failed")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failure.pending_jobs, 2);
        assert!(failure.hasFailures());
        assert_eq!(failure.failed_job_ids, ["job-failed"]);
    }
    let retried = repository
        .decrementPendingJobs(batch_id, "job-failed")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retried.failed_jobs, 2);
    assert!(retried.failed_job_ids.is_empty());
    let cancelled = repository.cancel(batch_id).await.unwrap().unwrap();
    assert!(cancelled.cancelled());
    assert!(cancelled.canceled());
    assert_eq!(cancelled.finished_at, cancelled.cancelled_at);
    assert!(repository.delete(batch_id).await.unwrap());
    assert!(repository.find(batch_id).await.unwrap().is_none());
    assert!(repository.cancel(batch_id).await.unwrap().is_none());
}

#[tokio::test]
async fn artisan_migrations_use_a_real_sqlite_ledger() {
    verify(None).await;
}

#[tokio::test]
async fn artisan_migrations_use_a_real_mysql_ledger() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify(Some(&url)).await;
}

#[test]
fn artisan_generates_a_migration_without_overwriting_it() {
    let root = Root::new();
    let args = [
        "make:migration",
        "CreateInventoryTable",
        "--path",
        "database/migrations",
    ];
    let output = success(root.run("local", None, &args));
    assert!(output.contains("create_inventory_table.rs"));
    let files: Vec<_> = fs::read_dir(root.0.join("database/migrations"))
        .unwrap()
        .collect();
    assert_eq!(files.len(), 1);
    let path = files.into_iter().next().unwrap().unwrap().path();
    let original = fs::read_to_string(&path).unwrap();
    assert!(
        original.contains("create(\"inventory\"")
            && original.contains("dropIfExists(\"inventory\"")
    );
    assert_eq!(root.run("local", None, &args).status.code(), Some(1));
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[tokio::test]
async fn mysql_fresh_wipes_the_selected_schema() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_WIPE_URL") else {
        return;
    };
    let root = Root::new();
    let db = root.connection(Some(&url));
    let mut schema = db.getSchemaBuilder();
    let run = |args: &[&str]| root.run("local", Some(&url), args);
    success(run(&["db:wipe", "--drop-views"]));
    success(run(&["migrate"]));
    db.unprepared(
        "create table wipe_parent (id bigint primary key);          create table wipe_child (parent bigint references wipe_parent(id));          create view wipe_view as select id from wipe_parent",
    ).await.unwrap();
    success(run(&["migrate:fresh", "--drop-views", "--step"]));
    assert!(!schema.hasTable("wipe_parent").await.unwrap());
    assert!(db.select("select * from wipe_view", &[]).await.is_err());
    assert!(schema.hasTable("users").await.unwrap());
    assert!(schema.hasTable("cache").await.unwrap());
    assert!(schema.hasTable("cache_locks").await.unwrap());
    assert!(schema.hasTable("jobs").await.unwrap());
    assert!(schema.hasTable("failed_jobs").await.unwrap());
    assert!(schema.hasTable("job_batches").await.unwrap());
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 9);
    success(run(&["db:wipe", "--drop-views"]));
    assert!(schema.getTables(None).await.unwrap().is_empty());
}
