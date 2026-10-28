//! 生产种子命令的显式执行、主库原子性与非覆盖幂等；真实双库由运行环境提供。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use super::database::seeders::{SEED_ARTICLE_IDS, SEED_EMAIL};
use super::models::article;
use figment::{Figment, providers::Serialized};
use openrust::Db;
use openrust::auth::AuthFlows;
use openrust::db::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, EntityTrait, IntoActiveModel,
    PaginatorTrait, Statement,
};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::testing::{TestApp, TestDatabase};
use std::time::Duration;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
}
impl Fixture {
    async fn create() -> TestResult<Option<Self>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping real example seed test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
            .merge(Serialized::defaults(serde_json::json!({
                "app":{"env":"test"}, "database":{"url":database.url()},
                "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
                "auth":{"jwt":{"issuer":"http://example.test","audience":"seed-tests","active_key_id":"test","keys":{"test":key}}},
                "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
            })));
        let app = TestApp::<crate::bootstrap::Server>::builder(source)
            .build()
            .await?;
        Ok(Some(Self { app, database }))
    }
    async fn seed(&self) -> openrust::Result<()> {
        self.app.app()?.run_command("db", vec!["seed".into()]).await
    }
    async fn seed_owner_exists(&self) -> TestResult<bool> {
        let db = self.app.app()?.state().resolve::<Db>().await?;
        let backend = db.backend()?;
        let users = db.quoted_table_name("users")?;
        let placeholder = if backend == openrust::db::sea_orm::DbBackend::Postgres {
            "$1"
        } else {
            "?"
        };
        Ok(db
            .connection()?
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!("SELECT id FROM {users} WHERE email = {placeholder}"),
                [SEED_EMAIL.into()],
            ))
            .await?
            .is_some())
    }
    async fn close(mut self) -> TestResult {
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
            .await?;
        self.database.close().await?;
        Ok(())
    }
}

#[tokio::test]
async fn explicit_seed_command_is_idempotent_preserves_edits_and_never_creates_login() -> TestResult
{
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let connection = db.connection()?;
    assert_eq!(
        article::Entity::find().count(&connection).await?,
        0,
        "bootstrap must not seed"
    );
    assert!(!fixture.seed_owner_exists().await?);
    fixture.seed().await?;
    assert_eq!(article::Entity::find().count(&connection).await?, 2);
    let first = article::Entity::find_by_id(SEED_ARTICLE_IDS[0])
        .one(&connection)
        .await?
        .ok_or("seed article missing")?;
    let owner = first.owner_id;
    assert!(owner > 0);
    let mut edited = first.into_active_model();
    edited.body = Set("由开发者修改的正文，重新执行种子不得覆盖".into());
    edited.update(&connection).await?;
    // 同一 Provider/生产命令入口并发执行也只贡献同一组隔离数据。
    let (first, second) = tokio::join!(fixture.seed(), fixture.seed());
    first?;
    second?;
    assert_eq!(article::Entity::find().count(&connection).await?, 2);
    let edited = article::Entity::find_by_id(SEED_ARTICLE_IDS[0])
        .one(&connection)
        .await?
        .ok_or("edited seed missing")?;
    assert_eq!(edited.body, "由开发者修改的正文，重新执行种子不得覆盖");
    assert_eq!(edited.owner_id, owner);
    let auth = fixture.app.app()?.state().resolve::<AuthFlows>().await?;
    let denied = auth
        .login(SEED_EMAIL, "!openrust-example-seed-disabled", "seed-test")
        .await;
    assert!(matches!(denied, Err(openrust::Error::Unauthorized)));
    drop(auth);
    drop(connection);
    drop(db);
    fixture.close().await
}

#[tokio::test]
async fn seed_id_collision_rolls_back_new_identity_without_overwriting_user_article() -> TestResult
{
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    let connection = db.connection()?;
    article::Entity::insert(article::ActiveModel {
        id: Set(SEED_ARTICLE_IDS[1]),
        owner_id: Set(123_456),
        title: Set("现有业务文章".into()),
        body: Set("不能被种子改写".into()),
        private_note_ciphertext: Set(None),
    })
    .exec_without_returning(&connection)
    .await?;
    let error = fixture
        .seed()
        .await
        .err()
        .ok_or("seed collision accepted")?;
    assert_eq!(error.code(), "example_seed_conflict");
    assert!(
        !fixture.seed_owner_exists().await?,
        "identity must roll back with articles"
    );
    assert!(
        article::Entity::find_by_id(SEED_ARTICLE_IDS[0])
            .one(&connection)
            .await?
            .is_none()
    );
    let existing = article::Entity::find_by_id(SEED_ARTICLE_IDS[1])
        .one(&connection)
        .await?
        .ok_or("original article lost")?;
    assert_eq!(existing.owner_id, 123_456);
    assert_eq!(existing.body, "不能被种子改写");
    drop(connection);
    drop(db);
    fixture.close().await
}

#[tokio::test]
async fn seed_reserved_email_collision_preserves_the_existing_account() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let auth = fixture.app.app()?.state().resolve::<AuthFlows>().await?;
    let password = "an explicit test-only account password";
    let code = auth
        .register(SEED_EMAIL, password, "seed-collision")
        .await?
        .ok_or("verification code missing")?;
    assert!(
        auth.verify_email(SEED_EMAIL, code.code(), "seed-collision")
            .await?
    );
    let original = auth.login(SEED_EMAIL, password, "seed-collision").await?;
    let error = fixture
        .seed()
        .await
        .err()
        .ok_or("reserved email collision accepted")?;
    assert_eq!(error.code(), "example_seed_conflict");
    let retained = auth.login(SEED_EMAIL, password, "seed-collision").await?;
    assert_eq!(retained.user_id(), original.user_id());
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    assert_eq!(article::Entity::find().count(&db.connection()?).await?, 0);
    drop(db);
    drop(auth);
    fixture.close().await
}
