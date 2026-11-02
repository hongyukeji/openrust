//! 迁移命令的受管执行与取消边界；调用 future 被释放不能丢失在途 DDL 所有者。

use std::time::Duration;

use crate::core::{
    App, Cancellation, Error, ExecutionContext, Result, resources::ResourceRegistrar,
};
use crate::db::{Db, DbBackend, migration_lock::MigrationLock};

use super::{CatalogMigrator, MigrationAction};

#[cfg(feature = "db-sqlite")]
pub(super) mod residency;

#[cfg(feature = "db-sqlite")]
tokio::task_local! { static SQLITE_OWNER: (); }

#[cfg(feature = "db-sqlite")]
pub(super) fn is_sqlite_owner() -> bool {
    SQLITE_OWNER.try_with(|()| ()).is_ok()
}

#[derive(Clone)]
pub(super) struct MigrationCancellation {
    caller: Cancellation,
    shutdown: Cancellation,
}

impl MigrationCancellation {
    #[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
    pub(super) fn uncancelled() -> Self {
        Self {
            caller: Cancellation::new(),
            shutdown: Cancellation::new(),
        }
    }

    pub(super) fn check(&self) -> Result<()> {
        if self.caller.is_cancelled() || self.shutdown.is_cancelled() {
            Err(cancelled())
        } else {
            Ok(())
        }
    }

    async fn cancelled(&self) {
        tokio::select! {
            () = self.caller.cancelled() => {},
            () = self.shutdown.cancelled() => {},
        }
    }
}

struct CancelCallerOnDrop(Cancellation);

impl Drop for CancelCallerOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) async fn execute(
    app: &App,
    resources: &ResourceRegistrar,
    db: Db,
    url: String,
    migrator: CatalogMigrator,
    action: MigrationAction,
    wait: Duration,
) -> Result<()> {
    let caller = Cancellation::new();
    let _cancel = CancelCallerOnDrop(caller.clone());
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let scope = app.state().container().current_entry_scope();
    #[cfg(feature = "db-sqlite")]
    let worker_scope = scope.clone();
    #[cfg(feature = "db-sqlite")]
    let sqlite = db.backend()? == DbBackend::Sqlite;
    let context = ExecutionContext::current();
    let metadata = context
        .as_ref()
        .map(|context| context.metadata().clone())
        .unwrap_or_default();
    let deadline = context.as_ref().and_then(ExecutionContext::deadline);
    let lock_deadline = tokio::time::Instant::now() + wait;
    let lock_deadline = deadline.map_or(lock_deadline, |deadline| deadline.min(lock_deadline));
    let locale = metadata.locale().cloned();
    let i18n = if app.state().container().contains::<crate::i18n::I18n>() {
        Some(app.state().resolve::<crate::i18n::I18n>().await?)
    } else {
        None
    };
    let task = move |shutdown| {
        Box::pin(async move {
            let cancellation = MigrationCancellation { caller, shutdown };
            #[cfg(feature = "db-sqlite")]
            let owner_cancellation = cancellation.clone();
            let work: crate::core::BoxFuture<'static, Result<()>> = Box::pin(async move {
                let remaining =
                    lock_deadline.saturating_duration_since(tokio::time::Instant::now());
                let work: crate::core::BoxFuture<'_, Result<()>> = Box::pin(perform(
                    &db,
                    &url,
                    &migrator,
                    action,
                    remaining,
                    &cancellation,
                ));
                if let (Some(i18n), Some(locale)) = (i18n, locale) {
                    i18n.with_locale(locale, work).await
                } else {
                    work.await
                }
            });
            #[cfg(feature = "db-sqlite")]
            let result = if sqlite {
                sqlite_owner(worker_scope, lock_deadline, owner_cancellation, work).await
            } else {
                work.await
            };
            #[cfg(not(feature = "db-sqlite"))]
            let result = work.await;
            report_result(sender, result);
        })
    };
    if let Some(scope) = scope {
        scope.spawn(metadata, deadline, task)?;
    } else {
        resources.spawn(move |shutdown| async move {
            let context = ExecutionContext::new(metadata, deadline, shutdown.clone());
            crate::core::with_execution_context(context, task(shutdown)).await;
        })?;
    }
    receiver
        .await
        .map_err(|_| Error::custom("migration_worker_lost", "迁移执行所有者异常终止"))?
}

