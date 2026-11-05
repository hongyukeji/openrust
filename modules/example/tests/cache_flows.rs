//! 命名文章缓存的实际业务与 Redis 网络共享；不是多进程集群验收。
#![cfg(all(
    feature = "auth",
    feature = "queue",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::jobs::article_index::{ARTICLE_STORE, ArticleIndexEntry, article_tag, cache_key};
use super::models::article;
use figment::{Figment, providers::Serialized};
use openrust::cache::{Cache, CacheScope};
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase, TestResponse};
use openrust::{Db, Error, Result};
#[cfg(feature = "cache-redis")]
use std::fmt::Write as _;
#[cfg(feature = "cache-redis")]
use std::sync::Arc;
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
fn sources(database: &str, store: Option<serde_json::Value>, storage: &std::path::Path) -> Figment {
    let mut framework = openrust::config::FrameworkConfig::default();
    framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    let source=Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(serde_json::json!({
        "app":{"env":"test"},"database":{"url":database},
        "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
        "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":storage}}},
        "auth":{"jwt":{"issuer":"http://example.test","audience":"cache-tests","active_key_id":"test","keys":{"test":key}}},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
    })));
    if let Some(store) = store {
        source.merge(Serialized::defaults(
            serde_json::json!({"cache":{"stores":{"example-prefix":store}}}),
        ))
    } else {
        source
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    id: i64,
    storage: tempfile::TempDir,
    token: String,
    other: String,
}
impl Fixture {
    async fn create(store: Option<serde_json::Value>) -> TestResult<Option<Self>> {
        let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
            eprintln!("skipping named article cache DB test: OPENRUST_TEST_DATABASE_URL is unset");
            return Ok(None);
        };
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let storage = tempfile::tempdir()?;
        let app =
            TestApp::<crate::bootstrap::Server>::builder(crate::modules::example::test_sources(
                &(sources(database.url(), store, storage.path())),
            )?)
            .build()
            .await?;
        let db = app.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        let users = db.quoted_table_name("users")?;
        for id in [1_i64, 2] {
            let sql = format!(
                "INSERT INTO {users} (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES ({id},'cache-{id}@example.test','not-used',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)"
            );
            connection
                .execute_raw(Statement::from_string(
                    connection.get_database_backend(),
                    sql,
                ))
                .await?;
        }
        let model = article::ActiveModel {
            owner_id: Set(1),
            title: Set("Original title".into()),
            body: Set("Public body".into()),
            ..Default::default()
        }
        .insert(&connection)
        .await?;
        Ok(Some(Self {
            token: super::selected_test_auth::seeded_password_access(app.app()?, 1).await?,
            other: super::selected_test_auth::seeded_password_access(app.app()?, 2).await?,
            app,
            database,
            id: model.id,
            storage,
        }))
    }
    fn path(&self) -> String {
        format!("/api/example/articles/{}/cached-title", self.id)
    }
    async fn close(mut self) -> TestResult {
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        self.database.close().await?;
        Ok(())
    }
}
fn request(
    method: &str,
    path: &str,
    token: Option<&str>,
    payload: &serde_json::Value,
) -> Result<Request<Body>> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        // 进程内相对 URI 须显式携带 Host，才能通过域路由 authority 校验。
        .header("host", "example.test")
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    request
        .body(Body::from(
            serde_json::to_vec(payload).map_err(Error::internal)?,
        ))
        .map_err(Error::internal)
}
async fn get(
    app: &TestApp<crate::bootstrap::Server>,
    path: &str,
    token: Option<&str>,
) -> Result<TestResponse> {
    app.request(request("GET", path, token, &serde_json::Value::Null)?)
        .await
}

