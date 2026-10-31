//! 公开 Cache API 的文章派生数据验收；当前 cached-title 生产路由仍单独验证 owner 授权。
//! Redis 跨应用 generation 共享不代表强一致，singleflight 只证明同应用加载合并。
#![cfg(all(
    feature = "auth",
    feature = "queue",
    feature = "cache-redis",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::jobs::article_index::{ARTICLE_STORE, ArticleIndexEntry, cache_key};
use super::models::article;
use figment::{Figment, providers::Serialized};
use openrust::auth::JwtTokens;
use openrust::cache::{Cache, CacheStore, TaggedCacheEntry};
use openrust::db::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter,
    Statement,
};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase};
use openrust::{Db, Error, Result};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Notify, Semaphore};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const TAG: &str = "example-article-titles";
const LOAD: &str = "example-owner-title-v1";
const TTL: Duration = Duration::from_secs(10);
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
    a: TestApp<crate::bootstrap::Server>,
    b: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    _storage: tempfile::TempDir,
    id: i64,
    owner: String,
    other: String,
    namespace: String,
    redis: redis::aio::MultiplexedConnection,
    entries: Vec<TaggedCacheEntry>,
}
impl Fixture {
    async fn create() -> TestResult<Self> {
        let admin = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
        let dsn = std::env::var("OPENRUST_TEST_REDIS_URL")?;
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce)?;
        let mut suffix = String::with_capacity(32);
        for byte in nonce {
            std::fmt::Write::write_fmt(&mut suffix, format_args!("{byte:02x}"))?;
        }
        let namespace = format!("article-behavior-{suffix}");
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let storage = tempfile::tempdir()?;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let mut framework = openrust::config::FrameworkConfig::default();
        framework.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let source=Figment::from(Serialized::defaults(framework)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"},"database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"},"example-prefix":{"driver":"redis","namespace":namespace,"dsn":dsn}}},
            "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":storage.path()}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"cache-behavior","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        })));
        let a = TestApp::<crate::bootstrap::Server>::builder(source.clone())
            .build()
            .await?;
        let b = TestApp::<crate::bootstrap::Server>::builder(source)
            .build()
            .await?;
        let db = a.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        let users = db.quoted_table_name("users")?;
        for id in [1_i64, 2] {
            connection.execute_raw(Statement::from_string(connection.get_database_backend(),format!("INSERT INTO {users} (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES ({id},'cache-behavior-{id}@example.test','unused',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)"))).await?;
        }
        let article = article::ActiveModel {
            owner_id: Set(1),
            title: Set("旧标题".into()),
            body: Set("缓存不负责授权".into()),
            ..Default::default()
        }
        .insert(&connection)
        .await?;
        let jwt = a.app()?.state().resolve::<JwtTokens>().await?;
        let redis = redis::Client::open(dsn)?
            .get_multiplexed_async_connection()
            .await?;
        Ok(Self {
            owner: jwt.issue_access_token(1)?,
            other: jwt.issue_access_token(2)?,
            a,
            b,
            database,
            _storage: storage,
            id: article.id,
            namespace,
            redis,
            entries: Vec::new(),
        })
    }
    async fn db(&self) -> Result<Arc<Db>> {
        self.a.app()?.state().resolve::<Db>().await
    }
    async fn store(app: &TestApp<crate::bootstrap::Server>) -> Result<CacheStore> {
        app.app()?
            .state()
            .resolve::<Cache>()
            .await?
            .store(ARTICLE_STORE)
            .await
    }
    fn key(&self) -> String {
        cache_key(1, self.id)
    }
    async fn update(&self, title: &str) -> TestResult {
        article::ActiveModel {
            id: Set(self.id),
            title: Set(title.into()),
            ..Default::default()
        }
        .update(&self.db().await?.primary_connection()?)
        .await?;
        Ok(())
    }
    async fn authorization(&self) -> TestResult {
        for app in [&self.a, &self.b] {
            for (token, status) in [
                (&self.owner, StatusCode::OK),
                (&self.other, StatusCode::NOT_FOUND),
            ] {
                let request = Request::builder()
                    .uri(format!("/api/example/articles/{}/cached-title", self.id))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())?;
                app.request(request).await?.assert_status(status)?;
            }
        }
        Ok(())
    }
    async fn remember_entry(&mut self, store: &CacheStore) -> TestResult {
        self.entries
            .push(store.tagged(&[TAG])?.entry(&self.key()).await?);
        Ok(())
    }
    async fn close(mut self) -> TestResult {
        self.a
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        self.b
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        for entry in &self.entries {
            entry.remove().await?;
        }
        // generation 元数据按设计无 TTL；只删除本随机 namespace 的确切元数据 key。
        let material = format!("{ARTICLE_STORE}:{}::{TAG}", self.namespace);
        let digest = format!("{:x}", Sha256::digest(material.as_bytes()));
        let _: usize = redis::cmd("DEL")
            .arg(format!("__openrust_cache_tag_meta_v1:{digest}"))
            .arg(format!("{}:{}", self.namespace, self.key()))
            .query_async(&mut self.redis)
            .await?;
        self.database.close().await?;
        Ok(())
    }
}
async fn load(db: Arc<Db>, owner: i64, id: i64) -> Result<ArticleIndexEntry> {
    let model = article::Entity::find_by_id(id)
        .filter(article::Column::OwnerId.eq(owner))
        .one(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::NotFound("article not found".into()))?;
    Ok(ArticleIndexEntry { title: model.title })
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 Redis，显式配置 OPENRUST_TEST_DATABASE_URL/OPENRUST_TEST_REDIS_URL"]
async fn article_tags_share_invalidation_and_reject_old_loader_across_apps() -> TestResult {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut f = Fixture::create().await?;
        f.authorization().await?;
        let a = Fixture::store(&f.a).await?;
        let b = Fixture::store(&f.b).await?;
        f.remember_entry(&a).await?;
        let captured = a.tagged(&[TAG])?.entry(&f.key()).await?;
        let old = load(f.db().await?, 1, f.id).await?;
        captured.set(&old, TTL).await?;
        assert_eq!(
            b.tagged(&[TAG])?
                .get::<ArticleIndexEntry>(&f.key())
                .await?
                .ok_or("shared tag value absent")?
                .title,
            "旧标题"
        );
        f.update("新标题").await?;
        b.invalidate_tags(&[TAG]).await?;
        assert_eq!(
            captured
                .set(&old, TTL)
                .await
                .err()
                .ok_or("old loader unexpectedly published")?
                .code(),
            "cache_tag_invalidated"
        );
        assert!(
            a.tagged(&[TAG])?
                .get::<ArticleIndexEntry>(&f.key())
                .await?
                .is_none()
        );
        f.remember_entry(&b).await?;
        b.tagged(&[TAG])?
            .set(&f.key(), &load(f.db().await?, 1, f.id).await?, TTL)
            .await?;
        assert_eq!(
            a.tagged(&[TAG])?
                .get::<ArticleIndexEntry>(&f.key())
                .await?
                .ok_or("new generation missing")?
                .title,
            "新标题"
        );
        f.authorization().await?;
        f.close().await
    })
    .await?
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 Redis，显式配置 OPENRUST_TEST_DATABASE_URL/OPENRUST_TEST_REDIS_URL"]
async fn article_swr_returns_stale_then_refreshes_and_honors_tag_invalidation() -> TestResult {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut f = Fixture::create().await?;
        f.authorization().await?;
        let a = Fixture::store(&f.a).await?;
        let b = Fixture::store(&f.b).await?;
        f.remember_entry(&a).await?;
        let fresh = Duration::from_millis(200);
        let stale = Duration::from_secs(4);
        let db = f.db().await?;
        let id = f.id;
        let first = a
            .tagged(&[TAG])?
            .get_stale_while_revalidate(&f.key(), LOAD, fresh, stale, move || {
                let db = Arc::clone(&db);
                async move { load(db, 1, id).await }
            })
            .await?;
        assert_eq!(first.title, "旧标题");
        f.update("后台刷新标题").await?;
        tokio::time::sleep(Duration::from_millis(250)).await;
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Semaphore::new(0));
        let db = f.db().await?;
        let entered_loader = Arc::clone(&entered);
        let release_loader = Arc::clone(&release);
        let value = tokio::time::timeout(
            Duration::from_secs(1),
            b.tagged(&[TAG])?
                .get_stale_while_revalidate(&f.key(), LOAD, fresh, stale, move || {
                    let db = Arc::clone(&db);
                    let entered = Arc::clone(&entered_loader);
                    let release = Arc::clone(&release_loader);
                    async move {
                        entered.notify_one();
                        let _permit = release.acquire().await.map_err(Error::internal)?;
                        load(db, 1, id).await
                    }
                }),
        )
        .await??;
        assert_eq!(value.title, "旧标题");
        tokio::time::timeout(Duration::from_secs(3), entered.notified()).await?;
        release.add_permits(1);
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                // SWR 保存包装值；只读观察 B 的后台写入，不启动 A 的替代 loader。
                let value = a.tagged(&[TAG])?.get::<serde_json::Value>(&f.key()).await?;
                if value
                    .as_ref()
                    .and_then(|entry| entry.pointer("/value/title"))
                    .and_then(serde_json::Value::as_str)
                    == Some("后台刷新标题")
                {
                    break Ok::<(), Error>(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await??;
        f.update("失效后新标题").await?;
        a.invalidate_tags(&[TAG]).await?;
        f.remember_entry(&a).await?;
        let db = f.db().await?;
        let new = b
            .tagged(&[TAG])?
            .get_stale_while_revalidate(&f.key(), LOAD, fresh, stale, move || {
                let db = Arc::clone(&db);
                async move { load(db, 1, id).await }
            })
            .await?;
        assert_eq!(new.title, "失效后新标题");
        f.authorization().await?;
        f.close().await
    })
    .await?
}