// SQLx SQLite 的原生 worker 不随调用 future 的 Drop 同步退出。专用 runtime
// 必须活到迁移及连接归还完成，不能被入口 Scope 的强制 abort 一起销毁。
#[cfg(feature = "db-sqlite")]
async fn sqlite_owner(
    scope: Option<std::sync::Arc<crate::core::container::Scope>>,
    deadline: tokio::time::Instant,
    cancellation: MigrationCancellation,
    work: crate::core::BoxFuture<'static, Result<()>>,
) -> Result<()> {
    // 限制被取消入口留下的原生执行线程；属于内部资源上限，不是 Laravel 配置默认值。
    const MAX_NATIVE_OWNERS: usize = 4;
    static OWNERS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    let acquire = OWNERS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_NATIVE_OWNERS)))
        .clone()
        .acquire_owned();
    let permit = tokio::select! {
        () = cancellation.cancelled() => return Err(cancelled()),
        result = tokio::time::timeout_at(deadline, acquire) => result
            .map_err(|_| Error::custom("migration_lock_timeout", "等待迁移执行名额超时"))?
            .map_err(|_| Error::custom("migration_owner_unavailable", "迁移执行名额不可用"))?,
    };
    let context = ExecutionContext::current();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("openrust-sqlite-migration".into())
        .spawn(move || {
            // 名额属于线程，等待方取消或 Scope 超时不能提前归还。
            let _permit = permit;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let result = match runtime {
                Ok(runtime) => runtime.block_on(SQLITE_OWNER.scope((), async move {
                    let work = async move {
                        if let Some(context) = context {
                            crate::core::with_execution_context(context, work).await
                        } else {
                            work.await
                        }
                    };
                    if let Some(scope) = scope {
                        scope.run(work).await
                    } else {
                        work.await
                    }
                })),
                Err(_) => Err(Error::custom(
                    "migration_owner_runtime_unavailable",
                    "无法建立迁移执行环境",
                )),
            };
            report_result(sender, result);
        })
        .map_err(|_| Error::custom("migration_owner_thread_unavailable", "无法建立迁移执行线程"))?;
    receiver
        .await
        .map_err(|_| Error::custom("migration_worker_lost", "迁移执行所有者异常终止"))?
}

fn report_result(sender: tokio::sync::oneshot::Sender<Result<()>>, result: Result<()>) {
    if let Err(result) = sender.send(result)
        && result.is_err()
    {
        tracing::error!("迁移等待方已退出；迁移执行或资源释放失败，回收状态不能视为成功");
    }
}

