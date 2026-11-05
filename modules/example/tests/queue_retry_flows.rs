//! 现有文章索引任务的真实数据库/Redis 失败重试；租户证明限任务元数据与缓存分区。
#![cfg(all(
    feature = "auth",
    feature = "queue-redis",
    feature = "cache-redis",
    feature = "storage-local",
    any(feature = "db-postgres", feature = "db-mysql")
))]

use super::jobs::article_index::{ARTICLE_STORE, ArticleIndexEntry, ArticleIndexJob, cache_key};
use super::models::article;
use figment::{Figment, providers::Serialized};
use openrust::cache::Cache;
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::queue::{DispatchOptions, Job, Queue};
use openrust::tenant::{TenantContext, TenantId};
use openrust::testing::{TestApp, TestDatabase};
use openrust::{Db, Error, Result};
use serde_json::Value;
use std::{sync::Arc, time::Duration};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const TENANT: &str = "article-queue-retry";
const QUEUE: &str = "article-retry";
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
    _storage: tempfile::TempDir,
    redis: redis::aio::MultiplexedConnection,
    namespace: String,
    redis_queue: bool,
    id: i64,
}
impl Fixture {
    async fn create(redis_queue: bool) -> TestResult<Self> {
        let admin = std::env::var("OPENRUST_TEST_DATABASE_URL")?;
        let redis_url = std::env::var("OPENRUST_TEST_REDIS_URL")?;
        let mut nonce = [0_u8; 16];
        getrandom::fill(&mut nonce)?;
        let mut suffix = String::with_capacity(32);
        for byte in nonce {
            std::fmt::Write::write_fmt(&mut suffix, format_args!("{byte:02x}"))?;
        }
        let namespace = format!("article-retry-{suffix}");
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let storage = tempfile::tempdir()?;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let config = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default())).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"}, "database":{"url":database.url()},
            "queue":{"default":if redis_queue {"redis"} else {"database"},"connections":{"database":{"driver":"database"},"redis":{"driver":"redis","redis_url":redis_url,"redis_namespace":namespace}}},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"},"example-prefix":{"driver":"redis","namespace":namespace,"dsn":redis_url}}},
            "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":storage.path()}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"queue-retry","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(config))?,
        )
        .build()
        .await?;
        let db = app.app()?.state().resolve::<Db>().await?;
        let article = article::ActiveModel {
            owner_id: Set(1),
            title: Set("真实重试后的文章索引".into()),
            body: Set("恢复数据库后读取".into()),
            ..Default::default()
        }
        .insert(&db.primary_connection()?)
        .await?;
        let redis = redis::Client::open(redis_url)?
            .get_multiplexed_async_connection()
            .await?;
        Ok(Self {
            app,
            database,
            _storage: storage,
            redis,
            namespace,
            redis_queue,
            id: article.id,
        })
    }
    async fn queue(&self) -> Result<Arc<Queue>> {
        self.app.app()?.state().resolve::<Queue>().await
    }
    async fn dispatch(&self, attempts: u32) -> TestResult<String> {
        let queue = self.queue().await?;
        Ok(TenantContext::scope(TenantId::new(TENANT)?, async {
            queue
                .dispatch_with_options(
                    QUEUE,
                    ArticleIndexJob::NAME,
                    &ArticleIndexJob {
                        owner_id: 1,
                        article_id: self.id,
                        action: "updated".into(),
                    },
                    DispatchOptions {
                        max_attempts: attempts,
                        delay: Duration::ZERO,
                        unique_key: None,
                    },
                )
                .await
        })
        .await??)
    }
    async fn consume(&self) -> Result<bool> {
        self.queue()
            .await?
            .run_next_with_state(QUEUE, self.app.app()?.state())
            .await
    }
    async fn snapshot(&self, id: &str, failed: bool) -> TestResult<Option<Value>> {
        if self.redis_queue {
            let mut connection = self.redis.clone();
            let raw: Option<String> = redis::cmd("HGET")
                .arg(format!(
                    "{{openrust:{}}}:{}",
                    self.namespace,
                    if failed { "failed" } else { "jobs" }
                ))
                .arg(id)
                .query_async(&mut connection)
                .await?;
            return raw
                .map(|json| serde_json::from_str(&json).map_err(Into::into))
                .transpose();
        }
        let db = self.app.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        let table = db.quoted_table_name(if failed { "failed_jobs" } else { "jobs" })?;
        let backend = connection.get_database_backend();
        let parameter = if backend == openrust::db::sea_orm::DbBackend::Postgres {
            "$1"
        } else {
            "?"
        };
        let fields = if failed {
            "attempts, payload, error AS error_code".to_owned()
        } else if backend == openrust::db::sea_orm::DbBackend::Postgres {
            "attempts, payload, available_at::text AS available_at".to_owned()
        } else {
            "attempts, payload, CAST(available_at AS CHAR) AS available_at".to_owned()
        };
        let row = connection
            .query_one_raw(Statement::from_sql_and_values(
                backend,
                format!("SELECT {fields} FROM {table} WHERE id = {parameter}"),
                vec![id.into()],
            ))
            .await?;
        row.map(|row| {
            let mut value = serde_json::json!({"attempts":row.try_get::<i64>("","attempts")?,"payload":row.try_get::<Value>("","payload")?});
            let field = if failed { "error_code" } else { "available_at" };
            value[field] = Value::String(row.try_get::<String>("",field)?);
            Ok(value)
        }).transpose()
    }
    async fn assert_cache(&self, present: bool) -> TestResult {
        let cache = self.app.app()?.state().resolve::<Cache>().await?;
        let store = cache.store(ARTICLE_STORE).await?;
        let entry = TenantContext::scope(TenantId::new(TENANT)?, async {
            store.get::<ArticleIndexEntry>(&cache_key(1, self.id)).await
        })
        .await??;
        if present {
            assert_eq!(
                entry.ok_or("retry did not rebuild index")?.title,
                "真实重试后的文章索引"
            );
        } else {
            assert!(entry.is_none());
        }
        // worker 自行恢复派发时 tenant；此处未替 worker 手工包裹上下文。
        assert!(
            store
                .get::<ArticleIndexEntry>(&cache_key(1, self.id))
                .await?
                .is_none()
        );
        Ok(())
    }
    async fn remove_cache(&self) -> TestResult {
        let cache = self.app.app()?.state().resolve::<Cache>().await?;
        let store = cache.store(ARTICLE_STORE).await?;
        TenantContext::scope(TenantId::new(TENANT)?, async {
            store.remove(&cache_key(1, self.id)).await
        })
        .await??;
        Ok(())
    }
    async fn close(mut self) -> TestResult {
        self.remove_cache().await?;
        self.app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await?;
        // 精确限制本 fixture 的随机 queue namespace，不触碰 Redis 其他数据。
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(format!("{{openrust:{}}}:*", self.namespace))
            .query_async(&mut self.redis)
            .await?;
        if !keys.is_empty() {
            let _: usize = redis::cmd("DEL")
                .arg(keys)
                .query_async(&mut self.redis)
                .await?;
        }
        self.database.close().await?;
        Ok(())
    }
}