#[tokio::test]
async fn default_named_projection_preserves_default_and_authorizes_before_cache_lookup()
-> TestResult {
    let Some(fixture) = Fixture::create(None).await? else {
        return Ok(());
    };
    let cache = fixture.app.app()?.state().resolve::<Cache>().await?;
    let config = fixture
        .app
        .app()?
        .section::<openrust::config::CacheConfig>("cache")?;
    assert_eq!(config.default, "default");
    assert_eq!(
        cache.store(ARTICLE_STORE).await?.scope(),
        CacheScope::Process
    );
    let first = get(&fixture.app, &fixture.path(), Some(&fixture.token)).await?;
    first.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = first.json()?;
    assert_eq!(body["data"]["cache_hit"], false);
    let second = get(&fixture.app, &fixture.path(), Some(&fixture.token)).await?;
    let body: serde_json::Value = second.json()?;
    assert_eq!(body["data"]["cache_hit"], true);
    // 若越权请求在 DB 授权前解码这个 poisoned key，会报缓存格式错误而非404。
    cache
        .store(ARTICLE_STORE)
        .await?
        .set(&cache_key(2, fixture.id), &"not an ArticleIndexEntry", None)
        .await?;
    get(&fixture.app, &fixture.path(), Some(&fixture.other))
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    get(&fixture.app, &fixture.path(), None)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    assert!(
        cache
            .get::<ArticleIndexEntry>(&cache_key(1, fixture.id))
            .await?
            .is_none()
    );
    fixture.close().await
}

#[tokio::test]
async fn tagged_swr_business_route_uses_job_invalidation_after_committed_update() -> TestResult {
    let Some(fixture) = Fixture::create(None).await? else {
        return Ok(());
    };
    let path = format!("{}/swr", fixture.path());
    let first = get(&fixture.app, &path, Some(&fixture.token)).await?;
    first.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = first.json()?;
    assert_eq!(body["data"]["title"], "Original title");
    assert_eq!(body["data"]["consistency"], "tagged-swr-eventual");
    get(&fixture.app, &path, Some(&fixture.other))
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    get(&fixture.app, &path, None)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    let detail = format!("/api/example/articles/{}", fixture.id);
    fixture
        .app
        .request(request(
            "PUT",
            &detail,
            Some(&fixture.token),
            &serde_json::json!({"title":"Updated by job","body":"Updated body"}),
        )?)
        .await?
        .assert_status(StatusCode::OK)?;
    let stale = get(&fixture.app, &path, Some(&fixture.token)).await?;
    assert_eq!(
        stale.json::<serde_json::Value>()?["data"]["title"],
        "Original title"
    );
    let queue = fixture
        .app
        .app()?
        .state()
        .resolve::<openrust::queue::Queue>()
        .await?;
    assert!(
        queue
            .run_next_with_state("example", fixture.app.app()?.state())
            .await?
    );
    let fresh = get(&fixture.app, &path, Some(&fixture.token)).await?;
    assert_eq!(
        fresh.json::<serde_json::Value>()?["data"]["title"],
        "Updated by job"
    );
    fixture
        .app
        .request(request(
            "DELETE",
            &detail,
            Some(&fixture.token),
            &serde_json::Value::Null,
        )?)
        .await?
        .assert_status(StatusCode::OK)?;
    get(&fixture.app, &path, Some(&fixture.token))
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    fixture.close().await
}

