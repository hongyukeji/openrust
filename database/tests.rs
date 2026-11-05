//! 应用升级走真实迁移命令，覆盖旧数据与回滚拒绝边界。

use openrust::config::{FrameworkConfig, NoConfig};
use openrust::core::{Application, DefaultProviders, ServiceProvider};
use openrust::db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use openrust::db::{DatabaseServiceProvider, FrameworkMigrator, MigrationServiceProvider};
use openrust::testing::{TestApp, TestDatabase};
use openrust::{Error, Result};

struct MigrationApp;
impl Application for MigrationApp {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        DefaultProviders::empty()
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        vec![
            Box::new(DatabaseServiceProvider),
            Box::new(MigrationServiceProvider::<super::Migrator>::new()),
        ]
    }
}

async fn migrate(database: &TestDatabase, args: Vec<String>) -> Result<()> {
    let sources = figment::Figment::from(figment::providers::Serialized::defaults(
        FrameworkConfig::default(),
    ))
    .merge(figment::providers::Serialized::defaults(
        serde_json::json!({
            "database":{"url":database.url(),"max_connections":2,"min_connections":0}
        }),
    ));
    TestApp::<MigrationApp>::builder(sources)
        .run_command("migrate", args)
        .await
}

#[tokio::test]
async fn optional_identifiers_preserve_accounts_and_refuse_lossy_rollback() -> Result<()> {
    let database = TestDatabase::sqlite_file::<FrameworkMigrator>().await?;
    let result: Result<()> = async {
        let connection = database.connection()?;
        connection.execute_unprepared("INSERT INTO users (id,email,password_hash,created_at,updated_at) VALUES (1,'old@example.test','kept-hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.map_err(Error::internal)?;
        migrate(&database, vec![]).await?;
        // 本用例未建立authority证明，先回退空挑战、凭据context及authority元数据三项迁移，再验证账号字段回退。
        migrate(&database, vec!["down".into(), "3".into()]).await?;
        let row = connection.query_one_raw(Statement::from_string(DbBackend::Sqlite,
            "SELECT email,password_hash,username,phone FROM users WHERE id=1"))
            .await.map_err(Error::internal)?.ok_or(Error::NotFound("原账户丢失".into()))?;
        assert_eq!(row.try_get::<String>("", "email").map_err(Error::internal)?, "old@example.test");
        assert_eq!(row.try_get::<String>("", "password_hash").map_err(Error::internal)?, "kept-hash");
        assert_eq!(row.try_get::<Option<String>>("", "username").map_err(Error::internal)?, None);
        assert_eq!(row.try_get::<Option<String>>("", "phone").map_err(Error::internal)?, None);
        connection.execute_unprepared("INSERT INTO users (id,email,username,password_hash,created_at,updated_at) VALUES (2,NULL,'user-only','kept-hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP), (3,NULL,NULL,'kept-hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.map_err(Error::internal)?;
        assert!(connection.execute_unprepared("INSERT INTO users (id,email,username,password_hash,created_at,updated_at) VALUES (4,NULL,'user-only','kept-hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.is_err());
        assert!(migrate(&database, vec!["down".into(),"1".into()]).await.is_err());
        let row = connection.query_one_raw(Statement::from_string(DbBackend::Sqlite,
            "SELECT username,email FROM users WHERE id=2"))
            .await.map_err(Error::internal)?.ok_or(Error::NotFound("回滚失败损坏账户".into()))?;
        assert_eq!(row.try_get::<String>("", "username").map_err(Error::internal)?, "user-only");
        assert_eq!(row.try_get::<Option<String>>("", "email").map_err(Error::internal)?, None);
        connection.execute_unprepared("UPDATE users SET email='restored-' || id || '@example.test' WHERE email IS NULL").await.map_err(Error::internal)?;
        assert!(migrate(&database, vec!["down".into(), "1".into()]).await.is_err());
        // 应用显式迁出/退役新增登录标识后，才允许删除这些字段。
        connection.execute_unprepared("UPDATE users SET username=NULL,phone=NULL").await.map_err(Error::internal)?;
        migrate(&database, vec!["down".into(),"1".into()]).await?;
        assert!(connection.execute_unprepared("INSERT INTO users (id,email,password_hash,created_at,updated_at) VALUES (5,NULL,'hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await.is_err());
        migrate(&database, vec![]).await?;
        let row = connection.query_one_raw(Statement::from_string(DbBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM users"))
            .await.map_err(Error::internal)?.ok_or(Error::NotFound("账户数量缺失".into()))?;
        assert_eq!(row.try_get::<i64>("", "count").map_err(Error::internal)?, 3);
        Ok(())
    }.await;
    let closed = database.close().await;
    result?;
    closed
}
