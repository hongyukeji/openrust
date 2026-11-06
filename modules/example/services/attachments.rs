//! 文章附件使用公开 Storage 流接口；数据库授权与对象发布不是跨系统事务。

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use openrust::database::sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter,
    QuerySelect, TransactionTrait,
};
use openrust::filesystem::FilesystemManager;
use openrust::filesystem::StoragePath;
use openrust::filesystem::StorageReader;
use openrust::filesystem::StoredObject;
use openrust::filesystem::UploadContext;
use openrust::filesystem::UploadInspector;
use openrust::filesystem::UploadOptions;
use openrust::filesystem::Visibility;
use openrust::foundation::resources::ResourceRegistrar;
use openrust::http::axum::{body::Body, http::StatusCode};
use openrust::tenant::{TenantContext, TenantId};
use openrust::{Db, Error, Result};

use crate::modules::example::http::requests::attachments::{
    AttachmentUpload, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENT_CHUNK, too_large,
};
use crate::modules::example::models::{article, attachment_cleanup};

const MAX_UPLOAD_OPERATION: Duration = Duration::from_secs(45);
const BODY_PENDING: u8 = 0;
const BODY_EOF: u8 = 1;
const BODY_DEADLINE: u8 = 2;

/// 每篇文章一个 UTF-8 文本附件，key 完全由已授权的业务标识构造。
pub(crate) struct ArticleAttachments {
    db: Arc<Db>,
    storage: Arc<FilesystemManager>,
    resources: ResourceRegistrar,
    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    publish_gate: Mutex<Option<Arc<PublishGate>>>,
    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    cleanup_gate: Mutex<Option<Arc<PublishGate>>>,
    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    locked_cleanup_gate: Mutex<std::collections::VecDeque<Arc<PublishGate>>>,
    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    deleted_object_gate: Mutex<Option<Arc<PublishGate>>>,
    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    before_intent_lock: Mutex<Option<Arc<tokio::sync::Notify>>>,
}
#[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
struct PublishGate {
    reached: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Semaphore>,
}
impl ArticleAttachments {
    pub(crate) fn new(
        db: Arc<Db>,
        storage: Arc<FilesystemManager>,
        resources: ResourceRegistrar,
    ) -> Self {
        Self {
            db,
            storage,
            resources,
            #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
            publish_gate: Mutex::new(None),
            #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
            cleanup_gate: Mutex::new(None),
            #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
            locked_cleanup_gate: Mutex::new(std::collections::VecDeque::new()),
            #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
            deleted_object_gate: Mutex::new(None),
            #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
            before_intent_lock: Mutex::new(None),
        }
    }

    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    pub(crate) fn pause_before_cleanup(
        &self,
        reached: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Semaphore>,
    ) -> Result<()> {
        *self
            .cleanup_gate
            .lock()
            .map_err(|_| Error::internal(std::io::Error::other("cleanup test gate poisoned")))? =
            Some(Arc::new(PublishGate { reached, release }));
        Ok(())
    }

    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    pub(crate) fn pause_after_cleanup_lock(
        &self,
        reached: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Semaphore>,
    ) -> Result<()> {
        self.locked_cleanup_gate
            .lock()
            .map_err(|_| {
                Error::internal(std::io::Error::other("locked cleanup test gate poisoned"))
            })?
            .push_back(Arc::new(PublishGate { reached, release }));
        Ok(())
    }

    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    pub(crate) fn pause_after_object_delete(
        &self,
        reached: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Semaphore>,
    ) -> Result<()> {
        *self.deleted_object_gate.lock().map_err(|_| {
            Error::internal(std::io::Error::other("deleted object test gate poisoned"))
        })? = Some(Arc::new(PublishGate { reached, release }));
        Ok(())
    }

    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    pub(crate) fn signal_before_intent_lock(&self, signal: Arc<tokio::sync::Notify>) -> Result<()> {
        *self.before_intent_lock.lock().map_err(|_| {
            Error::internal(std::io::Error::other("intent lock test signal poisoned"))
        })? = Some(signal);
        Ok(())
    }

    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    pub(crate) fn pause_after_publish(
        &self,
        reached: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Semaphore>,
    ) -> Result<()> {
        *self
            .publish_gate
            .lock()
            .map_err(|_| Error::internal(std::io::Error::other("upload test gate poisoned")))? =
            Some(Arc::new(PublishGate { reached, release }));
        Ok(())
    }