struct TableFault {
    db: Arc<Db>,
    restore: Option<String>,
}
impl TableFault {
    async fn inject(fixture: &Fixture) -> TestResult<Self> {
        let db = fixture.app.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        // 模块迁移使用固定业务表名，不能套用框架表前缀。
        let (table, unavailable) = match connection.get_database_backend() {
            openrust::db::sea_orm::DbBackend::Postgres => (
                "\"example_articles\"",
                "\"example_articles_retry_unavailable\"",
            ),
            openrust::db::sea_orm::DbBackend::MySql => {
                ("`example_articles`", "`example_articles_retry_unavailable`")
            }
            _ => return Err("unsupported queue retry database backend".into()),
        };
        connection
            .execute_raw(Statement::from_string(
                connection.get_database_backend(),
                format!("ALTER TABLE {table} RENAME TO {unavailable}"),
            ))
            .await?;
        Ok(Self {
            db,
            restore: Some(format!("ALTER TABLE {unavailable} RENAME TO {table}")),
        })
    }
    async fn restore(mut self) -> TestResult {
        if let Some(sql) = &self.restore {
            let connection = self.db.primary_connection()?;
            tokio::time::timeout(
                Duration::from_secs(5),
                connection.execute_raw(Statement::from_string(
                    connection.get_database_backend(),
                    sql.clone(),
                )),
            )
            .await??;
            self.restore = None;
        }
        Ok(())
    }
}
impl Drop for TableFault {
    fn drop(&mut self) {
        if let Some(sql) = self.restore.take() {
            let db = Arc::clone(&self.db);
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                let _cleanup = runtime.spawn(async move {
                    if let Ok(connection) = db.primary_connection() {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(5),
                            connection.execute_raw(Statement::from_string(
                                connection.get_database_backend(),
                                sql,
                            )),
                        )
                        .await;
                    }
                });
            }
        }
    }
}
fn assert_attempt(value: &Value, attempts: u64) {
    assert_eq!(value["attempts"], attempts);
    assert_eq!(value["payload"]["metadata"]["tenant_id"], TENANT);
    assert!(value["payload"]["payload"]["article_id"].as_i64().is_some());
}
async fn exercise(redis: bool) -> TestResult {
    let fixture = Fixture::create(redis).await?;
    let id = fixture.dispatch(2).await?;
    let fault = TableFault::inject(&fixture).await?;
    let failed = fixture.consume().await;
    // 紧接持久失败写入检查退避，避免恢复 DDL 和快照查询耗尽一秒退避窗口。
    let immediate = fixture.consume().await;
    // 在检查结果与断言之前恢复隔离表；Drop 是提前退出时的有界后备。
    fault.restore().await?;
    assert!(failed?);
    assert!(!immediate?, "backoff must prevent immediate retry");
    assert_attempt(
        &fixture.snapshot(&id, false).await?.ok_or("retry missing")?,
        1,
    );
    let waiting = fixture
        .snapshot(&id, false)
        .await?
        .ok_or("backoff record missing")?;
    assert!(!waiting["available_at"].is_null());
    fixture.assert_cache(false).await?;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.consume().await? {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok::<(), Error>(())
    })
    .await??;
    assert!(fixture.snapshot(&id, false).await?.is_none());
    fixture.assert_cache(true).await?;
    fixture.remove_cache().await?;
    let terminal = fixture.dispatch(1).await?;
    let fault = TableFault::inject(&fixture).await?;
    let failed = fixture.consume().await;
    fault.restore().await?;
    assert!(failed?);
    assert!(fixture.snapshot(&terminal, false).await?.is_none());
    assert_attempt(
        &fixture
            .snapshot(&terminal, true)
            .await?
            .ok_or("terminal failure missing")?,
        1,
    );
    let failed_record = fixture
        .snapshot(&terminal, true)
        .await?
        .ok_or("failed record missing")?;
    assert_ne!(
        failed_record["error_code"]
            .as_str()
            .ok_or("safe failure code missing")?,
        ""
    );
    fixture.assert_cache(false).await?;
    let queue = fixture.queue().await?;
    let retried = queue.retry_failed(&terminal).await?;
    assert_ne!(retried, terminal);
    assert_eq!(queue.retry_failed(&terminal).await?, retried);
    assert_attempt(
        &fixture
            .snapshot(&retried, false)
            .await?
            .ok_or("public retry missing")?,
        0,
    );
    assert!(fixture.consume().await?);
    fixture.assert_cache(true).await?;
    assert!(fixture.snapshot(&retried, false).await?.is_none());
    fixture.close().await
}
#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 Redis，显式配置 OPENRUST_TEST_DATABASE_URL/OPENRUST_TEST_REDIS_URL 后运行"]
async fn database_article_index_retries_and_recovers_terminal_failure() -> TestResult {
    exercise(false).await
}
#[tokio::test]
#[ignore = "需真实 PostgreSQL/MySQL 与 Redis，显式配置 OPENRUST_TEST_DATABASE_URL/OPENRUST_TEST_REDIS_URL 后运行"]
async fn redis_article_index_retries_and_recovers_terminal_failure() -> TestResult {
    exercise(true).await
}
