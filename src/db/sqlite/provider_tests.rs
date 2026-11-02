//! 标准数据库 Provider 建池路径的 SQLite 物理连接验收。

use super::super::{DatabaseBackend, DatabaseInstanceConfig, connect_database};
use sea_orm::DatabaseConnection;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

async fn connect(
    url: &str,
    config: &DatabaseInstanceConfig,
    root: Option<&Path>,
) -> crate::Result<DatabaseConnection> {
    connect_database(url, config, DatabaseBackend::Sqlite, "test", None, root).await
}

fn named_memory(shared: bool) -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    format!(
        "sqlite:file:openrust-provider-{}-{}?mode=memory{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed),
        if shared { "&cache=shared" } else { "" }
    )
}

#[tokio::test]
async fn every_physical_connection_receives_defaults_without_forced_journal() -> TestResult {
    let file = tempfile::NamedTempFile::new()?;
    let url = format!(
        "sqlite://{}",
        super::encode_path(file.path().to_str().ok_or("path encoding")?)
    );
    let config = DatabaseInstanceConfig {
        max_connections: 2,
        min_connections: 0,
        ..Default::default()
    };
    let database = connect(&url, &config, None).await?;
    let pool = database.get_sqlite_connection_pool();
    let mut first = pool.acquire().await?;
    let mut second = pool.acquire().await?;
    for physical in [&mut *first, &mut *second] {
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut *physical)
            .await?;
        let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&mut *physical)
            .await?;
        let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(&mut *physical)
            .await?;
        let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut *physical)
            .await?;
        assert_eq!(foreign_keys, 1);
        assert_eq!(busy_timeout, 60_000);
        assert_eq!(synchronous, 2);
        assert_eq!(journal, "delete");
    }
    // TEMP 表属于单条物理连接；第二条连接不能看到它。
    sqlx::query("CREATE TEMP TABLE physical_identity (value INTEGER)")
        .execute(&mut *first)
        .await?;
    assert!(
        sqlx::query("SELECT value FROM physical_identity")
            .fetch_all(&mut *second)
            .await
            .is_err()
    );
    assert_eq!(pool.options().get_max_connections(), 2);
    drop(first);
    drop(second);
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn explicit_special_pragmas_override_misc_on_every_connection() -> TestResult {
    let mut config = DatabaseInstanceConfig {
        max_connections: 2,
        min_connections: 0,
        busy_timeout: Some(43),
        journal_mode: Some("memory".into()),
        synchronous: Some("full".into()),
        ..Default::default()
    };
    config
        .pragmas
        .insert("foreign_keys".into(), serde_json::json!(false));
    config
        .pragmas
        .insert("busy_timeout".into(), serde_json::json!(17));
    config
        .pragmas
        .insert("synchronous".into(), serde_json::json!("OFF"));
    config
        .pragmas
        .insert("journal_mode".into(), serde_json::json!("OFF"));
    let database = connect(&named_memory(true), &config, None).await?;
    let pool = database.get_sqlite_connection_pool();
    let mut first = pool.acquire().await?;
    let mut second = pool.acquire().await?;
    for physical in [&mut *first, &mut *second] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
                .fetch_one(&mut *physical)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
                .fetch_one(&mut *physical)
                .await?,
            43
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA synchronous")
                .fetch_one(&mut *physical)
                .await?,
            2
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
                .fetch_one(&mut *physical)
                .await?,
            "memory"
        );
    }
    drop(first);
    drop(second);
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn misc_busy_timeout_survives_without_an_explicit_special_override() -> TestResult {
    let mut config = DatabaseInstanceConfig::default();
    config
        .pragmas
        .insert("busy_timeout".into(), serde_json::json!(19));
    let database = connect("sqlite::memory:", &config, None).await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
            .fetch_one(database.get_sqlite_connection_pool())
            .await?,
        19
    );
    database.close().await?;
    Ok(())
}

