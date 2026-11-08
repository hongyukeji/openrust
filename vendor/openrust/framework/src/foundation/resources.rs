//! Provider 与入口 scope 共用的有界异步资源回收。

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

use futures_util::FutureExt as _;

use tokio::sync::Notify;
use tokio::time::Instant;

use super::lifecycle::{BackgroundTaskHandle, BackgroundTasks};
use crate::core::{Cancellation, Error, Result};
use crate::support::BoxFuture;

type Cleanup = Box<dyn FnOnce() -> BoxFuture<'static, Result<()>> + Send>;
const MAX_CHILD_SCOPES: usize = 4096;

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Copy, Default)]
struct Outcome {
    failures: usize,
    timed_out: bool,
    interrupted: bool,
}

impl Outcome {
    fn result(self) -> Result<()> {
        if self.interrupted {
            Err(Error::custom(
                "resource_shutdown_interrupted",
                "资源关闭驱动已中断，不能确认异步回收完成",
            ))
        } else if self.timed_out {
            Err(Error::custom(
                "resource_shutdown_timeout",
                "资源关闭超过共同期限",
            ))
        } else if self.failures != 0 {
            Err(Error::custom(
                "resource_shutdown_failed",
                format!("{} 项资源关闭失败（原始值未公开）", self.failures),
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Default)]
struct Inner {
    // None 表示已封口；取出的用户闭包只能在锁外调用或销毁。
    callbacks: Mutex<Option<Vec<Cleanup>>>,
    sealed: AtomicBool,
    tasks: BackgroundTasks,
    outcome: OnceLock<Outcome>,
    finished: Notify,
    deadline: OnceLock<Instant>,
    children: Mutex<Children>,
    child_requested: Arc<Notify>,
    next_child: AtomicU64,
    child_failures: AtomicUsize,
}

struct Completion {
    inner: Arc<Inner>,
    completed: bool,
}

struct ChildEntry {
    owner: Arc<ResourceOwner>,
    close: Option<Box<dyn FnOnce() + Send>>,
    requested: Option<Instant>,
    started: bool,
}

#[derive(Default)]
struct Children {
    entries: HashMap<u64, ChildEntry>,
    driver_running: bool,
}

impl Completion {
    fn finish(mut self, outcome: Outcome) {
        let _ = self.inner.outcome.set(outcome);
        self.completed = true;
        self.inner.finished.notify_waiters();
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.inner.outcome.set(Outcome {
                interrupted: true,
                ..Outcome::default()
            });
            self.inner.finished.notify_waiters();
        }
    }
}

/// 一个应用或显式生命周期拥有的资源；关闭必须显式 `await`。
///
/// 所有者不可克隆，登记句柄为弱引用。`Drop` 只能合作取消/中止任务并释放值，
/// 不会阻塞执行异步回调，也不能作为异步清理已完成的证明。
pub struct ResourceOwner {
    inner: Arc<Inner>,
}

impl Default for ResourceOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceOwner {
    /// 立即通知受管任务合作取消；实际 join 与回调仍由 shutdown 负责。
    pub(crate) fn cancel_tasks(&self) {
        self.inner.tasks.cancel();
    }

    pub(crate) fn record_close_failure(&self) {
        self.inner.child_failures.fetch_add(1, Ordering::AcqRel);
    }

    /// 同步封闭新登记和全部子 scope，异步 join/回调仍由 shutdown 完成。
    pub(crate) fn seal(&self) {
        let callbacks = lock(&self.inner.callbacks);
        self.inner.sealed.store(true, Ordering::Release);
        let children: Vec<_> = {
            let mut children = lock(&self.inner.children);
            children
                .entries
                .values_mut()
                .map(|child| (Arc::clone(&child.owner), child.close.take()))
                .collect()
        };
        drop(callbacks);
        self.inner.tasks.cancel();
        for (owner, close) in children {
            if let Some(close) = close
                && std::panic::catch_unwind(AssertUnwindSafe(close)).is_err()
            {
                self.record_close_failure();
            }
            owner.seal();
        }
    }
    fn shutdown_child(child: Arc<Self>, deadline: Instant) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move { child.shutdown(deadline).await })
    }

