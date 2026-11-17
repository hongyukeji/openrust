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
        self.command(environment, mysql, args).output().unwrap()
    }

    fn command(&self, environment: &str, mysql: Option<&str>, args: &[&str]) -> Command {
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
        command.arg("artisan").args(args);
        command
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

#[path = "../database/migrations/mod.rs"]
mod application_migrations;

fn sanctum_migration() -> Option<&'static str> {
    application_migrations::entries()
        .iter()
        .find(|entry| {
            entry
                .path
                .ends_with("_create_personal_access_tokens_table.rs")
        })
        .map(|entry| entry.path)
}

#[cfg(feature = "sanctum")]
async fn verify_sanctum(root: &Root, mysql: Option<&str>, db: &Connection, path: &str) {
    let run = |args: &[&str]| success(root.run("local", mysql, args));
    let migration_name = std::path::Path::new(path)
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap();
    let ledger = || {
        db.table("migrations")
            .r#where("migration", "like", "%create_personal_access_tokens_table")
    };
    assert_eq!(
        ledger().pluck("migration").await.unwrap(),
        [migration_name.into()]
    );
    let mut schema = db.getSchemaBuilder();
    let columns = schema.getColumns("personal_access_tokens").await.unwrap();
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        [
            "id",
            "tokenable_type",
            "tokenable_id",
            "name",
            "token",
            "abilities",
            "last_used_at",
            "expires_at",
            "created_at",
            "updated_at",
        ]
    );
    let indexes = schema.getIndexes("personal_access_tokens").await.unwrap();
    for (columns, unique) in [
        (&["id"][..], true),
        (&["tokenable_type", "tokenable_id"][..], false),
        (&["token"][..], true),
        (&["expires_at"][..], false),
    ] {
        assert!(
            indexes.iter().any(|index| {
                index.unique == unique
                    && index
                        .columns
                        .iter()
                        .map(Option::as_deref)
                        .eq(columns.iter().copied().map(Some))
            }),
            "missing index on {columns:?}"
        );
    }

    let token = "a".repeat(64);
    let insert = "INSERT INTO personal_access_tokens (tokenable_type, tokenable_id, name, token) VALUES (?, ?, ?, ?)";
    db.insert(
        insert,
        &[
            "app::models::User".into(),
            42.into(),
            "device".into(),
            token.as_str().into(),
        ],
    )
    .await
    .unwrap();
    assert!(
        db.insert(
            insert,
            &[
                "app::models::Other".into(),
                42.into(),
                "other".into(),
                token.as_str().into()
            ]
        )
        .await
        .is_err()
    );
    assert!(run(&["migrate", "--path", path]).contains("Nothing to migrate"));
    assert_eq!(
        db.table("personal_access_tokens").count("*").await.unwrap(),
        1
    );
    assert!(run(&["migrate:rollback", "--path", path, "--step=1"]).contains("DONE"));
    assert!(!schema.hasTable("personal_access_tokens").await.unwrap());
    assert_eq!(ledger().count("*").await.unwrap(), 0);
    assert!(run(&["migrate", "--path", path]).contains("DONE"));
    assert_eq!(
        ledger().pluck("migration").await.unwrap(),
        [migration_name.into()]
    );
    assert!(schema.hasTable("personal_access_tokens").await.unwrap());
    assert_eq!(
        db.table("personal_access_tokens").count("*").await.unwrap(),
        0
    );
    assert!(run(&["migrate:rollback", "--path", path, "--step=1"]).contains("DONE"));
    assert!(!schema.hasTable("personal_access_tokens").await.unwrap());
    assert!(schema.hasTable("users").await.unwrap());
    assert!(run(&["migrate", "--path", path]).contains("DONE"));
    verify_token_storage(db).await;
    db.table("personal_access_tokens").delete().await.unwrap();
    let now = chrono::Utc::now();
    for (name, age, expired) in [
        ("explicit", 60, Some(30)),
        ("young-expiry", 60, Some(2)),
        ("old-created", 30 * 60, None),
        ("active", 1, None),
    ] {
        let timestamp = |minutes| {
            (now - chrono::TimeDelta::minutes(minutes))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        };
        db.insert(
            "INSERT INTO personal_access_tokens (tokenable_type, tokenable_id, name, token, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)",
            &[
                "users".into(), 42.into(), name.into(), format!("{name:0<64}").into(),
                timestamp(age).into(),
                expired.map_or(openrust::database::Value::Null, |minutes| timestamp(minutes).into()),
            ],
        ).await.unwrap();
    }
    assert!(run(&["sanctum:prune-expired"]).contains("[24 hours]"));
    assert_eq!(
        db.table("personal_access_tokens").count("*").await.unwrap(),
        4
    );
    std::fs::write(root.0.join(".env"), "SANCTUM_EXPIRATION=60\n").unwrap();
    assert!(run(&["sanctum:prune-expired"]).contains("[24 hours]"));
    assert_eq!(
        db.table("personal_access_tokens").count("*").await.unwrap(),
        3
    );
    assert!(run(&["sanctum:prune-expired", "--hours=0"]).contains("[0 hours]"));
    assert_eq!(
        db.table("personal_access_tokens").count("*").await.unwrap(),
        1
    );
    std::fs::remove_file(root.0.join(".env")).unwrap();
    assert!(run(&["migrate:rollback", "--path", path, "--step=1"]).contains("DONE"));
}