    async fn authorized_path(&self, owner_id: i64, article_id: i64) -> Result<StoragePath> {
        let path = Self::object_path(owner_id, article_id)?;
        article::Entity::find_by_id(article_id)
            .filter(article::Column::OwnerId.eq(owner_id))
            .one(&self.db.primary_connection()?)
            .await
            .map_err(Error::internal)?
            .ok_or_else(not_found)?;
        Ok(path)
    }

    /// 在文章所有权检查后修改当前租户的附件可见性。
    pub(crate) async fn set_visibility(
        &self,
        owner_id: i64,
        article_id: i64,
        visibility: Visibility,
    ) -> Result<()> {
        let path = self.authorized_path(owner_id, article_id).await?;
        self.storage
            .default_disk()
            .await?
            .set_visibility(&path, visibility)
            .await
            .map_err(storage_error)
    }

    fn object_path(owner_id: i64, article_id: i64) -> Result<StoragePath> {
        if owner_id <= 0 {
            return Err(Error::Unauthorized);
        }
        if article_id <= 0 {
            return Err(not_found());
        }
        StoragePath::new(format!(
            "example/articles/{owner_id}/{article_id}/attachment.txt"
        ))
    }

    /// 与文章删除写在同一数据库事务内；不会在提交前碰对象存储。
    pub(crate) async fn record_deleted(
        &self,
        transaction: &DatabaseTransaction,
        owner_id: i64,
        article_id: i64,
    ) -> Result<()> {
        attachment_cleanup::ActiveModel {
            article_id: Set(article_id),
            owner_id: Set(owner_id),
            tenant_id: Set(TenantContext::current().map(|tenant| tenant.as_str().to_owned())),
        }
        .insert(transaction)
        .await
        .map_err(Error::internal)?;
        Ok(())
    }