    /// 创建尚未封口的资源所有者。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                callbacks: Mutex::new(Some(Vec::new())),
                ..Inner::default()
            }),
        }
    }

    /// 获取不延长所有者寿命的登记句柄。
    pub fn registrar(&self) -> ResourceRegistrar {
        ResourceRegistrar {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// 取消并等待受管任务，再逆序尝试全部资源回调，共享首个调用的 deadline。
    ///
    /// 重复或并发调用共享一次关闭结果。开始关闭后，即使调用方的等待 future 被取消，
    /// 已提交的有界回收仍继续。回调必须合作式异步执行，不能阻塞运行时线程。
    /// 某项失败或超时不会略过其余回调；超时项只能证明
    /// 已尝试并取消，不能声称其异步协议已正常完成。
    pub async fn shutdown(&self, deadline: Instant) -> Result<()> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            Error::custom("shutdown_runtime_missing", "异步资源关闭需要 Tokio 运行时")
        })?;
        let callbacks = {
            let mut callbacks = lock(&self.inner.callbacks);
            if callbacks.is_some() {
                let _ = self.inner.deadline.set(deadline);
            }
            callbacks.take()
        };
        let deadline = *self.inner.deadline.get().unwrap_or(&deadline);
        if let Some(callbacks) = callbacks {
            // 在任何用户代码（包括关闭 scope 缓存时的 Drop）执行前建立完成保护。
            let completion = Completion {
                inner: Arc::clone(&self.inner),
                completed: false,
            };
            let children: Vec<_> = lock(&self.inner.children)
                .entries
                .drain()
                .map(|(_, child)| child)
                .collect();
            let mut close_failures = 0;
            let children: Vec<_> = children
                .into_iter()
                .map(|mut child| {
                    if let Some(close) = child.close.take()
                        && std::panic::catch_unwind(AssertUnwindSafe(close)).is_err()
                    {
                        close_failures += 1;
                    }
                    child.owner
                })
                .collect();
            let tasks = self.inner.tasks.stop();
            let inner = Arc::clone(&self.inner);
            runtime.spawn(async move {
                let mut outcome = Outcome {
                    failures: close_failures,
                    ..Outcome::default()
                };
                match tokio::time::timeout_at(
                    deadline,
                    BackgroundTasks::join_until(tasks, deadline),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        outcome.failures += 1;
                        outcome.timed_out |= error.code() == "background_task_shutdown_timeout";
                    }
                    Err(_) => {
                        outcome.failures += 1;
                        outcome.timed_out = true;
                    }
                }
                for child in children {
                    if Self::shutdown_child(child, deadline).await.is_err() {
                        outcome.failures += 1;
                    }
                }
                outcome.failures += inner.child_failures.swap(0, Ordering::AcqRel);
                for cleanup in callbacks.into_iter().rev() {
                    // 用户同步构造与 future poll 都可能 unwind，不能让一个坏回调遗失后续回收。
                    let Ok(future) = std::panic::catch_unwind(AssertUnwindSafe(cleanup)) else {
                        outcome.failures += 1;
                        continue;
                    };
                    match tokio::time::timeout_at(deadline, AssertUnwindSafe(future).catch_unwind())
                        .await
                    {
                        Ok(Ok(Ok(()))) => {}
                        Ok(Ok(Err(_)) | Err(_)) => outcome.failures += 1,
                        Err(_) => {
                            outcome.failures += 1;
                            outcome.timed_out = true;
                        }
                    }
                }
                completion.finish(outcome);
            });
        }
        loop {
            let notified = self.inner.finished.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(outcome) = self.inner.outcome.get() {
                return outcome.result();
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                let outcome = Outcome {
                    timed_out: true,
                    ..Outcome::default()
                };
                let _ = self.inner.outcome.set(outcome);
                self.inner.finished.notify_waiters();
            }
        }
    }
}

impl Drop for ResourceOwner {
    fn drop(&mut self) {
        let callbacks = lock(&self.inner.callbacks).take();
        drop(callbacks);
        // 已开始的 shutdown 自己持有 JoinSet；未显式关闭时 Drop 只负责中止所有者的任务。
        drop(self.inner.tasks.stop());
    }
}

/// 资源登记的弱句柄；不保存应用或容器的强引用。
#[derive(Clone)]
pub struct ResourceRegistrar {
    inner: Weak<Inner>,
}