#[cfg(feature = "sanctum")]
async fn verify_token_storage(db: &Connection) {
    use openrust::{database::Value, sanctum::PersonalAccessTokens};

    let store = PersonalAccessTokens::new(db.clone(), "fixture_");
    let expiry = chrono::DateTime::from_timestamp(1_900_000_000, 0).unwrap();
    let issued = store
        .createToken("users", 42, "browser", &["read"], Some(expiry))
        .await
        .unwrap();
    let (id, secret) = issued.plain_text_token.split_once('|').unwrap();
    assert_eq!(id, issued.access_token.id.to_string());
    let entropy_and_checksum = secret.strip_prefix("fixture_").unwrap();
    assert_eq!(entropy_and_checksum.len(), 48);
    assert!(
        entropy_and_checksum[..40]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric())
    );
    assert_eq!(
        &entropy_and_checksum[40..],
        format!(
            "{:08x}",
            crc32fast_for_fixture(&entropy_and_checksum.as_bytes()[..40])
        )
    );

    let stored = db
        .table("personal_access_tokens")
        .first()
        .await
        .unwrap()
        .unwrap();
    let Some(Value::Text(hash)) = stored.get("token") else {
        panic!("missing digest")
    };
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(hash, secret);
    assert!(store.findToken(hash).await.unwrap().is_none());
    for credential in [issued.plain_text_token.as_str(), secret] {
        let token = store.findToken(credential).await.unwrap().unwrap();
        assert_eq!(token.id, issued.access_token.id);
        assert_eq!(
            (token.tokenable_type.as_str(), &token.tokenable_id),
            (
                "users",
                &openrust::auth::AuthIdentifier::UnsignedInteger(42)
            )
        );
        assert_eq!(token.expires_at, Some(expiry));
        assert!(token.can("read") && token.cant("write"));
        assert!(!token.isExpired(expiry - chrono::TimeDelta::seconds(1), None));
        assert!(!token.isExpired(expiry, None));
        assert!(token.isExpired(expiry + chrono::TimeDelta::seconds(1), None));
        assert!(token.isExpired(
            token.created_at.unwrap() + chrono::TimeDelta::minutes(1),
            Some(chrono::TimeDelta::minutes(1))
        ));
    }
    for invalid in [
        "",
        "junk",
        "+1|invalid",
        "18446744073709551616|invalid",
        "1|invalid",
    ] {
        assert!(store.findToken(invalid).await.unwrap().is_none());
    }
    let (a, b) = tokio::join!(
        store.createToken("users", 42, "one", &["*"], None),
        store.createToken("users", 42, "two", &["*"], None),
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a.plain_text_token.as_str(), b.plain_text_token.as_str());
    assert!(a.access_token.can("write"));
    let id = issued.access_token.id;
    assert!(!store.revokeToken("other", 42, id.clone()).await.unwrap());
    assert!(!store.revokeToken("users", 43, id.clone()).await.unwrap());
    assert!(store.revokeToken("users", 42, id.clone()).await.unwrap());
    assert!(!store.revokeToken("users", 42, id.clone()).await.unwrap());
    assert!(
        store
            .findToken(&issued.plain_text_token)
            .await
            .unwrap()
            .is_none()
    );
}

#[cfg(feature = "sanctum")]
fn crc32fast_for_fixture(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |mut crc, byte| {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
        crc
    })
}

