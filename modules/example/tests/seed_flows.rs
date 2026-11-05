//! 生产种子命令的显式执行、主库原子性与非覆盖幂等；真实双库由运行环境提供。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use super::database::seeders::SEED_ARTICLE_IDS;
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
        let mut migrations = super::selected_test_auth::migrations();
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
                "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
                "auth":{"jwt":{"issuer":"http://example.test","audience":"seed-tests","active_key_id":"test","keys":{"test":key}}},
                "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
            })));
        let app = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        )
        .build()
        .await?;
        Ok(Some(Self { app, database }))
    }
    async fn seed(&self) -> openrust::Result<()> {
        self.app.app()?.run_command("db", vec!["seed".into()]).await
    }
    async fn user_count(&self) -> TestResult<i64> {
        let db = self.app.app()?.state().resolve::<Db>().await?;
        let backend = db.backend()?;
        let users = db.quoted_table_name("users")?;
        let row = db
            .connection()?
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!("SELECT COUNT(*) AS count FROM {users}"),
                [],
            ))
            .await?
            .ok_or("user count query returned no row")?;
        Ok(row.try_get("", "count")?)
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
    let users_before = fixture.user_count().await?;
    fixture.seed().await?;
    assert_eq!(article::Entity::find().count(&connection).await?, 2);
    let first = article::Entity::find_by_id(SEED_ARTICLE_IDS[0])
        .one(&connection)
        .await?
        .ok_or("seed article missing")?;
    let owner = first.owner_id;
    assert_eq!(owner, -7_300_001);
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
    assert_eq!(fixture.user_count().await?, users_before);
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
        fixture.user_count().await? == 0,
        "example seeding must not create framework auth users"
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
async fn seed_does_not_reserve_or_modify_framework_auth_accounts() -> TestResult {
    let Some(fixture) = Fixture::create().await? else {
        return Ok(());
    };
    let auth = fixture.app.app()?.state().resolve::<AuthFlows>().await?;
    let password = "an explicit test-only account password";
    let code = auth
        .register("example-seed@seed.invalid", password, "seed-collision")
        .await?
        .ok_or("verification code missing")?;
    assert!(
        auth.verify_email("example-seed@seed.invalid", code.code(), "seed-collision")
            .await?
    );
    let original = auth
        .login("example-seed@seed.invalid", password, "seed-collision")
        .await?;
    fixture.seed().await?;
    let retained = auth
        .login("example-seed@seed.invalid", password, "seed-collision")
        .await?;
    assert_eq!(retained.user_id(), original.user_id());
    let db = fixture.app.app()?.state().resolve::<Db>().await?;
    assert_eq!(article::Entity::find().count(&db.connection()?).await?, 2);
    drop(db);
    drop(auth);
    fixture.close().await
}