impl ResourceRegistrar {
    /// 提交子 scope 回收请求；同一父所有者至多运行一个回收驱动。
    pub(crate) fn request_child_cleanup(&self, id: u64, deadline: Instant) -> Result<()> {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        let callbacks = lock(&inner.callbacks);
        if callbacks.is_none() || inner.sealed.load(Ordering::Acquire) {
            // 父 shutdown 已接管全部仍跟踪的子 scope。
            return Ok(());
        }
        let should_spawn = {
            let mut children = lock(&inner.children);
            if let Some(child) = children.entries.get_mut(&id) {
                child.requested = Some(deadline);
            } else {
                return Ok(());
            }
            if children.driver_running {
                false
            } else {
                children.driver_running = true;
                true
            }
        };
        if should_spawn {
            let weak = Arc::downgrade(&inner);
            let notify = Arc::clone(&inner.child_requested);
            if let Err(error) =
                inner
                    .tasks
                    .handle()
                    .spawn_cleanup_driver(move |cancel| async move {
                        Self::drive_child_cleanup(weak, notify, cancel).await;
                    })
            {
                lock(&inner.children).driver_running = false;
                return Err(error);
            }
        }
        inner.child_requested.notify_one();
        drop(callbacks);
        Ok(())
    }

    async fn drive_child_cleanup(
        inner: Weak<Inner>,
        notify: Arc<Notify>,
        cancellation: Cancellation,
    ) {
        loop {
            let notified = notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let Some(owner) = inner.upgrade() else {
                break;
            };
            let next = {
                let mut children = lock(&owner.children);
                if cancellation.is_cancelled() {
                    children.driver_running = false;
                    None
                } else if let Some((id, child)) = children
                    .entries
                    .iter_mut()
                    .find(|(_, child)| child.requested.is_some() && !child.started)
                {
                    child.started = true;
                    Some((*id, Arc::clone(&child.owner), child.requested))
                } else {
                    None
                }
            };
            drop(owner);
            let Some((id, child, Some(deadline))) = next else {
                if cancellation.is_cancelled() {
                    break;
                }
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    () = &mut notified => continue,
                }
            };
            let failed = tokio::select! {
                biased;
                () = cancellation.cancelled() => break,
                result = child.shutdown(deadline) => result.is_err(),
            };
            if let Some(owner) = inner.upgrade() {
                ResourceRegistrar {
                    inner: Arc::downgrade(&owner),
                }
                .finish_child(id, failed);
            }
        }
    }

    /// 跟踪仍活跃的子生命周期；父关闭时会等待并聚合子资源回收结果。
    pub(crate) fn track_child(
        &self,
        child: Arc<ResourceOwner>,
        close: impl FnOnce() + Send + 'static,
    ) -> Result<u64> {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        let callbacks = lock(&inner.callbacks);
        if callbacks.is_none() || inner.sealed.load(Ordering::Acquire) {
            return Err(resources_closed());
        }
        let id = inner.next_child.fetch_add(1, Ordering::Relaxed);
        let mut children = lock(&inner.children);
        if children.entries.len() >= MAX_CHILD_SCOPES {
            return Err(Error::custom_with_status(
                http::StatusCode::SERVICE_UNAVAILABLE,
                "scope_capacity_exceeded",
                "活跃生命周期已达到上限",
            ));
        }
        children.entries.insert(
            id,
            ChildEntry {
                owner: child,
                close: Some(Box::new(close)),
                requested: None,
                started: false,
            },
        );
        Ok(id)
    }

    /// 子生命周期完成后移出活跃表，失败由父关闭结果聚合。
    pub(crate) fn finish_child(&self, id: u64, failed: bool) {
        if let Some(inner) = self.inner.upgrade() {
            let mut children = lock(&inner.children);
            if children.entries.remove(&id).is_some() && failed {
                inner.child_failures.fetch_add(1, Ordering::AcqRel);
            }
        }
    }

    /// 返回同一资源所有者的后台任务句柄，供内置缓存刷新任务使用。
    pub(crate) fn background_handle(&self) -> Result<BackgroundTaskHandle> {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        if lock(&inner.callbacks).is_none() || inner.sealed.load(Ordering::Acquire) {
            return Err(resources_closed());
        }
        Ok(inner.tasks.handle())
    }

    /// 登记一个异步关闭回调；调用顺序与登记顺序相反。
    pub fn on_shutdown<F>(&self, cleanup: F) -> Result<()>
    where
        F: FnOnce() -> BoxFuture<'static, Result<()>> + Send + 'static,
    {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        let mut callbacks = lock(&inner.callbacks);
        let callbacks = callbacks
            .as_mut()
            .filter(|_| !inner.sealed.load(Ordering::Acquire))
            .ok_or_else(resources_closed)?;
        callbacks.push(Box::new(cleanup));
        Ok(())
    }

    /// 创建复用框架取消协议的受管异步任务；关闭后不能再登记。
    pub fn spawn<F, Fut>(&self, task: F) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        tokio::runtime::Handle::try_current()
            .map_err(|_| Error::custom("resource_runtime_missing", "受管任务需要 Tokio 运行时"))?;
        let callbacks = lock(&inner.callbacks);
        if callbacks.is_none() || inner.sealed.load(Ordering::Acquire) {
            return Err(resources_closed());
        }
        inner.tasks.handle().spawn(task)
    }
}