async fn verify(mysql: Option<&str>) {
    let root = Root::new();
    let db = root.connection(mysql);
    let mut schema = db.getSchemaBuilder();
    for name in [
        "personal_access_tokens",
        "job_chain_deliveries",
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
    let cancelled = root.run(
        "production",
        mysql,
        &["migrate", "--graceful", "--no-interaction"],
    );
    assert_eq!(cancelled.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&cancelled.stdout).contains("cancelled"));
    assert!(!schema.hasTable("migrations").await.unwrap());

    let preview = success(run(&["migrate", "--pretend"]));
    assert!(preview.contains("create table") && preview.contains("users"));
    assert!(preview.contains("job_batches"));
    assert!(!schema.hasTable("users").await.unwrap());
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 0);
    assert!(success(run(&["migrate:status", "--pending"])).contains("Pending"));
    let pending = run(&["migrate:status", "--pending=17"]);
    assert_eq!(pending.status.code(), Some(17));
    assert!(String::from_utf8_lossy(&pending.stdout).contains("Pending"));

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
    schema
        .create("cache", |table| {
            table.id();
        })
        .await
        .unwrap();
    let failed = run(&["migrate"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("create_cache_table"));
    let graceful = success(run(&["migrate", "--graceful"]));
    assert!(graceful.contains("WARN") && graceful.contains("create_cache_table"));
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 1);
    assert!(schema.hasTable("users").await.unwrap());
    assert!(!schema.hasTable("jobs").await.unwrap());
    schema.drop("cache").await.unwrap();
    let repaired = success(run(&["migrate", "--graceful"]));
    assert!(repaired.contains("DONE") && !repaired.contains("WARN"));
    assert!(schema.hasTable("cache").await.unwrap());
    assert!(schema.hasTable("cache_locks").await.unwrap());
    assert!(schema.hasTable("jobs").await.unwrap());
    assert!(schema.hasTable("failed_jobs").await.unwrap());
    assert!(schema.hasTable("job_batches").await.unwrap());
    verify_batches(db.clone()).await;
    assert!(success(run(&["migrate"])).contains("Nothing to migrate"));
    let seed = [
        "migrate",
        "--seed",
        "--seeder=MissingMigrationFixtureSeeder",
    ];
    assert_eq!(run(&seed).status.code(), Some(1));
    let graceful = success(run(&[
        "migrate",
        "--graceful",
        "--seed",
        "--seeder=MissingMigrationFixtureSeeder",
    ]));
    assert!(graceful.contains("WARN") && graceful.contains("MissingMigrationFixtureSeeder"));
    assert!(success(run(&["migrate:status", "--pending=17"])).contains("No pending migrations"));
    assert_eq!(
        db.table("migrations").count("*").await.unwrap(),
        12 + u64::from(sanctum_migration().is_some())
    );
    assert!(
        success(run(&["migrate:rollback", "--pretend", "--batch", "1"])).contains("drop table")
    );
    assert!(schema.hasTable("users").await.unwrap());
    assert_eq!(
        schema.hasTable("personal_access_tokens").await.unwrap(),
        sanctum_migration().is_some()
    );
    if let Some(path) = sanctum_migration() {
        #[cfg(feature = "sanctum")]
        verify_sanctum(&root, mysql, &db, path).await;
        #[cfg(not(feature = "sanctum"))]
        assert!(success(run(&["migrate:rollback", "--path", path, "--step=1"])).contains("DONE"));
    }
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(
        !schema
            .hasColumn("job_chain_deliveries", "acknowledged_at")
            .await
            .unwrap()
    );
    assert!(schema.hasTable("job_chain_deliveries").await.unwrap());
    db.table("job_chain_deliveries")
        .insert(&[[("id".into(), "legacy-chain".into())].into_iter().collect()])
        .await
        .unwrap();
    let receipt_upgrade = success(run(&["migrate"]));
    assert!(receipt_upgrade.contains("add_acknowledged_at_to_job_chain_deliveries"));
    assert_eq!(
        db.table("job_chain_deliveries")
            .r#where("id", "=", "legacy-chain")
            .whereNull("acknowledged_at")
            .count("*")
            .await
            .unwrap(),
        1
    );
    if sanctum_migration().is_some() {
        assert!(
            success(run(&["migrate:rollback", "--step=1"]))
                .contains("create_personal_access_tokens_table")
        );
    }
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(!schema.hasTable("job_chain_deliveries").await.unwrap());
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(
        !schema
            .hasColumn("job_batches_jobs", "chain_successor")
            .await
            .unwrap()
    );
    assert!(schema.hasTable("job_batches_continuations").await.unwrap());
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
    assert!(upgraded.contains("add_chain_successor_to_job_batches_jobs"));
    assert!(upgraded.contains("create_job_chain_deliveries_table"));
    assert!(upgraded.contains("add_acknowledged_at_to_job_chain_deliveries"));
    assert_eq!(
        db.table("job_batches_outbox")
            .r#where("batch_id", "=", pending.id.as_str())
            .count("*")
            .await
            .unwrap(),
        1
    );
    if sanctum_migration().is_some() {
        assert!(
            success(run(&["migrate:rollback", "--step=1"]))
                .contains("create_personal_access_tokens_table")
        );
    }
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
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
    assert!(upgrade.contains("add_chain_successor_to_job_batches_jobs"));
    assert!(upgrade.contains("create_job_chain_deliveries_table"));
    assert!(upgrade.contains("add_acknowledged_at_to_job_chain_deliveries"));
    assert!(!upgrade.contains("create_failed_jobs_table"));
    assert!(schema.hasTable("job_batches").await.unwrap());
    if sanctum_migration().is_some() {
        assert!(
            success(run(&["migrate:rollback", "--step=1"]))
                .contains("create_personal_access_tokens_table")
        );
    }
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
    assert!(success(run(&["migrate:rollback", "--step=1"])).contains("DONE"));
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
    db.statement(
        "insert into users (name, email, password) values ('Hold', 'hold@example.test', 'example')",
        &[],
    )
    .await
    .unwrap();
    let before = db.table("migrations").count("*").await.unwrap();
    let output = success(run(&["migrate:refresh", "--step=1"]));
    assert_eq!(
        output
            .lines()
            .filter(|line| line.ends_with(" DONE"))
            .count(),
        2
    );
    assert!(!output.contains("create_users_table DONE"));
    assert_eq!(db.table("users").count("*").await.unwrap(), 1);
    assert_eq!(db.table("migrations").count("*").await.unwrap(), before);
    let output = success(run(&["migrate:refresh", "--step=0"]));
    assert!(output.contains("create_users_table DONE"));
    assert_eq!(db.table("users").count("*").await.unwrap(), 0);
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
    assert_eq!(
        db.table("migrations").count("*").await.unwrap(),
        12 + u64::from(sanctum_migration().is_some())
    );
    success(run(&["db:wipe", "--drop-views"]));
    assert!(
        schema
            .getTables(Some(db.getDatabaseName()))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn mysql_artisan_migrate_isolated_coordinates_two_real_processes() {
    use openrust::redis::RedisManager;
    use std::{process::Stdio, time::Duration};

    let Ok(redis_url) = std::env::var("OPENRUST_ACCEPTANCE_REDIS_URL") else {
        return;
    };
    let Ok(mysql) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = root.connection(Some(&mysql));
    let prefix = format!("migration-isolation-{}:", std::process::id());
    let key = format!("{prefix}framework/command-migrate");
    let redis = RedisManager::new(Arc::new(Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"redis":{"default":{"url":redis_url}}}),
    )]))))
    .connection(None)
    .unwrap();

    let command = |args: &[&str]| {
        let mut command = root.command("local", Some(&mysql), args);
        command
            .env("CACHE_STORE", "redis")
            .env("CACHE_PREFIX", &prefix)
            .env("REDIS_PREFIX", "")
            .env("REDIS_URL", &redis_url)
            .env("REDIS_CACHE_LOCK_CONNECTION", "default");
        command
    };
    let run = |args: &[&str]| command(args).output().unwrap();
    success(run(&["migrate:install"]));
    let unavailable = command(&["migrate", "--isolated"])
        .env("CACHE_STORE", "missing")
        .output()
        .unwrap();
    assert_eq!(unavailable.status.code(), Some(1));
    assert!(
        String::from_utf8(unavailable.stderr)
            .unwrap()
            .contains("Command isolation lock")
    );
    assert_eq!(db.table("migrations").count("*").await.unwrap(), 0);

    let mut gate = db.beginTransaction().await.unwrap();
    // The empty InnoDB ledger range prevents the first process from recording a migration.
    gate.select("select id from migrations where id >= 0 for update", &[])
        .await
        .unwrap();
    let mut holder = tokio::process::Command::from(command(&["migrate", "--isolated"]))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let owner = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            assert!(
                holder.try_wait().unwrap().is_none(),
                "migration holder exited early"
            );
            if let Some(owner) = redis.get(&key).await.unwrap() {
                break owner;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("migration holder acquired the cache lock");
    let ttl = redis.command("TTL", &[key.as_bytes()]).await.unwrap();
    assert!(matches!(ttl, openrust::redis::Value::Int(3500..=3600)));
    assert!(
        success(run(&["migrate", "--isolated"]))
            .contains("The [migrate] command is already running.")
    );
    let skipped = run(&["migrate", "--isolated=19", "--database=mysql"]);
    assert_eq!(skipped.status.code(), Some(19));
    assert!(
        String::from_utf8(skipped.stdout)
            .unwrap()
            .contains("The [migrate] command is already running.")
    );
    assert_eq!(redis.get(&key).await.unwrap(), Some(owner));

    gate.rollBack(None).await.unwrap();
    let output = tokio::time::timeout(Duration::from_secs(15), holder.wait_with_output())
        .await
        .expect("migration holder finished after releasing the ledger")
        .unwrap();
    success(output);
    assert!(redis.get(&key).await.unwrap().is_none());
    assert!(db.table("migrations").count("*").await.unwrap() > 0);
    let failed = run(&["migrate", "--isolated", "--database=missing"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(redis.get(&key).await.unwrap().is_none());
    assert!(success(run(&["migrate", "--isolated"])).contains("Nothing to migrate."));

    let database_cache = root
        .command("local", Some(&mysql), &["migrate", "--isolated"])
        .env("CACHE_STORE", "database")
        .env("CACHE_PREFIX", &prefix)
        .output()
        .unwrap();
    assert!(success(database_cache).contains("Nothing to migrate."));
    assert_eq!(db.table("cache_locks").count("*").await.unwrap(), 0);
}

#[tokio::test]
async fn mysql_artisan_dumps_and_restores_schema_and_migration_history() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    if std::env::var_os("OPENRUST_TEST_SCHEMA_DUMP").is_none() {
        return;
    }
    let root = Root::new();
    let db = root.connection(Some(&url));
    let run = |args: &[&str]| root.run("local", Some(&url), args);
    let source = application_migrations::entries()[0].path;
    let name = std::path::Path::new(source)
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap();
    success(run(&["migrate", "--path", source]));
    db.statement(
        "create table snapshot_rows (id int auto_increment primary key, label varchar(60))",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        "insert into snapshot_rows (id, label) values (140, 'application data to exclude')",
        &[],
    )
    .await
    .unwrap();
    db.statement(
        "create view snapshot_view as select label from snapshot_rows",
        &[],
    )
    .await
    .unwrap();
    db.unprepared("create trigger snapshot_row_insert before insert on snapshot_rows for each row set new.label = upper(new.label)")
        .await
        .unwrap();
    db.unprepared("create procedure snapshot_ready() select 1")
        .await
        .unwrap();
    success(run(&["schema:dump", "--database=mysql"]));
    let snapshot = root.0.join("database/schema/mysql-schema.sql");
    let dump = fs::read_to_string(&snapshot).unwrap();
    assert!(dump.contains("CREATE TABLE `snapshot_rows`") && dump.contains("snapshot_view"));
    assert!(dump.contains("INSERT INTO `migrations`") && dump.contains(name));
    assert!(!dump.contains("application data to exclude") && !dump.contains("AUTO_INCREMENT=141"));
    success(run(&[
        "schema:dump",
        "--path=database/schema/structure.sql",
        "--without-migration-data",
    ]));
    let structure = fs::read_to_string(root.0.join("database/schema/structure.sql")).unwrap();
    assert!(structure.contains("CREATE TABLE `migrations`"));
    assert!(!structure.contains("INSERT INTO `migrations`"));
    assert_eq!(fs::read_to_string(&snapshot).unwrap(), dump);

    let source_path = root.0.join(source);
    let applications = source_path.parent().unwrap();
    fs::create_dir_all(applications).unwrap();
    fs::write(&source_path, "migration source").unwrap();
    fs::write(applications.join("mod.rs"), "module glue").unwrap();
    fs::write(applications.join("notes.txt"), "unrelated").unwrap();
    let conflicting_output = format!("--path={source}");
    assert_eq!(
        run(&["schema:dump", "--prune", &conflicting_output])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(&source_path).unwrap(),
        "migration source"
    );
    success(run(&["schema:dump", "--prune"]));
    assert!(!source_path.exists());
    assert!(applications.join("mod.rs").is_file() && applications.join("notes.txt").is_file());

    success(run(&["db:wipe", "--drop-views"]));
    let mut schema = db.getSchemaBuilder();
    assert!(success(run(&["migrate", "--pretend"])).contains(name));
    assert!(!schema.hasTable("snapshot_rows").await.unwrap());
    let invalid = root.0.join("database/schema/invalid.sql");
    fs::write(&invalid, "THIS IS INVALID SQL;").unwrap();
    let failed = run(&["migrate", "--schema-path=database/schema/invalid.sql"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("database schema command mysql failed")
    );
    assert!(!schema.hasTable("migrations").await.unwrap());

    let output = success(run(&["migrate"]));
    assert!(output.contains("Loading stored database schemas."));
    assert!(!output.contains(&format!("{name} DONE")));
    assert!(schema.hasTable("snapshot_rows").await.unwrap());
    assert!(schema.hasView("snapshot_view").await.unwrap());
    assert_eq!(db.table("snapshot_rows").count("*").await.unwrap(), 0);
    db.statement(
        "insert into snapshot_rows (label) values ('trigger restored')",
        &[],
    )
    .await
    .unwrap();
    assert_eq!(
        db.table("snapshot_rows").value("label").await.unwrap(),
        "TRIGGER RESTORED".into()
    );
    assert_eq!(db.scalar("select count(*) from information_schema.routines where routine_schema = database() and routine_name = 'snapshot_ready'", &[])
        .await
        .unwrap(), 1.into());
    assert!(db.table("migrations").count("*").await.unwrap() > 1);
    let output = success(run(&["migrate"]));
    assert!(output.contains("Nothing to migrate."));
    assert!(!output.contains("Loading stored database schemas."));
    let custom = root.0.join("database/schema/custom.sql");
    fs::rename(&snapshot, &custom).unwrap();
    let output = success(run(&[
        "migrate:fresh",
        "--drop-views",
        "--schema-path=database/schema/custom.sql",
    ]));
    assert!(output.contains("Loading stored database schemas."));
    assert!(!output.contains(&format!("{name} DONE")));
    assert!(schema.hasView("snapshot_view").await.unwrap());
    assert_eq!(db.table("snapshot_rows").count("*").await.unwrap(), 0);

    let (endpoint, _) = url.rsplit_once('/').unwrap();
    let missing_name = "openrust-refresh-missing";
    let missing = format!("{endpoint}/{missing_name}");
    let failed = root.run("local", Some(&missing), &["migrate:refresh", "--force"]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(
        !db.table("information_schema.schemata")
            .r#where("schema_name", "=", missing_name)
            .exists()
            .await
            .unwrap()
    );

    fs::rename(custom, snapshot).unwrap();
    success(run(&["db:wipe", "--drop-views"]));
    let output = success(run(&["migrate:refresh", "--no-interaction"]));
    assert!(output.contains("Loading stored database schemas."));
    assert!(!output.contains(&format!("{name} DONE")));
    assert!(schema.hasView("snapshot_view").await.unwrap());
    assert_eq!(db.table("snapshot_rows").count("*").await.unwrap(), 0);
    assert!(db.table("migrations").count("*").await.unwrap() > 1);
}

#[tokio::test]
async fn mysql_artisan_creates_only_confirmed_missing_databases() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    if std::env::var_os("OPENRUST_TEST_DATABASE_CREATION").is_none() {
        return;
    }
    let root = Root::new();
    let admin = root.connection(Some(&url));
    let exists = |name: &str| {
        let admin = admin.clone();
        let name = name.to_owned();
        async move {
            admin
                .table("information_schema.schemata")
                .r#where("schema_name", "=", name.as_str())
                .exists()
                .await
                .unwrap()
        }
    };
    let (endpoint, _) = url.rsplit_once('/').expect("MySQL fixture database");
    let name = "openrust-migrate-create";
    let target = format!("{endpoint}/{name}");
    assert!(!exists(name).await);
    let run = |environment, args: &[&str]| root.run(environment, Some(target.as_str()), args);

    assert_eq!(
        run("local", &["migrate", "--no-interaction"]).status.code(),
        Some(1)
    );
    assert_eq!(
        run("production", &["migrate", "--no-interaction"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        run("local", &["migrate", "--force", "--database=mysql::read"])
            .status
            .code(),
        Some(1)
    );
    assert!(!exists(name).await);

    let output = success(run(
        "production",
        &["migrate", "--database=mysql", "--force", "--no-interaction"],
    ));
    assert!(output.contains("create_users_table DONE"));
    assert!(exists(name).await);
    let created = root.connection(Some(target.as_str()));
    assert!(created.getSchemaBuilder().hasTable("users").await.unwrap());
    let recorded = created.table("migrations").count("*").await.unwrap();
    assert!(recorded > 0);
    assert!(
        success(run("local", &["migrate", "--no-interaction"])).contains("Nothing to migrate.")
    );
    assert_eq!(
        created.table("migrations").count("*").await.unwrap(),
        recorded
    );

    let denied_name = "openrust-create-denied";
    let (_, host) = endpoint
        .rsplit_once('@')
        .expect("MySQL fixture credentials");
    let denied = format!("mysql://migration_reader:fixture-password@{host}/{denied_name}");
    let failure = root.run("local", Some(denied.as_str()), &["migrate", "--force"]);
    assert_eq!(failure.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failure.stderr).contains("database create database failed"));
    assert!(!exists(denied_name).await);

    let fresh_name = "openrust-fresh-create";
    let fresh_url = format!("{endpoint}/{fresh_name}");
    let fresh = |environment, args: &[&str]| root.run(environment, Some(&fresh_url), args);
    assert_eq!(
        fresh("production", &["migrate:fresh", "--no-interaction"])
            .status
            .code(),
        Some(1)
    );
    assert!(!exists(fresh_name).await);
    assert!(
        success(fresh("local", &["migrate:fresh", "--no-interaction"]))
            .contains("create_users_table DONE")
    );
    let fresh_db = root.connection(Some(&fresh_url));
    assert!(fresh_db.getSchemaBuilder().hasTable("users").await.unwrap());
    assert!(fresh_db.table("migrations").count("*").await.unwrap() > 0);

    let existing_name = "openrust-fresh-no-ledger";
    let existing_url = format!("{endpoint}/{existing_name}");
    admin
        .getSchemaBuilder()
        .createDatabase(existing_name)
        .await
        .unwrap();
    let existing = root.connection(Some(&existing_url));
    existing
        .statement(
            "create table independent_sentinel (id int primary key)",
            &[],
        )
        .await
        .unwrap();
    success(root.run(
        "local",
        Some(&existing_url),
        &["migrate:fresh", "--no-interaction"],
    ));
    assert!(
        existing
            .getSchemaBuilder()
            .hasTable("independent_sentinel")
            .await
            .unwrap()
    );
    assert!(existing.getSchemaBuilder().hasTable("users").await.unwrap());
}

#[test]
fn artisan_generates_cache_tables_and_rejects_duplicate_migrations() {
    let root = Root::new();
    for name in ["make:cache-table", "cache:table"] {
        let help = success(root.run("local", None, &[name, "--help"]));
        assert!(help.contains("Create a migration for the cache database table"));
        assert!(!help.contains("not implemented yet"));
    }

    let output = success(root.run("local", None, &["cache:table"]));
    assert!(output.contains("Migration created successfully."));
    assert!(output.contains("Rebuild the application to include the migration."));
    let directory = root.0.join("database/migrations");
    let files: Vec<_> = fs::read_dir(&directory)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(files.len(), 1);
    let path = files[0].path();
    assert!(
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("_create_cache_table.rs")
    );
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("create(\"cache\""));
    assert!(source.contains("table.mediumText(\"value\");"));
    assert!(source.contains("create(\"cache_locks\""));
    assert_eq!(
        source
            .matches("table.bigInteger(\"expiration\").index();")
            .count(),
        2
    );
    assert!(source.contains("dropIfExists(\"cache\")"));
    assert!(source.contains("dropIfExists(\"cache_locks\")"));

    let duplicate = root.run("local", None, &["make:cache-table"]);
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("Migration already exists."));
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    assert_eq!(fs::read_to_string(path).unwrap(), source);

    let bundled = Root::new();
    let directory = bundled.0.join("database/migrations");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("0001_01_01_000001_create_cache_table.rs");
    fs::write(&path, "bundled migration").unwrap();
    let duplicate = bundled.run("local", None, &["cache:table"]);
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("Migration already exists."));
    assert_eq!(fs::read_to_string(path).unwrap(), "bundled migration");
}
