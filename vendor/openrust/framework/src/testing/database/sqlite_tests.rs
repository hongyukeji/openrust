//! SQLite 测试数据库的所有权与共享生命周期契约。

use super::TestDatabase;
#[cfg(feature = "db-sqlite")]
use crate::Result;
use sea_orm_migration::{MigrationTrait, MigratorTrait};

struct EmptyMigrator;

#[sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[cfg(feature = "db-sqlite")]
async fn app(url: &str) -> Result<crate::foundation::ProviderRuntime> {
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder.provider(crate::database::DatabaseServiceProvider);
    builder
        .start(figment::Figment::from(
            figment::providers::Serialized::defaults(serde_json::json!({"database": {"url": url}})),
        ))
        .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn owned_memory_isolated_but_application_url_shares_data() -> Result<()> {
    use sea_orm::ConnectionTrait;
    let first = TestDatabase::sqlite_memory::<EmptyMigrator>().await?;
    let second = TestDatabase::sqlite_memory::<EmptyMigrator>().await?;
    assert_ne!(first.url(), second.url());
    first
        .connection()?
        .execute_unprepared("CREATE TABLE sentinel (id INTEGER PRIMARY KEY)")
        .await
        .map_err(crate::Error::internal)?;
    first
        .connection()?
        .execute_unprepared("INSERT INTO sentinel VALUES (73)")
        .await
        .map_err(crate::Error::internal)?;
    let runtime = app(first.url()).await?;
    let connection = runtime
        .container()
        .resolve::<crate::database::Db>()
        .await?
        .connection()?;
    let count = connection
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DbBackend::Sqlite,
            "SELECT count(*) AS n FROM sentinel",
        ))
        .await
        .map_err(crate::Error::internal)?;
    assert_eq!(
        count
            .ok_or_else(|| crate::Error::custom("missing_test_row", "缺少测试行"))?
            .try_get::<i64>("", "n")
            .map_err(crate::Error::internal)?,
        1
    );
    assert!(
        second
            .connection()?
            .execute_unprepared("SELECT * FROM sentinel")
            .await
            .is_err()
    );
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    // 应用池关闭后，guard 的独立池仍保持该内存库。
    first
        .connection()?
        .execute_unprepared("SELECT * FROM sentinel")
        .await
        .map_err(crate::Error::internal)?;
    first.close().await?;
    second.close().await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn owned_file_persists_across_application_pool_and_is_removed_on_close() -> Result<()> {
    use sea_orm::ConnectionTrait;
    let database = TestDatabase::sqlite_file::<EmptyMigrator>().await?;
    let directory = database
        .sqlite_directory
        .clone()
        .ok_or_else(|| crate::Error::custom("missing_owned_directory", "缺少自有目录"))?;
    let path = directory.join("database.sqlite");
    assert!(path.is_file());
    database
        .connection()?
        .execute_unprepared("CREATE TABLE persistent_sentinel (id INTEGER)")
        .await
        .map_err(crate::Error::internal)?;
    let runtime = app(database.url()).await?;
    runtime
        .container()
        .resolve::<crate::database::Db>()
        .await?
        .connection()?
        .execute_unprepared("INSERT INTO persistent_sentinel VALUES (1)")
        .await
        .map_err(crate::Error::internal)?;
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await?;
    assert!(path.is_file());
    database
        .connection()?
        .execute_unprepared("SELECT * FROM persistent_sentinel")
        .await
        .map_err(crate::Error::internal)?;
    database.close().await?;
    assert!(!directory.exists());
    Ok(())
}

#[tokio::test]
async fn ordinary_file_source_is_refused_without_disclosing_uri() {
    let source =
        "sqlite://private-user:private-password@private-host/private.sqlite?token=private-token";
    let error = TestDatabase::create::<EmptyMigrator>(source).await.err();
    assert!(error.is_some());
    let Some(error) = error else {
        return;
    };
    let message = format!("{error:?}");
    assert!(!message.contains("private-password"));
    assert!(!message.contains("private-token"));
    assert!(!message.contains("private.sqlite"));
}

#[cfg(not(feature = "db-sqlite"))]
#[tokio::test]
async fn feature_disabled_constructors_refuse_before_allocating_files() {
    assert!(
        TestDatabase::sqlite_memory::<EmptyMigrator>()
            .await
            .is_err()
    );
    assert!(TestDatabase::sqlite_file::<EmptyMigrator>().await.is_err());
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn application_pool_keeps_shared_memory_alive_after_guard_close() -> Result<()> {
    use sea_orm::ConnectionTrait;
    let database = TestDatabase::sqlite_memory::<EmptyMigrator>().await?;
    database
        .connection()?
        .execute_unprepared("CREATE TABLE lifetime_sentinel (id INTEGER)")
        .await
        .map_err(crate::Error::internal)?;
    let runtime = app(database.url()).await?;
    let connection = runtime
        .container()
        .resolve::<crate::database::Db>()
        .await?
        .connection()?;
    assert!(!format!("{database:?}").contains(database.url()));
    database.close().await?;
    connection
        .execute_unprepared("INSERT INTO lifetime_sentinel VALUES (1)")
        .await
        .map_err(crate::Error::internal)?;
    runtime
        .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
        .await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn explicit_sqlite_close_failure_does_not_retry_deletion_after_lock_release() -> Result<()> {
    let database = TestDatabase::sqlite_file::<EmptyMigrator>().await?;
    let directory = database
        .sqlite_directory
        .clone()
        .ok_or_else(|| crate::Error::custom("test_directory_missing", "文件测试库缺少自有目录"))?;
    let path = directory.join("database.sqlite");
    let mutex_path = directory.join(format!(".openrust-migration-{}.lock", "0".repeat(64)));
    let mutex = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&mutex_path)
        .map_err(crate::Error::internal)?;
    mutex.lock().map_err(crate::Error::internal)?;
    let outcome = database.close().await;
    // current-thread runtime 在下次 await 前不会运行旧 Drop 另行 spawn 的删除任务。
    mutex.unlock().map_err(crate::Error::internal)?;
    drop(mutex);
    assert_eq!(
        outcome.err().as_ref().map(crate::Error::code),
        Some("sqlite_cleanup_active_migration")
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(path.exists());
    assert!(mutex_path.exists());
    super::cleanup_sqlite_directory(Some(&directory))?;
    assert!(!directory.exists());
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn unpolled_sqlite_close_keeps_its_owned_files_instead_of_scheduling_drop_deletion()
-> Result<()> {
    let database = TestDatabase::sqlite_file::<EmptyMigrator>().await?;
    let directory = database
        .sqlite_directory
        .clone()
        .ok_or_else(|| crate::Error::custom("test_directory_missing", "文件测试库缺少自有目录"))?;
    let connection = database.connection()?;
    let future = database.close();
    drop(future);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(directory.join("database.sqlite").exists());
    connection.close().await.map_err(crate::Error::internal)?;
    super::cleanup_sqlite_directory(Some(&directory))?;
    assert!(!directory.exists());
    Ok(())
}