    /// 提交后幂等处理持久意图；取消/存储失败时意图保留，可由后续调用或维护命令重试。
    #[allow(clippy::too_many_lines)] // 持锁重查、对象删除与意图提交必须在同一事务作用域。
    pub(crate) async fn cleanup_deleted(&self, owner_id: i64, article_id: i64) -> Result<()> {
        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
        let cleanup_gate = self
            .cleanup_gate
            .lock()
            .map_err(|_| Error::internal(std::io::Error::other("cleanup test gate poisoned")))?
            .take();
        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
        if let Some(gate) = cleanup_gate {
            gate.reached.notify_one();
            let _permit =
                gate.release.acquire().await.map_err(|_| {
                    Error::internal(std::io::Error::other("cleanup test gate closed"))
                })?;
        }
        let connection = self.db.primary_connection()?;
        let transaction = connection.begin().await.map_err(Error::internal)?;
        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
        if let Some(signal) = self
            .before_intent_lock
            .lock()
            .map_err(|_| {
                Error::internal(std::io::Error::other("intent lock test signal poisoned"))
            })?
            .take()
        {
            signal.notify_one();
        }
        let pending = attachment_cleanup::Entity::find_by_id(article_id)
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(Error::internal)?;
        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
        let locked_gate = self
            .locked_cleanup_gate
            .lock()
            .map_err(|_| {
                Error::internal(std::io::Error::other("locked cleanup test gate poisoned"))
            })?
            .pop_front();
        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
        if let Some(gate) = locked_gate {
            gate.reached.notify_one();
            let _permit = gate.release.acquire().await.map_err(|_| {
                Error::internal(std::io::Error::other("locked cleanup test gate closed"))
            })?;
        }
        let Some(pending) = pending.filter(|row| row.owner_id == owner_id) else {
            transaction.rollback().await.map_err(Error::internal)?;
            return Ok(());
        };
        let tenant = pending.tenant_id.map(TenantId::new).transpose()?;
        if TenantContext::current().is_some_and(|current| tenant.as_ref() != Some(&current)) {
            transaction.rollback().await.map_err(Error::internal)?;
            return Err(cleanup_tenant_mismatch());
        }
        if article::Entity::find_by_id(article_id)
            .one(&transaction)
            .await
            .map_err(Error::internal)?
            .is_some()
        {
            transaction.rollback().await.map_err(Error::internal)?;
            return Err(Error::custom(
                "example_attachment_cleanup_conflict",
                "文章标识已被重新使用，附件需要人工核对",
            ));
        }
        let delete = async {
            let path = Self::object_path(owner_id, article_id)?;
            let disk = self.storage.default_disk().await?;
            disk.delete(&path).await
        };
        let deletion = if let Some(tenant) = tenant {
            TenantContext::scope(tenant, delete).await?
        } else {
            delete.await
        };
        match deletion {
            Ok(()) => {}
            Err(error) if error.code() == "storage_object_not_found" => {}
            Err(_) => Err(Error::custom_with_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "example_attachment_cleanup_failed",
                "文章已删除，但附件清理失败",
            ))?,
        }
        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
        let deleted_gate = self
            .deleted_object_gate
            .lock()
            .map_err(|_| {
                Error::internal(std::io::Error::other("deleted object test gate poisoned"))
            })?
            .take();
        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
        if let Some(gate) = deleted_gate {
            gate.reached.notify_one();
            let _permit = gate.release.acquire().await.map_err(|_| {
                Error::internal(std::io::Error::other("deleted object test gate closed"))
            })?;
        }
        attachment_cleanup::Entity::delete_by_id(article_id)
            .exec(&transaction)
            .await
            .map_err(Error::internal)?;
        transaction.commit().await.map_err(Error::internal)?;
        Ok(())
    }

    /// 可由维护命令调用；只处理已提交的删除意图，失败保留其余待重试项。
    pub(crate) async fn reconcile_pending(&self) -> Result<u64> {
        if TenantContext::current().is_some() {
            return Err(cleanup_tenant_mismatch());
        }
        let connection = self.db.primary_connection()?;
        let mut completed = 0;
        loop {
            let rows = attachment_cleanup::Entity::find()
                .limit(100)
                .all(&connection)
                .await
                .map_err(Error::internal)?;
            if rows.is_empty() {
                return Ok(completed);
            }
            for row in rows {
                self.cleanup_deleted(row.owner_id, row.article_id).await?;
                completed += 1;
            }
        }
    }

    /// 行锁串行化上传与文章删除，再逐块检查并原子发布对象。
    ///
    /// 同 key 并发上传遵守后端完整对象发布语义，不承诺业务级先后次序。
    /// 文章删除的 DELETE 必须等待同一行锁；HTTP 等待取消后，拥有事务的后台任务继续收尾，
    /// 随后删除会清理固定 key。数据库与对象存储仍不具备跨系统原子提交。
    #[allow(clippy::too_many_lines)] // 事务锁、受管任务和发布失败语义需在同一闭环可见。
    pub(crate) async fn upload(
        &self,
        owner_id: i64,
        article_id: i64,
        declaration: AttachmentUpload,
        body: Body,
    ) -> Result<StoredObject> {
        self.upload_with_deadline(
            owner_id,
            article_id,
            declaration,
            body,
            MAX_UPLOAD_OPERATION,
            Duration::from_secs(15),
        )
        .await
    }

    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    pub(crate) async fn upload_with_test_deadline(
        &self,
        owner_id: i64,
        article_id: i64,
        declaration: AttachmentUpload,
        body: Body,
        deadline: Duration,
    ) -> Result<StoredObject> {
        self.upload_with_deadline(
            owner_id,
            article_id,
            declaration,
            body,
            deadline,
            Duration::from_secs(15),
        )
        .await
    }

    #[cfg(all(
        test,
        feature = "http-client",
        feature = "storage-s3",
        any(feature = "db-postgres", feature = "db-mysql")
    ))]
    pub(crate) async fn upload_with_test_deadlines(
        &self,
        owner_id: i64,
        article_id: i64,
        declaration: AttachmentUpload,
        body: Body,
        body_deadline: Duration,
        response_deadline: Duration,
    ) -> Result<StoredObject> {
        self.upload_with_deadline(
            owner_id,
            article_id,
            declaration,
            body,
            body_deadline,
            response_deadline,
        )
        .await
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn upload_with_deadline(
        &self,
        owner_id: i64,
        article_id: i64,
        _declaration: AttachmentUpload,
        body: Body,
        body_deadline: Duration,
        response_deadline: Duration,
    ) -> Result<StoredObject> {
        let path = Self::object_path(owner_id, article_id)?;
        let disk = self.storage.default_disk().await?;
        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
        let publish_gate = self
            .publish_gate
            .lock()
            .map_err(|_| Error::internal(std::io::Error::other("upload test gate poisoned")))?
            .take();
        let database = Arc::clone(&self.db);
        let tenant = TenantContext::current();
        let (request_cancel_sender, request_cancel_receiver) = tokio::sync::oneshot::channel();
        let (body_eof_sender, body_eof_receiver) = tokio::sync::oneshot::channel();
        let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
        self.resources.spawn(move |_cancellation| async move {
            let operation = async move {
                let transaction = database
                    .primary_connection()?
                    .begin()
                    .await
                    .map_err(Error::internal)?;
                let exists = article::Entity::find_by_id(article_id)
                    .filter(article::Column::OwnerId.eq(owner_id))
                    .lock_exclusive()
                    .one(&transaction)
                    .await
                    .map_err(Error::internal)?;
                if exists.is_none() {
                    transaction.rollback().await.map_err(Error::internal)?;
                    return Err(not_found());
                }
                // 遗留删除意图未清理前不允许同 key 发布新对象；旧意图只会清旧对象。
                if attachment_cleanup::Entity::find_by_id(article_id)
                    .one(&transaction)
                    .await
                    .map_err(Error::internal)?
                    .is_some()
                {
                    transaction.rollback().await.map_err(Error::internal)?;
                    return Err(Error::custom(
                        "example_attachment_cleanup_conflict",
                        "附件清理尚未完成",
                    ));
                }
                let body_deadline_at = tokio::time::Instant::now() + body_deadline;
                let body_state = Arc::new(AtomicU8::new(BODY_PENDING));
                let upload = disk.put_body(
                    path,
                    Some("text/plain".to_owned()),
                    bounded_body(
                        body,
                        request_cancel_receiver,
                        body_eof_sender,
                        Arc::clone(&body_state),
                    ),
                    UploadOptions {
                        visibility: None,
                        max_bytes: MAX_ATTACHMENT_BYTES,
                        max_chunk_bytes: MAX_ATTACHMENT_CHUNK,
                        allowed_content_types: vec!["text/plain".to_owned()],
                    },
                    Some(Arc::new(TextAttachmentInspector::default())),
                );
                let result = await_bounded_body_upload(
                    upload,
                    body_eof_receiver,
                    body_deadline_at,
                    body_state,
                )
                .await
                .map_err(storage_error);
                match result {
                    Ok(stored) => {
                        #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
                        if let Some(gate) = publish_gate {
                            gate.reached.notify_one();
                            let _permit = gate.release.acquire().await.map_err(|_| {
                                Error::internal(std::io::Error::other("upload test gate closed"))
                            })?;
                        }
                        transaction.commit().await.map_err(Error::internal)?;
                        Ok(stored)
                    }
                    Err(error) => {
                        transaction.rollback().await.map_err(Error::internal)?;
                        Err(error)
                    }
                }
            };
            let result = match tenant {
                Some(tenant) => TenantContext::scope(tenant, operation)
                    .await
                    .unwrap_or_else(Err),
                None => operation.await,
            };
            let _ = result_sender.send(result);
        })?;
        let response = tokio::time::timeout(response_deadline, result_receiver).await;
        drop(request_cancel_sender);
        match response {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::custom(
                "example_attachment_upload_worker_failed",
                "附件上传任务异常终止",
            )),
            // HTTP 可先返回超时；App 资源所有者仍跟踪发布任务，并在关闭时等待有界收尾。
            Err(_) => Err(Error::custom_with_status(
                StatusCode::REQUEST_TIMEOUT,
                "example_attachment_upload_timeout",
                "附件上传超过 15 秒上限；后台发布仍在收尾",
            )),
        }
    }

    pub(crate) async fn open(&self, owner_id: i64, article_id: i64) -> Result<StorageReader> {
        let path = self.authorized_path(owner_id, article_id).await?;
        self.storage
            .default_disk()
            .await?
            .open(&path, None)
            .await
            .map_err(storage_error)
    }

    /// 删除前重新检查文章归属；对象不存在时统一返回 404。
    pub(crate) async fn delete(&self, owner_id: i64, article_id: i64) -> Result<()> {
        let path = Self::object_path(owner_id, article_id)?;
        let disk = self.storage.default_disk().await?;
        let transaction = self
            .db
            .primary_connection()?
            .begin()
            .await
            .map_err(Error::internal)?;
        let exists = article::Entity::find_by_id(article_id)
            .filter(article::Column::OwnerId.eq(owner_id))
            .lock_exclusive()
            .one(&transaction)
            .await
            .map_err(Error::internal)?;
        if exists.is_none() {
            transaction.rollback().await.map_err(Error::internal)?;
            return Err(not_found());
        }
        let result = async {
            disk.stat(&path).await.map_err(storage_error)?;
            disk.delete(&path).await.map_err(storage_error)
        }
        .await;
        match result {
            Ok(()) => transaction.commit().await.map_err(Error::internal),
            Err(error) => {
                transaction.rollback().await.map_err(Error::internal)?;
                Err(error)
            }
        }
    }
}