fn resources_closed() -> Error {
    Error::custom("resources_closed", "资源生命周期已结束，不能再登记")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn child_cleanup_requests_use_one_driver_and_close_each_child() -> Result<()> {
        let parent = ResourceOwner::new();
        let registrar = parent.registrar();
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(Notify::new());
        for _ in 0..16 {
            let child = Arc::new(ResourceOwner::new());
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            let completed = Arc::clone(&completed);
            let finished = Arc::clone(&finished);
            child.registrar().on_shutdown(move || {
                Box::pin(async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(current, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(2)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    if completed.fetch_add(1, Ordering::SeqCst) + 1 == 16 {
                        finished.notify_one();
                    }
                    Ok(())
                })
            })?;
            let id = registrar.track_child(child, || {})?;
            registrar.request_child_cleanup(id, Instant::now() + Duration::from_secs(2))?;
        }
        tokio::time::timeout(Duration::from_secs(2), finished.notified())
            .await
            .unwrap();
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
        parent
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await?;
        assert_eq!(completed.load(Ordering::SeqCst), 16);
        Ok(())
    }

    #[tokio::test]
    async fn parent_shutdown_racing_child_requests_keeps_every_callback() -> Result<()> {
        let parent = ResourceOwner::new();
        let registrar = parent.registrar();
        let completed = Arc::new(AtomicUsize::new(0));
        for _ in 0..32 {
            let child = Arc::new(ResourceOwner::new());
            let count = Arc::clone(&completed);
            child.registrar().on_shutdown(move || {
                Box::pin(async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })?;
            let id = registrar.track_child(child, || {})?;
            registrar.request_child_cleanup(id, Instant::now() + Duration::from_secs(2))?;
        }
        parent
            .shutdown(Instant::now() + Duration::from_secs(2))
            .await?;
        assert_eq!(completed.load(Ordering::SeqCst), 32);
        Ok(())
    }

    #[tokio::test]
    async fn active_child_capacity_rejects_before_unbounded_backlog() -> Result<()> {
        let parent = ResourceOwner::new();
        let registrar = parent.registrar();
        for _ in 0..MAX_CHILD_SCOPES {
            registrar.track_child(Arc::new(ResourceOwner::new()), || {})?;
        }
        let error = registrar
            .track_child(Arc::new(ResourceOwner::new()), || {})
            .err();
        assert_eq!(
            error.as_ref().map(Error::code),
            Some("scope_capacity_exceeded")
        );
        parent
            .shutdown(Instant::now() + Duration::from_secs(10))
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn child_cleanup_driver_has_reserved_slot_when_user_tasks_are_full() -> Result<()> {
        let parent = ResourceOwner::new();
        let registrar = parent.registrar();
        for _ in 0..super::super::lifecycle::MAX_BACKGROUND_TASKS {
            registrar.spawn(|cancel| async move { cancel.cancelled().await })?;
        }
        let cleaned = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(Notify::new());
        let child = Arc::new(ResourceOwner::new());
        let count = Arc::clone(&cleaned);
        let notice = Arc::clone(&done);
        child.registrar().on_shutdown(move || {
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                notice.notify_one();
                Ok(())
            })
        })?;
        let id = registrar.track_child(child, || {})?;
        registrar.request_child_cleanup(id, Instant::now() + Duration::from_secs(2))?;
        tokio::time::timeout(Duration::from_secs(2), done.notified())
            .await
            .unwrap();
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
        // 旧请求完成后驱动仍占保留槽；第二请求须由同一驱动继续回收。
        let second = Arc::new(ResourceOwner::new());
        let count = Arc::clone(&cleaned);
        let notice = Arc::clone(&done);
        second.registrar().on_shutdown(move || {
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                notice.notify_one();
                Ok(())
            })
        })?;
        let second_id = registrar.track_child(second, || {})?;
        registrar.request_child_cleanup(second_id, Instant::now() + Duration::from_secs(2))?;
        tokio::time::timeout(Duration::from_secs(2), done.notified())
            .await
            .unwrap();
        assert_eq!(cleaned.load(Ordering::SeqCst), 2);
        parent
            .shutdown(Instant::now() + Duration::from_secs(2))
            .await?;
        assert!(lock(&parent.inner.children).entries.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn panicking_child_close_keeps_other_children_and_callbacks() {
        let owner = ResourceOwner::new();
        let registrar = owner.registrar();
        let cleaned = Arc::new(AtomicUsize::new(0));
        for should_panic in [true, false] {
            let child = Arc::new(ResourceOwner::new());
            let child_cleaned = Arc::clone(&cleaned);
            child
                .registrar()
                .on_shutdown(move || {
                    Box::pin(async move {
                        child_cleaned.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })
                .unwrap();
            registrar
                .track_child(child, move || {
                    if should_panic {
                        std::panic::resume_unwind(Box::new("private drop"));
                    }
                })
                .unwrap();
        }
        let parent_cleaned = Arc::clone(&cleaned);
        registrar
            .on_shutdown(move || {
                Box::pin(async move {
                    parent_cleaned.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .unwrap();
        let error = owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "resource_shutdown_failed");
        assert_eq!(cleaned.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn reverse_cleanup_continues_after_failure_without_disclosing_source() {
        let owner = ResourceOwner::new();
        let registrar = owner.registrar();
        let order = Arc::new(Mutex::new(Vec::new()));
        for index in 0..3 {
            let order = Arc::clone(&order);
            registrar
                .on_shutdown(move || {
                    Box::pin(async move {
                        lock(&order).push(index);
                        if index == 1 {
                            Err(Error::custom("private", "SECRET_SENTINEL"))
                        } else {
                            Ok(())
                        }
                    })
                })
                .unwrap();
        }
        let error = owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "resource_shutdown_failed");
        assert!(!format!("{error} {error:?}").contains("SECRET_SENTINEL"));
        assert_eq!(*lock(&order), [2, 1, 0]);
        assert_eq!(
            registrar
                .on_shutdown(|| Box::pin(async { Ok(()) }))
                .unwrap_err()
                .code(),
            "resources_closed"
        );
        assert_eq!(
            owner.shutdown(Instant::now()).await.unwrap_err().code(),
            "resource_shutdown_failed"
        );
        assert_eq!(lock(&order).len(), 3);
    }

    #[tokio::test]
    async fn cancelled_waiter_does_not_cancel_started_cleanup_or_duplicate_it() {
        let owner = Arc::new(ResourceOwner::new());
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let notice = Arc::clone(&entered);
        let finish = Arc::clone(&release);
        let count = Arc::clone(&calls);
        owner
            .registrar()
            .on_shutdown(move || {
                Box::pin(async move {
                    count.fetch_add(1, Ordering::AcqRel);
                    notice.notify_one();
                    finish.notified().await;
                    Ok(())
                })
            })
            .unwrap();
        let first = Arc::clone(&owner);
        let waiter = tokio::spawn(async move {
            first
                .shutdown(Instant::now() + Duration::from_secs(1))
                .await
        });
        entered.notified().await;
        waiter.abort();
        assert!(waiter.await.unwrap_err().is_cancelled());
        release.notify_one();
        let (a, b) = tokio::join!(
            owner.shutdown(Instant::now()),
            owner.shutdown(Instant::now())
        );
        a.unwrap();
        b.unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn shared_deadline_cancels_hanging_callback_and_attempts_remaining_cleanup() {
        let owner = ResourceOwner::new();
        let done = Arc::new(AtomicBool::new(false));
        let check = Arc::clone(&done);
        owner
            .registrar()
            .on_shutdown(move || {
                Box::pin(async move {
                    check.store(true, Ordering::Release);
                    Ok(())
                })
            })
            .unwrap();
        owner
            .registrar()
            .on_shutdown(|| Box::pin(std::future::pending()))
            .unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            owner.shutdown(Instant::now() + Duration::from_millis(10)),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err().code(), "resource_shutdown_timeout");
        tokio::time::timeout(Duration::from_secs(1), async {
            while !done.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn managed_tasks_are_cancelled_and_joined_before_resource_cleanup() {
        let owner = ResourceOwner::new();
        let started = Arc::new(Notify::new());
        let stopped = Arc::new(AtomicBool::new(false));
        let notice = Arc::clone(&started);
        let finished = Arc::clone(&stopped);
        owner
            .registrar()
            .spawn(move |cancel| async move {
                notice.notify_one();
                cancel.cancelled().await;
                finished.store(true, Ordering::Release);
            })
            .unwrap();
        owner
            .registrar()
            .on_shutdown(move || {
                Box::pin(async move {
                    assert!(stopped.load(Ordering::Acquire));
                    Ok(())
                })
            })
            .unwrap();
        started.notified().await;
        owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            owner.registrar().spawn(|_| async {}).unwrap_err().code(),
            "resources_closed"
        );
    }

    #[tokio::test]
    async fn dropping_owner_invalidates_weak_registrar_and_aborts_owned_tasks() {
        struct OnDrop(Arc<AtomicBool>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let owner = ResourceOwner::new();
        let registrar = owner.registrar();
        let started = Arc::new(Notify::new());
        let ended = Arc::new(AtomicBool::new(false));
        let notice = Arc::clone(&started);
        let guard = OnDrop(Arc::clone(&ended));
        registrar
            .spawn(move |_| async move {
                let _guard = guard;
                notice.notify_one();
                std::future::pending::<()>().await;
            })
            .unwrap();
        started.notified().await;
        drop(owner);
        tokio::time::timeout(Duration::from_secs(1), async {
            while !ended.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(registrar.inner.upgrade().is_none());
        assert_eq!(
            registrar
                .on_shutdown(|| Box::pin(async { Ok(()) }))
                .unwrap_err()
                .code(),
            "resources_closed"
        );
    }
    #[tokio::test]
    async fn panicking_callback_constructor_and_future_do_not_lose_other_cleanup() {
        for constructor_panic in [false, true] {
            let owner = ResourceOwner::new();
            let done = Arc::new(AtomicBool::new(false));
            let mark = Arc::clone(&done);
            owner
                .registrar()
                .on_shutdown(move || {
                    Box::pin(async move {
                        mark.store(true, Ordering::Release);
                        Ok(())
                    })
                })
                .unwrap();
            owner
                .registrar()
                .on_shutdown(move || {
                    if constructor_panic {
                        std::panic::resume_unwind(Box::new("private callback"));
                    }
                    Box::pin(async { std::panic::resume_unwind(Box::new("private future")) })
                })
                .unwrap();
            let (a, b) = tokio::time::timeout(Duration::from_secs(1), async {
                tokio::join!(
                    owner.shutdown(Instant::now() + Duration::from_millis(100)),
                    owner.shutdown(Instant::now() + Duration::from_millis(100))
                )
            })
            .await
            .unwrap();
            assert_eq!(a.unwrap_err().code(), "resource_shutdown_failed");
            assert_eq!(b.unwrap_err().code(), "resource_shutdown_failed");
            assert!(done.load(Ordering::Acquire));
        }
    }

    #[test]
    fn runtime_cancellation_publishes_interrupted_result_for_later_runtime() {
        let owner = Arc::new(ResourceOwner::new());
        let entered = Arc::new(Notify::new());
        let notice = Arc::clone(&entered);
        owner
            .registrar()
            .on_shutdown(move || {
                Box::pin(async move {
                    notice.notify_one();
                    std::future::pending().await
                })
            })
            .unwrap();
        let first = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        first.block_on(async {
            let copy = Arc::clone(&owner);
            let waiter = tokio::spawn(async move {
                copy.shutdown(Instant::now() + Duration::from_secs(30))
                    .await
            });
            entered.notified().await;
            waiter.abort();
            assert!(waiter.await.unwrap_err().is_cancelled());
        });
        drop(first);
        let second = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = second.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(1),
                owner.shutdown(Instant::now() + Duration::from_secs(1)),
            )
            .await
            .unwrap()
        });
        assert_eq!(result.unwrap_err().code(), "resource_shutdown_interrupted");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocked_task_poll_cannot_extend_shutdown_waiter_deadline() {
        let owner = ResourceOwner::new();
        let entered = Arc::new(Notify::new());
        let notice = Arc::clone(&entered);
        owner
            .registrar()
            .spawn(move |_| async move {
                notice.notify_one();
                // 故意模拟不合作第三方 poll；异步取消无法抢占正在执行的同步代码。
                std::thread::sleep(Duration::from_millis(250));
            })
            .unwrap();
        entered.notified().await;
        let started = Instant::now();
        let error = owner
            .shutdown(Instant::now() + Duration::from_millis(10))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "resource_shutdown_timeout");
        assert!(started.elapsed() < Duration::from_millis(200));
    }
}