#[tokio::test]
async fn anonymous_memory_has_one_non_recycled_connection_and_retains_rows() -> TestResult {
    for url in ["sqlite::memory:", "sqlite://:memory:"] {
        let config = DatabaseInstanceConfig {
            max_connections: 5,
            min_connections: 3,
            ..Default::default()
        };
        let database = connect(url, &config, None).await.map_err(|error| {
            use std::error::Error as _;
            format!(
                "anonymous pool initialization: {error}; source: {:?}",
                error.source()
            )
        })?;
        let pool = database.get_sqlite_connection_pool();
        assert_eq!(pool.options().get_max_connections(), 1);
        assert_eq!(pool.options().get_idle_timeout(), None);
        assert_eq!(pool.options().get_max_lifetime(), None);
        {
            let mut physical = pool.acquire().await?;
            sqlx::query("CREATE TABLE retained (value INTEGER NOT NULL)")
                .execute(&mut *physical)
                .await?;
            sqlx::query("INSERT INTO retained VALUES (7)")
                .execute(&mut *physical)
                .await?;
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT value FROM retained")
                .fetch_one(pool)
                .await?,
            7
        );
        database.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn private_named_memory_isolated_but_shared_name_shares_rows() -> TestResult {
    let config = DatabaseInstanceConfig {
        max_connections: 2,
        min_connections: 0,
        ..Default::default()
    };
    for shared in [false, true] {
        let url = named_memory(shared);
        let first = connect(&url, &config, None).await?;
        let second = connect(&url, &config, None).await?;
        let first_pool = first.get_sqlite_connection_pool();
        let second_pool = second.get_sqlite_connection_pool();
        assert_eq!(
            first_pool.options().get_max_connections(),
            if shared { 2 } else { 1 }
        );
        assert_eq!(first_pool.options().get_idle_timeout(), None);
        assert_eq!(first_pool.options().get_max_lifetime(), None);
        sqlx::query("CREATE TABLE sharing (value INTEGER NOT NULL)")
            .execute(first_pool)
            .await?;
        sqlx::query("INSERT INTO sharing VALUES (11)")
            .execute(first_pool)
            .await?;
        let observed = sqlx::query_scalar::<_, i64>("SELECT value FROM sharing")
            .fetch_one(second_pool)
            .await;
        if shared {
            assert_eq!(observed?, 11);
        } else {
            assert!(observed.is_err());
        }
        first.close().await?;
        second.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn missing_files_are_not_created_and_errors_do_not_reveal_paths() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("secret-credential-marker.sqlite");
    let url = format!(
        "sqlite://{}",
        super::encode_path(path.to_str().ok_or("path encoding")?)
    );
    let error = match connect(&url, &DatabaseInstanceConfig::default(), None).await {
        Ok(database) => {
            database.close().await?;
            return Err("missing database unexpectedly created".into());
        }
        Err(error) => error,
    };
    assert!(!path.exists());
    assert!(!format!("{error} {error:?}").contains("secret-credential-marker"));
    Ok(())
}

#[tokio::test]
async fn relative_file_uses_application_root_fallback() -> TestResult {
    let root = tempfile::tempdir()?;
    let filename = format!("openrust-root-fallback-{}.sqlite", std::process::id());
    let path = root.path().join(&filename);
    std::fs::File::create(&path)?;
    let url = format!("sqlite:{filename}");
    let database = connect(&url, &DatabaseInstanceConfig::default(), Some(root.path())).await?;
    sqlx::query("CREATE TABLE fallback_identity (value INTEGER)")
        .execute(database.get_sqlite_connection_pool())
        .await?;
    let absolute_url = format!(
        "sqlite://{}",
        super::encode_path(path.to_str().ok_or("path encoding")?)
    );
    let absolute = connect(&absolute_url, &DatabaseInstanceConfig::default(), None).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fallback_identity")
        .fetch_one(absolute.get_sqlite_connection_pool())
        .await?;
    assert_eq!(count, 0);
    database.close().await?;
    absolute.close().await?;
    Ok(())
}

#[tokio::test]
async fn file_uri_decodes_reserved_path_characters_exactly_once() -> TestResult {
    let root = tempfile::tempdir()?;
    let filename = "literal%3F?#.sqlite";
    let path = root.path().join(filename);
    let encoded = super::encode_path(path.to_str().ok_or("path encoding")?);
    let url = format!("sqlite:file:{encoded}");
    let database = connect(&url, &DatabaseInstanceConfig::default(), None).await?;
    sqlx::query("CREATE TABLE uri_identity (value INTEGER)")
        .execute(database.get_sqlite_connection_pool())
        .await?;
    database.close().await?;
    assert!(path.is_file());
    assert!(std::fs::metadata(&path)?.len() > 0);
    assert!(!root.path().join("literal").exists());
    assert!(!root.path().join("literal??#.sqlite").exists());
    assert!(!root.path().join("literal%253F%3F%23.sqlite").exists());
    let entries = std::fs::read_dir(root.path())?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(entries, [std::ffi::OsString::from(filename)]);
    Ok(())
}

#[tokio::test]
async fn native_uri_parameters_keep_encoded_file_identity_and_read_only_mode() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("native?#%.sqlite");
    let uri = url::Url::from_file_path(&path).map_err(|()| "file URI")?;
    let writer = connect(
        &format!("sqlite:{uri}?mode=rwc&psow=0"),
        &DatabaseInstanceConfig::default(),
        None,
    )
    .await?;
    sqlx::query("CREATE TABLE readonly_identity (value INTEGER)")
        .execute(writer.get_sqlite_connection_pool())
        .await?;
    writer.close().await?;
    assert!(path.is_file());
    let reader = connect(
        &format!("sqlite:{uri}?mode=ro&immutable=1"),
        &DatabaseInstanceConfig::default(),
        None,
    )
    .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM readonly_identity")
        .fetch_one(reader.get_sqlite_connection_pool())
        .await?;
    assert_eq!(count, 0);
    assert!(
        sqlx::query("INSERT INTO readonly_identity VALUES (1)")
            .execute(reader.get_sqlite_connection_pool())
            .await
            .is_err()
    );
    reader.close().await?;
    Ok(())
}

#[tokio::test]
async fn sqlite_transaction_savepoint_rollback_preserves_outer_transaction() -> TestResult {
    let database = connect("sqlite::memory:", &DatabaseInstanceConfig::default(), None).await?;
    let pool = database.get_sqlite_connection_pool();
    sqlx::query("CREATE TABLE transaction_identity (value INTEGER)")
        .execute(pool)
        .await?;
    let mut outer = pool.begin().await?;
    sqlx::query("INSERT INTO transaction_identity VALUES (1)")
        .execute(&mut *outer)
        .await?;
    {
        use sqlx::Acquire;
        let mut savepoint = (&mut *outer).begin().await?;
        sqlx::query("INSERT INTO transaction_identity VALUES (2)")
            .execute(&mut *savepoint)
            .await?;
        savepoint.rollback().await?;
    }
    outer.commit().await?;
    let sum: i64 = sqlx::query_scalar("SELECT sum(value) FROM transaction_identity")
        .fetch_one(pool)
        .await?;
    assert_eq!(sum, 1);
    database.close().await?;
    Ok(())
}