fn bounded_body(
    body: Body,
    cancellation: tokio::sync::oneshot::Receiver<()>,
    eof_sender: tokio::sync::oneshot::Sender<()>,
    body_state: Arc<AtomicU8>,
) -> Body {
    let stream = futures_util::stream::unfold(
        (
            body.into_data_stream(),
            None::<openrust::http::axum::body::Bytes>,
            Some(cancellation),
            Some(eof_sender),
            body_state,
        ),
        |(mut stream, mut pending, mut cancellation, mut eof_sender, body_state)| async move {
            loop {
                if let Some(mut bytes) = pending.take() {
                    let length = bytes.len().min(MAX_ATTACHMENT_CHUNK);
                    let output = bytes.split_to(length);
                    if !bytes.is_empty() {
                        pending = Some(bytes);
                    }
                    return Some((
                        Ok::<_, Error>(output),
                        (stream, pending, cancellation, eof_sender, body_state),
                    ));
                }
                let next = if let Some(cancel) = &mut cancellation {
                    tokio::select! {
                        biased;
                        _ = &mut *cancel => {
                            cancellation = None;
                            Some(Err(Error::custom(
                                "example_attachment_request_cancelled",
                                "附件请求已取消",
                            )))
                        }
                        next = stream.next() => next.map(|result| result.map_err(|_| Error::custom(
                            "example_attachment_body_failed",
                            "附件请求流读取失败",
                        ))),
                    }
                } else {
                    tokio::select! {
                        biased;
                        next = stream.next() => next.map(|result| result.map_err(|_| Error::custom(
                            "example_attachment_body_failed",
                            "附件请求流读取失败",
                        ))),
                    }
                };
                match next {
                    Some(Ok(bytes)) => pending = Some(bytes),
                    Some(Err(error)) => {
                        return Some((
                            Err(error),
                            (stream, None, cancellation, eof_sender, body_state),
                        ));
                    }
                    None => {
                        if body_state
                            .compare_exchange(
                                BODY_PENDING,
                                BODY_EOF,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_err()
                        {
                            return Some((
                                Err(upload_deadline_error()),
                                (stream, pending, cancellation, eof_sender, body_state),
                            ));
                        }
                        if let Some(sender) = eof_sender.take() {
                            let _ = sender.send(());
                        }
                        return None;
                    }
                }
            }
        },
    );
    Body::from_stream(stream)
}

fn not_found() -> Error {
    Error::NotFound("article attachment not found".into())
}

fn upload_deadline_error() -> Error {
    Error::custom_with_status(
        StatusCode::REQUEST_TIMEOUT,
        "example_attachment_upload_deadline",
        "附件请求体读取超过期限",
    )
}

async fn await_bounded_body_upload<T, F>(
    upload: F,
    body_eof: tokio::sync::oneshot::Receiver<()>,
    deadline: tokio::time::Instant,
    body_state: Arc<AtomicU8>,
) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    tokio::pin!(upload);
    tokio::pin!(body_eof);
    tokio::select! {
        biased;
        result = &mut upload => result,
        eof = &mut body_eof => if eof.is_ok() {
            upload.await
        } else {
            tokio::select! {
                biased;
                result = &mut upload => result,
                () = tokio::time::sleep_until(deadline) => {
                    match body_state.compare_exchange(
                        BODY_PENDING,
                        BODY_DEADLINE,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) | Err(BODY_DEADLINE) => Err(upload_deadline_error()),
                        Err(BODY_EOF) => upload.await,
                        Err(_) => Err(Error::internal(std::io::Error::other("invalid attachment body state"))),
                    }
                },
            }
        },
        () = tokio::time::sleep_until(deadline) => {
            match body_state.compare_exchange(
                BODY_PENDING,
                BODY_DEADLINE,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) | Err(BODY_DEADLINE) => Err(upload_deadline_error()),
                Err(BODY_EOF) => upload.await,
                Err(_) => Err(Error::internal(std::io::Error::other("invalid attachment body state"))),
            }
        },
    }
}

