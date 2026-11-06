//! 模块就绪探针实际检查文章表；缺迁移不能报告依赖健康。

use std::time::Duration;

use figment::{Figment, providers::Serialized};
use openrust::database::sea_orm::ConnectionTrait;
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::testing::{TestApp, TestDatabase};

struct Migrator;

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}

#[tokio::test]
async fn module_readiness_requires_its_article_table() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping example readiness DB test: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&admin).await?;
    let mut defaults = openrust::config::FrameworkConfig::default();
    defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    let source = Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(
        serde_json::json!({
            "app":{"env":"test"},
            "database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"readiness-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        }),
    ));
    let mut app = TestApp::<crate::bootstrap::Server>::builder(
        crate::modules::example::test_sources(&(source))?,
    )
    .build()
    .await?;
    let readiness = app
        .app()?
        .state()
        .resolve::<openrust::core::Readiness>()
        .await?;
    assert!(readiness.dependencies_ready().await);
    let db = app.app()?.state().resolve::<openrust::Db>().await?;
    db.primary_connection()?
        .execute_unprepared("DROP TABLE example_articles")
        .await?;
    assert!(!readiness.dependencies_ready().await);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    database.close().await?;
    Ok(())
}
