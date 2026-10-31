//! 租户业务以主库成员关系授权；缓存和对象仅为可重建的派生数据。
use std::{sync::Arc, time::Duration};

use crate::modules::example::models::{tenant_membership, tenant_record};
use openrust::auth::{CurrentUser, QueryRule, QueryScope};
use openrust::cache::Cache;
use openrust::core::{RegisterContext, ServiceProvider};
use openrust::db::sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbBackend, EntityTrait, Statement,
};
#[cfg(feature = "queue")]
use openrust::queue::{Job, JobContext, Queue, QueueContributions};
#[cfg(any(feature = "storage-local", feature = "storage-s3"))]
use openrust::storage::{Storage, StoragePath, StorageReader, UploadOptions};
use openrust::tenant::{TenantContext, TenantId};
use openrust::{Db, Error, Result};

const TTL: Duration = Duration::from_secs(60);
#[cfg(feature = "queue")]
pub(crate) const TENANT_QUEUE: &str = "example-tenant";
pub(crate) struct TenantServiceProvider;
impl ServiceProvider for TenantServiceProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.scoped::<TenantRecords, _>(|resolver| {
                Box::pin(async move {
                    Ok(Arc::new(TenantRecords {
                        db: resolver.resolve::<Db>().await?,
                        cache: resolver.resolve::<Cache>().await?,
                        #[cfg(any(feature = "storage-local", feature = "storage-s3"))]
                        storage: if resolver.is_bound::<Storage>()? {
                            Some(resolver.resolve::<Storage>().await?)
                        } else {
                            None
                        },
                        #[cfg(feature = "queue")]
                        queue: if resolver.is_bound::<Queue>()? {
                            Some(resolver.resolve::<Queue>().await?)
                        } else {
                            None
                        },
                    }))
                })
            })
            .await?;
            Ok(())
        })
    }
    #[cfg(feature = "queue")]
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::core::BootContext<'_>,
    ) -> openrust::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if app.is_bound::<QueueContributions>() {
                app.resolve::<QueueContributions>()
                    .await?
                    .register_job::<TenantRecordJob>()?;
            }
            Ok(())
        })
    }
}
pub(crate) struct TenantRecords {
    db: Arc<Db>,
    cache: Arc<Cache>,
    #[cfg(any(feature = "storage-local", feature = "storage-s3"))]
    storage: Option<Arc<Storage>>,
    #[cfg(feature = "queue")]
    queue: Option<Arc<Queue>>,
}
#[derive(serde::Serialize)]
pub(crate) struct RecordView {
    pub(crate) value: String,
    pub(crate) cache_hit: bool,
}
fn not_found() -> Error {
    Error::NotFound("tenant record not found".into())
}
fn cache_key(owner: i64) -> String {
    format!("example:tenant:record:{owner}")
}
impl TenantRecords {
    /// selector 只有通过主库成员关系检查后才能被中间件提升为可信上下文。
    pub(crate) async fn verify_membership(&self, owner: i64, tenant: &TenantId) -> Result<()> {
        if tenant_membership::Entity::find_by_id((tenant.as_str().to_owned(), owner))
            .one(&self.db.primary_connection()?)
            .await
            .map_err(Error::internal)?
            .is_none()
        {
            return Err(Error::Forbidden("租户成员关系不存在".into()));
        }
        Ok(())
    }
    pub(crate) async fn get(&self, user: &CurrentUser) -> Result<RecordView> {
        let tenant = TenantContext::require_current()?;
        self.verify_membership(user.id(), &tenant).await?;
        let record = QueryRule::<tenant_record::Entity>::new(|user| {
            Ok(QueryScope::new(
                tenant_record::Column::OwnerId.eq(user.id()),
            ))
        })
        .for_tenant(tenant_record::Column::TenantId)
        .select(user, Condition::all())?
        .one(&self.db.primary_connection()?)
        .await?
        .ok_or_else(not_found)?;
        // 每次先确认主库记录和权限；缓存只提供允许短暂旧值的派生显示。
        if let Some(value) = self.cache.get::<String>(&cache_key(user.id())).await? {
            return Ok(RecordView {
                value,
                cache_hit: true,
            });
        }
        self.cache
            .set(&cache_key(user.id()), &record.value, Some(TTL))
            .await?;
        Ok(RecordView {
            value: record.value,
            cache_hit: false,
        })
    }
    pub(crate) async fn put(&self, user: &CurrentUser, value: String) -> Result<()> {
        validate_value(&value)?;
        let tenant = TenantContext::require_current()?;
        let owner = user.id();
        let db = Arc::clone(&self.db);
        self.db.transaction(move |transaction| Box::pin(async move {
            let backend = db.backend()?;
            let (lock, upsert) = match backend {
                DbBackend::Postgres => (
                    "SELECT user_id FROM example_tenant_memberships WHERE tenant_id = $1 AND user_id = $2 FOR UPDATE",
                    "INSERT INTO example_tenant_records (tenant_id,owner_id,value) VALUES ($1,$2,$3) ON CONFLICT (tenant_id,owner_id) DO UPDATE SET value = EXCLUDED.value"),
                DbBackend::MySql => (
                    "SELECT user_id FROM example_tenant_memberships WHERE tenant_id = ? AND user_id = ? FOR UPDATE",
                    "INSERT INTO example_tenant_records (tenant_id,owner_id,value) VALUES (?,?,?) ON DUPLICATE KEY UPDATE value = VALUES(value)"),
                _ => return Err(not_found()),
            };
            if transaction.query_one_raw(Statement::from_sql_and_values(backend, lock,
                [tenant.as_str().into(),owner.into()])).await.map_err(Error::internal)?.is_none() {
                return Err(Error::Forbidden("租户成员关系不存在".into()));
            }
            transaction.execute_raw(Statement::from_sql_and_values(backend, upsert,
                [tenant.as_str().into(),owner.into(),value.into()])).await.map_err(Error::internal)?;
            Ok(())
        })).await?;
        // 数据库已提交；缓存失效失败不会回滚主库，调用者可以显式重建，TTL 也有界。
        self.cache.remove(&cache_key(owner)).await
    }
    #[cfg(any(feature = "queue", feature = "storage-local", feature = "storage-s3"))]
    async fn load_owned(&self, owner: i64) -> Result<tenant_record::Model> {
        let tenant = TenantContext::require_current()?;
        self.verify_membership(owner, &tenant).await?;
        tenant_record::Entity::find_by_id((tenant.as_str().to_owned(), owner))
            .one(&self.db.primary_connection()?)
            .await
            .map_err(Error::internal)?
            .ok_or_else(not_found)
    }
    #[cfg(any(feature = "storage-local", feature = "storage-s3"))]
    pub(crate) async fn write_file(&self, owner: i64) -> Result<()> {
        let record = self.load_owned(owner).await?;
        let storage = self.storage.as_ref().ok_or_else(storage_missing)?;
        storage
            .default_disk()
            .await?
            .put_body(
                file_key(owner)?,
                Some("text/plain".into()),
                openrust::http::axum::body::Body::from(record.value),
                UploadOptions {
                    visibility: None,
                    max_bytes: 2048,
                    max_chunk_bytes: 2048,
                    allowed_content_types: vec!["text/plain".into()],
                },
                None,
            )
            .await?;
        Ok(())
    }
    #[cfg(any(feature = "storage-local", feature = "storage-s3"))]
    pub(crate) async fn read_file(&self, owner: i64) -> Result<StorageReader> {
        self.load_owned(owner).await?;
        self.storage
            .as_ref()
            .ok_or_else(storage_missing)?
            .default_disk()
            .await?
            .open(&file_key(owner)?, None)
            .await
    }
    #[cfg(feature = "queue")]
    pub(crate) async fn dispatch(&self, owner: i64) -> Result<String> {
        self.load_owned(owner).await?;
        self.queue
            .as_ref()
            .ok_or_else(|| {
                Error::custom("example_tenant_queue_unavailable", "租户派生任务队列未启用")
            })?
            .dispatch(
                TENANT_QUEUE,
                TenantRecordJob::NAME,
                &TenantRecordJob { owner_id: owner },
            )
            .await
    }
    #[cfg(feature = "queue")]
    async fn project(&self, owner: i64) -> Result<()> {
        let record = self.load_owned(owner).await?;
        self.cache
            .set(&cache_key(owner), &record.value, Some(TTL))
            .await?;
        #[cfg(any(feature = "storage-local", feature = "storage-s3"))]
        if self.storage.is_some() {
            self.write_file(owner).await?;
        }
        Ok(())
    }
}
fn validate_value(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.chars().count() > 512 || value.chars().any(char::is_control)
    {
        return Err(Error::BadRequest(
            "value 必须为 1 至 512 个无控制字符的非空字符".into(),
        ));
    }
    Ok(())
}
#[cfg(any(feature = "storage-local", feature = "storage-s3"))]
fn file_key(owner: i64) -> Result<StoragePath> {
    StoragePath::new(format!("example/tenant-record/{owner}.txt"))
}
#[cfg(any(feature = "storage-local", feature = "storage-s3"))]
fn storage_missing() -> Error {
    Error::custom(
        "example_tenant_storage_unavailable",
        "租户派生对象存储未启用",
    )
}
#[cfg(feature = "queue")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TenantRecordJob {
    pub(crate) owner_id: i64,
}
#[cfg(feature = "queue")]
impl Job for TenantRecordJob {
    const NAME: &'static str = "example_tenant_record.v1";
    fn handle(&self, context: JobContext) -> openrust::BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let current = TenantContext::require_current()?;
            if context.metadata().tenant_id() != Some(&current) || self.owner_id <= 0 {
                return Err(Error::Forbidden("租户任务上下文不匹配".into()));
            }
            // payload 无 tenant；不能用 payload 绕过 worker 恢复的执行上下文。
            context
                .service::<TenantRecords>()
                .await?
                .project(self.owner_id)
                .await
        })
    }
}