fn cleanup_tenant_mismatch() -> Error {
    Error::custom_with_status(
        StatusCode::FORBIDDEN,
        "example_attachment_cleanup_tenant_mismatch",
        "附件清理需要原始租户作用域或无租户维护入口",
    )
}
fn storage_error(error: Error) -> Error {
    match error.code() {
        "storage_upload_too_large" | "storage_upload_chunk_too_large" => too_large(),
        "storage_object_not_found" => not_found(),
        _ => error,
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;

    #[tokio::test]
    async fn completed_body_wins_when_eof_and_deadline_are_both_ready() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let _ = sender.send(());
        let body_state = Arc::new(AtomicU8::new(BODY_EOF));
        let deadline = tokio::time::Instant::now() - Duration::from_millis(1);
        let result = await_bounded_body_upload(
            async {
                tokio::task::yield_now().await;
                Ok(7_u8)
            },
            receiver,
            deadline,
            body_state,
        )
        .await;
        assert!(matches!(result, Ok(7)));
    }

    #[tokio::test]
    async fn closed_eof_signal_keeps_deadline_active_for_failed_body_cleanup() {
        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        drop(sender);
        let body_state = Arc::new(AtomicU8::new(BODY_PENDING));
        let deadline = tokio::time::Instant::now() + Duration::from_millis(10);
        let result = await_bounded_body_upload(
            std::future::pending::<Result<()>>(),
            receiver,
            deadline,
            body_state,
        )
        .await;
        assert_eq!(
            result.err().map(|error| error.code().to_owned()),
            Some("example_attachment_upload_deadline".to_owned())
        );
    }

    #[tokio::test]
    async fn deadline_winner_prevents_a_late_normal_eof_from_starting_publish() {
        let (_sender, receiver) = tokio::sync::oneshot::channel::<()>();
        let body_state = Arc::new(AtomicU8::new(BODY_PENDING));
        let deadline = tokio::time::Instant::now() - Duration::from_millis(1);
        let result = await_bounded_body_upload(
            std::future::pending::<Result<()>>(),
            receiver,
            deadline,
            Arc::clone(&body_state),
        )
        .await;
        assert_eq!(
            result.err().map(|error| error.code().to_owned()),
            Some("example_attachment_upload_deadline".to_owned())
        );
        assert_eq!(body_state.load(Ordering::Acquire), BODY_DEADLINE);
        assert_eq!(
            body_state.compare_exchange(
                BODY_PENDING,
                BODY_EOF,
                Ordering::AcqRel,
                Ordering::Acquire
            ),
            Err(BODY_DEADLINE)
        );
    }
}