#[cfg(feature = "cache-redis")]
#[tokio::test]
async fn explicit_redis_default_runs_business_swr_without_memory_fallback() -> TestResult {
    use sha2::{Digest, Sha256};

    let Some(fixture) = Fixture::create(None).await? else {
        return Ok(());
    };
    let redis_url = std::env::var("OPENRUST_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce)?;
    let namespace = format!(
        "example-swr-{}",
        nonce.iter().fold(String::new(), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
    );
    let source = sources(fixture.database.url(), None, fixture.storage.path()).merge(
        Serialized::defaults(serde_json::json!({"cache":{"default":"redis","stores":{
            "redis":{"driver":"redis","namespace":namespace,"dsn":redis_url}
        }}})),
    );
    let mut app = TestApp::<crate::bootstrap::Server>::builder(
        crate::modules::example::test_sources(&(source))?,
    )
    .build()
    .await?;
    let cache = app.app()?.state().resolve::<Cache>().await?;
    assert_eq!(
        app.app()?
            .section::<openrust::config::CacheConfig>("cache")?
            .default,
        "redis"
    );
    let store = cache.default_store().await?;
    assert_eq!(store.scope(), CacheScope::Shared);
    assert_eq!(cache.store("default").await?.scope(), CacheScope::Process);
    let key = cache_key(1, fixture.id);
    let tag = article_tag(1, fixture.id);
    let tagged = store.tagged(&[&tag])?;
    let before = tagged.entry(&key).await?;
    let swr_path = format!("{}/swr", fixture.path());
    let first = get(&app, &swr_path, Some(&fixture.token)).await?;
    assert_eq!(
        first.json::<serde_json::Value>()?["data"]["title"],
        "Original title"
    );
    get(&app, &swr_path, Some(&fixture.other))
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    let detail = format!("/api/example/articles/{}", fixture.id);
    fixture
        .app
        .request(request(
            "PUT",
            &detail,
            Some(&fixture.token),
            &serde_json::json!({"title":"Redis SWR updated","body":"Updated body"}),
        )?)
        .await?
        .assert_status(StatusCode::OK)?;
    let before_job = get(&app, &swr_path, Some(&fixture.token)).await?;
    assert_eq!(
        before_job.json::<serde_json::Value>()?["data"]["title"],
        "Original title"
    );
    let queue = app
        .app()?
        .state()
        .resolve::<openrust::queue::Queue>()
        .await?;
    assert!(
        queue
            .run_next_with_state("example", app.app()?.state())
            .await?
    );
    let after = tagged.entry(&key).await?;
    let fresh = get(&app, &swr_path, Some(&fixture.token)).await?;
    assert_eq!(
        fresh.json::<serde_json::Value>()?["data"]["title"],
        "Redis SWR updated"
    );
    before.remove().await?;
    after.remove().await?;
    let material = format!("default:{namespace}::{tag}");
    let digest =
        Sha256::digest(material.as_bytes())
            .iter()
            .fold(String::new(), |mut output, byte| {
                let _ = write!(output, "{byte:02x}");
                output
            });
    let mut connection = redis::Client::open(redis_url)?
        .get_multiplexed_async_connection()
        .await?;
    let _: usize = redis::cmd("DEL")
        .arg(format!("__cache_tag_meta_v1:{digest}"))
        .query_async(&mut connection)
        .await?;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    fixture.close().await
}

#[cfg(feature = "cache-redis")]
#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一安全业务流程的断言、真实资源与回收保持在一个测试生命周期内"
)]
async fn redis_named_projection_is_shared_across_apps_and_rebuilt_after_committed_writes()
-> TestResult {
    let redis = std::env::var("OPENRUST_TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce)?;
    let namespace = format!(
        "example-cache-{}",
        nonce.iter().fold(String::new(), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
    );
    let store = serde_json::json!({"driver":"redis","namespace":namespace,"dsn":redis});
    let Some(fixture) = Fixture::create(Some(store.clone())).await? else {
        return Ok(());
    };
    let mut second =
        TestApp::<crate::bootstrap::Server>::builder(crate::modules::example::test_sources(
            &(sources(fixture.database.url(), Some(store), fixture.storage.path())),
        )?)
        .build()
        .await?;
    let first_cache = fixture.app.app()?.state().resolve::<Cache>().await?;
    let second_cache = second.app()?.state().resolve::<Cache>().await?;
    assert!(!Arc::ptr_eq(&first_cache, &second_cache));
    let first_store = first_cache.store(ARTICLE_STORE).await?;
    let second_store = second_cache.store(ARTICLE_STORE).await?;
    assert_eq!(first_store.scope(), CacheScope::Shared);
    get(&fixture.app, &fixture.path(), Some(&fixture.token))
        .await?
        .assert_status(StatusCode::OK)?;
    let shared = get(&second, &fixture.path(), Some(&fixture.token)).await?;
    let body: serde_json::Value = shared.json()?;
    assert_eq!(body["data"]["cache_hit"], true);
    assert_eq!(body["data"]["title"], "Original title");
    get(&second, &fixture.path(), Some(&fixture.other))
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    // 同一个 named Redis store 仍由框架按执行中的 tenant 分区，不自行拼接未可信租户键。
    let tenant_key = cache_key(1, fixture.id);
    openrust::tenant::TenantContext::scope(
        openrust::tenant::TenantId::new("article-cache-tenant-a")?,
        async {
            first_store
                .set(
                    &tenant_key,
                    &ArticleIndexEntry {
                        title: "Tenant A".into(),
                    },
                    Some(Duration::from_secs(60)),
                )
                .await?;
            assert_eq!(
                second_store
                    .get::<ArticleIndexEntry>(&tenant_key)
                    .await?
                    .ok_or_else(|| Error::custom("test_cache_missing", "tenant cache missing"))?
                    .title,
                "Tenant A"
            );
            Ok::<_, Error>(())
        },
    )
    .await??;
    openrust::tenant::TenantContext::scope(
        openrust::tenant::TenantId::new("article-cache-tenant-b")?,
        async {
            assert!(
                second_store
                    .get::<ArticleIndexEntry>(&tenant_key)
                    .await?
                    .is_none()
            );
            Ok::<_, Error>(())
        },
    )
    .await??;
    let detail = format!("/api/example/articles/{}", fixture.id);
    let updated = fixture
        .app
        .request(request(
            "PUT",
            &detail,
            Some(&fixture.token),
            &serde_json::json!({"title":"Updated title","body":"Updated body"}),
        )?)
        .await?;
    assert_eq!(
        updated.status(),
        StatusCode::OK,
        "update code: {}",
        updated.json::<serde_json::Value>()?["code"]
    );
    let stale = get(&second, &fixture.path(), Some(&fixture.token)).await?;
    assert_eq!(
        stale.json::<serde_json::Value>()?["data"]["title"],
        "Original title"
    );
    let queue = second
        .app()?
        .state()
        .resolve::<openrust::queue::Queue>()
        .await?;
    assert!(
        queue
            .run_next_with_state("example", second.app()?.state())
            .await?
    );
    let fresh = get(&second, &fixture.path(), Some(&fixture.token)).await?;
    assert_eq!(
        fresh.json::<serde_json::Value>()?["data"]["title"],
        "Updated title"
    );
    first_store.remove(&cache_key(1, fixture.id)).await?;
    let rebuilt = get(&second, &fixture.path(), Some(&fixture.token)).await?;
    assert_eq!(
        rebuilt.json::<serde_json::Value>()?["data"]["cache_hit"],
        false
    );
    let deleted = fixture
        .app
        .request(request(
            "DELETE",
            &detail,
            Some(&fixture.token),
            &serde_json::Value::Null,
        )?)
        .await?;
    deleted.assert_status(StatusCode::OK)?;
    assert_eq!(
        deleted.json::<serde_json::Value>()?["data"]["deleted"],
        true
    );
    // 尚未消费删除 job 时缓存仍在，但 DB 权限/存在性检查已经拒绝读取。
    assert!(
        second_store
            .get::<ArticleIndexEntry>(&tenant_key)
            .await?
            .is_some()
    );
    get(&second, &fixture.path(), Some(&fixture.token))
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    assert!(
        queue
            .run_next_with_state("example", second.app()?.state())
            .await?
    );
    assert!(
        second_store
            .get::<ArticleIndexEntry>(&tenant_key)
            .await?
            .is_none()
    );
    openrust::tenant::TenantContext::scope(
        openrust::tenant::TenantId::new("article-cache-tenant-a")?,
        async { first_store.remove(&tenant_key).await },
    )
    .await??;
    second
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    fixture.close().await
}

#[tokio::test]
async fn explicit_unknown_named_driver_is_not_overwritten_by_example_default() -> TestResult {
    let Some(fixture) = Fixture::create(None).await? else {
        return Ok(());
    };
    for store in [
        serde_json::json!({"driver":"nonexistent-example-driver"}),
        serde_json::json!({"driver":"example-prefix","key_prefix":""}),
    ] {
        let result =
            TestApp::<crate::bootstrap::Server>::builder(crate::modules::example::test_sources(
                &(sources(fixture.database.url(), Some(store), fixture.storage.path())),
            )?)
            .build()
            .await;
        assert!(result.is_err());
    }
    fixture.close().await
}

#[cfg(feature = "cache-redis")]
#[tokio::test]
async fn unavailable_explicit_default_redis_does_not_become_example_memory_store() -> TestResult {
    let Some(fixture) = Fixture::create(None).await? else {
        return Ok(());
    };
    let source=sources(fixture.database.url(),Some(serde_json::json!({"driver":"redis","namespace":"example-closed-redis","dsn":"redis://127.0.0.1:1/"})),fixture.storage.path())
        .merge(Serialized::defaults(serde_json::json!({"cache":{"default":"example-prefix"}})));
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        TestApp::<crate::bootstrap::Server>::builder(crate::modules::example::test_sources(
            &(source),
        )?)
        .build(),
    )
    .await?;
    assert!(result.is_err());
    fixture.close().await
}