#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 Redis，显式配置 OPENRUST_TEST_DATABASE_URL/OPENRUST_TEST_REDIS_URL"]
async fn article_concurrent_misses_merge_one_app_loader() -> TestResult {
    tokio::time::timeout(Duration::from_secs(20), async {
        let f = Fixture::create().await?;
        let store = Fixture::store(&f.a).await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Semaphore::new(0));
        let polled = Arc::new(AtomicUsize::new(0));
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..8 {
            let store = store.clone();
            let key = f.key();
            let db = f.db().await?;
            let id = f.id;
            let calls = Arc::clone(&calls);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let polled = Arc::clone(&polled);
            tasks.spawn(async move {
                let operation = store.get_or_load_with(&key, LOAD, Some(TTL), move || {
                    let db = Arc::clone(&db);
                    let calls = Arc::clone(&calls);
                    let entered = Arc::clone(&entered);
                    let release = Arc::clone(&release);
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        entered.notify_one();
                        let _permit = release.acquire().await.map_err(Error::internal)?;
                        load(db, 1, id).await
                    }
                });
                let mut operation = Box::pin(operation);
                let mut counted = false;
                futures_util::future::poll_fn(|context| {
                    let state = operation.as_mut().poll(context);
                    if !counted {
                        counted = true;
                        polled.fetch_add(1, Ordering::SeqCst);
                    }
                    state
                })
                .await
            });
        }
        tokio::time::timeout(Duration::from_secs(3), entered.notified()).await?;
        tokio::time::timeout(Duration::from_secs(3), async {
            while polled.load(Ordering::SeqCst) != 8 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        assert_eq!(tasks.len(), 8);
        release.add_permits(1);
        while let Some(result) =
            tokio::time::timeout(Duration::from_secs(3), tasks.join_next()).await?
        {
            assert_eq!(result??.title, "旧标题");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let b = Fixture::store(&f.b).await?;
        assert_eq!(
            b.get::<ArticleIndexEntry>(&f.key())
                .await?
                .ok_or("shared derived title missing")?
                .title,
            "旧标题"
        );
        f.authorization().await?;
        f.close().await
    })
    .await?
}