/// 实际检查 UTF-8 与控制字符；不是防病毒或敏感内容识别器。
/// 每次上传创建一个实例，只保留跨块字符最多三个尾字节。
#[derive(Default)]
pub(crate) struct TextAttachmentInspector {
    tail: Mutex<Vec<u8>>,
}
#[async_trait]
impl UploadInspector for TextAttachmentInspector {
    async fn inspect_chunk(&self, _context: &UploadContext, chunk: &[u8]) -> Result<()> {
        let mut tail = self.tail.lock().map_err(|_| invalid_text())?;
        let mut bytes = std::mem::take(&mut *tail);
        bytes.extend_from_slice(chunk);
        match std::str::from_utf8(&bytes) {
            Ok(text) => check_characters(text),
            Err(error) => {
                if error.error_len().is_some() {
                    return Err(invalid_text());
                }
                let prefix = std::str::from_utf8(&bytes[..error.valid_up_to()])
                    .map_err(|_| invalid_text())?;
                check_characters(prefix)?;
                tail.extend_from_slice(&bytes[error.valid_up_to()..]);
                if tail.len() > 3 {
                    return Err(invalid_text());
                }
                Ok(())
            }
        }
    }
    async fn finish(&self, _context: &UploadContext, total: u64) -> Result<()> {
        if total == 0 || !self.tail.lock().map_err(|_| invalid_text())?.is_empty() {
            return Err(invalid_text());
        }
        Ok(())
    }
}
fn check_characters(text: &str) -> Result<()> {
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(invalid_text());
    }
    Ok(())
}
fn invalid_text() -> Error {
    Error::custom_with_status(
        StatusCode::UNPROCESSABLE_ENTITY,
        "example_attachment_text_invalid",
        "附件必须是非空且不含禁用控制字符的 UTF-8 文本",
    )
}