async fn perform(
    db: &Db,
    url: &str,
    migrator: &CatalogMigrator,
    action: MigrationAction,
    wait: Duration,
    cancellation: &MigrationCancellation,
) -> Result<()> {
    let lock = tokio::select! {
        () = cancellation.cancelled() => return Err(cancelled()),
        result = ExecutionLock::acquire(db, url, wait) => result?,
    };
    // 已开始的 migration 保持所有权至完成；取消只阻止后续 migration，不能抢先解锁。
    let work = super::run_migrator_owned(migrator, db, action, cancellation);
    #[cfg(feature = "db-sqlite")]
    let mut outcome = if db.backend()? == DbBackend::Sqlite {
        use futures_util::FutureExt as _;
        if let Ok(outcome) = std::panic::AssertUnwindSafe(work).catch_unwind().await {
            outcome
        } else {
            // guard 在 catch 范围外；原生操作/归还任务结束前绝不解锁。
            db.primary_connection()?
                .get_sqlite_connection_pool()
                .close()
                .await;
            Err(Error::custom(
                "migration_panicked",
                "迁移实现异常终止，数据库连接池已关闭",
            ))
        }
    } else {
        work.await
    };
    #[cfg(not(feature = "db-sqlite"))]
    let outcome = work.await;
    #[cfg(feature = "db-sqlite")]
    if db.backend()? == DbBackend::Sqlite
        && outcome.as_ref().is_err_and(|error| {
            matches!(
                error.code(),
                "migration_rollback_failed" | "migration_commit_failed"
            )
        })
    {
        // commit(self) 错误与 rollback 错误均不能依赖 Drop 的排队操作证明结束。
        db.primary_connection()?
            .get_sqlite_connection_pool()
            .close()
            .await;
    }
    #[cfg(feature = "db-sqlite")]
    if db.backend()? == DbBackend::Sqlite {
        let pool = db
            .primary_connection()?
            .get_sqlite_connection_pool()
            .clone();
        if !pool.is_closed() {
            // SQLx 的 Drop 将归还动作 spawn 到当前 runtime。结束专用 runtime 前
            // 明确归还一个存活连接，避免关闭最后的物理连接而丢失内存库。
            let resident = match pool.acquire().await {
                Ok(anchor) => residency::return_connection(anchor).await,
                Err(_) => Err(Error::custom(
                    "migration_connection_return_failed",
                    "无法取得迁移归还屏障连接",
                )),
            };
            if !matches!(resident, Ok(true)) {
                // 仅对按标准规则使用单连接的内存池标记关闭，拒绝随后新启动的借用。
                // 文件/多连接池中的单个 handle 失效，不足以关闭其他合法借用者。
                let single_connection_memory = crate::db::sqlite::connection_settings(
                    url,
                    &crate::config::DatabaseInstanceConfig::default(),
                    None,
                )
                .is_ok_and(|settings| settings.in_memory && settings.single_connection);
                if single_connection_memory && pool.options().get_max_connections() == 1 {
                    // SQLx close() 在构造 future 时同步标记关闭；等待仅负责借用者/空闲连接回收。
                    // 此 handle 已完成归还，不能等待随后借入的新请求再结束本次迁移。
                    // 不撤销已有 acquire，也不保证故障与禁重连原子化；应用 shutdown 负责最终回收。
                    drop(pool.close());
                }
                if outcome.is_ok() {
                    outcome = Err(Error::custom(
                        "migration_connection_return_failed",
                        "无法确认迁移物理连接归还后的存活状态",
                    ));
                } else {
                    tracing::error!("迁移失败后无法确认物理连接归还状态");
                }
            }
        } else if outcome.is_ok() {
            outcome = Err(Error::custom(
                "migration_pool_closed",
                "迁移结束前数据库池已关闭",
            ));
        }
    }
    let release = lock.release().await;
    match (outcome, release) {
        (Err(error), Err(_)) => {
            // 不把 SQL、连接地址或驱动诊断写入日志，且保留原迁移错误供调用方处理。
            tracing::error!("迁移失败后锁释放也失败；异常清理将继续保留锁所有权");
            Err(error)
        }
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(not(feature = "db-sqlite"))]
pub(super) fn sqlite_feature_disabled() -> Error {
    Error::custom("database_feature_disabled", "SQLite 迁移需要 db-sqlite")
}

enum ExecutionLock {
    Network(MigrationLock),
    #[cfg(feature = "db-sqlite")]
    Sqlite(crate::db::sqlite::migration_lock::SqliteMigrationLock),
}

impl ExecutionLock {
    async fn acquire(db: &Db, url: &str, wait: Duration) -> Result<Self> {
        if db.backend()? == DbBackend::Sqlite {
            #[cfg(feature = "db-sqlite")]
            {
                return crate::db::sqlite::migration_lock::SqliteMigrationLock::acquire(
                    db, url, wait,
                )
                .await
                .map(Self::Sqlite);
            }
            #[cfg(not(feature = "db-sqlite"))]
            {
                return Err(sqlite_feature_disabled());
            }
        }
        MigrationLock::acquire(db, url, wait)
            .await
            .map(Self::Network)
    }

    async fn release(self) -> Result<()> {
        match self {
            Self::Network(lock) => lock.release().await,
            #[cfg(feature = "db-sqlite")]
            Self::Sqlite(lock) => lock.release(),
        }
    }
}

fn cancelled() -> Error {
    Error::custom("migration_cancelled", "迁移调用已取消")
}

#[cfg(all(test, feature = "db-sqlite"))]
mod tests;
