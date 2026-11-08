//! Provider 与入口 scope 共用的有界异步资源回收。

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::task::{Context, Poll};

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
    observed_tasks: Mutex<Vec<Weak<TaskCompletionInner>>>,
    outcome: OnceLock<Outcome>,
    finished: Notify,
    deadline: OnceLock<Instant>,
    children: Mutex<Children>,
    child_requested: Arc<Notify>,
    next_child: AtomicU64,
    child_failures: AtomicUsize,
}

/// 取消清理driver时，剩余独立回调也逐项处置，避免Vec隐式unwind清理。
struct CleanupQueue(Vec<Cleanup>);
impl CleanupQueue {
    fn discard(&mut self) -> usize {
        let mut failures = 0;
        while let Some(mut item) = self.pop() {
            failures += usize::from(item.discard());
        }
        failures
    }
    fn pop(&mut self) -> Option<CleanupItem> {
        self.0.pop().map(|cleanup| CleanupItem {
            value: Some(cleanup),
            // 来自已登记callback的实际槽；不是owner open状态推断出的新任务。
            ticket: ResourceDisposalTicket::admitted(),
            transferred: false,
        })
    }
}
impl Drop for CleanupQueue {
    fn drop(&mut self) {
        self.discard();
    }
}
struct CleanupItem {
    value: Option<Cleanup>,
    ticket: ResourceDisposalTicket,
    transferred: bool,
}
impl CleanupItem {
    fn credential(&self) -> super::lifecycle::disposal::Credential {
        super::lifecycle::disposal::Credential::Cleanup(self.ticket.clone())
    }
    fn discard(&mut self) -> bool {
        super::lifecycle::disposal::dispose(self.value.take(), Some(self.credential()));
        self.ticket.account.finish()
    }
}
impl Drop for CleanupItem {
    fn drop(&mut self) {
        if !self.transferred {
            self.discard();
        }
    }
}

/// 只保存原callback槽的票据；恢复同步上下文后才结束该槽。
struct CleanupRun {
    future: Option<BoxFuture<'static, Result<()>>>,
    ticket: ResourceDisposalTicket,
}
impl CleanupRun {
    fn clear(&mut self) {
        super::lifecycle::disposal::dispose(
            self.future.take(),
            Some(super::lifecycle::disposal::Credential::Cleanup(
                self.ticket.clone(),
            )),
        );
    }
    fn finish(&mut self) -> bool {
        self.clear();
        self.ticket.account.finish()
    }
}
impl Drop for CleanupRun {
    fn drop(&mut self) {
        self.finish();
    }
}
impl Future for CleanupRun {
    type Output = std::result::Result<Result<()>, ()>;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let (polled, _) = super::lifecycle::disposal::scope(
            Some(super::lifecycle::disposal::Credential::Cleanup(
                this.ticket.clone(),
            )),
            || {
                let result = super::lifecycle::catch_isolated(|| {
                    this.future
                        .as_mut()
                        .map_or(Poll::Ready(Ok(())), |future| future.as_mut().poll(context))
                });
                if result.is_err() {
                    super::lifecycle::disposal::failure(None);
                }
                result
            },
        );
        match polled {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(result)) => Poll::Ready(Ok(result)),
            Err(()) => Poll::Ready(Err(())),
        }
    }
}

/// 认领时已登记child退役责任；票据离开父锁后才执行关闭钩子。
struct CloseClaim {
    owner: Arc<ResourceOwner>,
    close: Option<Box<dyn FnOnce() + Send>>,
    cleanup: Option<ResourceCleanup>,
    parent: ResourceRegistrar,
}
impl CloseClaim {
    fn take(child: &mut ChildEntry, parent: &ResourceRegistrar) -> Option<Self> {
        child.close.as_ref()?;
        let cleanup = child.owner.registrar().begin_cleanup();
        Some(Self {
            owner: Arc::clone(&child.owner),
            close: child.close.take(),
            cleanup,
            parent: parent.clone(),
        })
    }
    fn invoke(&mut self) -> bool {
        let credential = self.cleanup.as_ref().map(|cleanup| {
            super::lifecycle::disposal::Credential::Cleanup(cleanup.disposal_ticket())
        });
        let (failed, _) = super::lifecycle::disposal::scope(credential, || {
            let failed = self
                .close
                .take()
                .is_some_and(|close| super::lifecycle::catch_isolated(close).is_err());
            if failed {
                super::lifecycle::disposal::failure(None);
            }
            failed
        });
        failed
    }
    fn finish(&mut self, failed: bool) {
        if let Some(mut cleanup) = self.cleanup.take() {
            cleanup.finish(usize::from(failed));
        } else if failed {
            self.parent.record_cleanup_failures(1);
        }
    }
}
impl Drop for CloseClaim {
    fn drop(&mut self) {
        if self.close.is_some() {
            let failed = self.invoke();
            self.owner.seal();
            self.finish(failed);
        } else {
            self.finish(false);
        }
    }
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
    driver_starting: bool,
    driver_life: Weak<DriverLife>,
    driver_child: Option<u64>,
}

/// 只记录同一Children中当前driver的生命期，不是另一调度器/任务表。
struct DriverLife {
    owner: Weak<Inner>,
    ended: AtomicBool,
    submitting: AtomicBool,
}
impl Children {
    fn clear_driver(&mut self) {
        self.driver_running = false;
        self.driver_starting = false;
        self.driver_life = Weak::new();
        if let Some(id) = self.driver_child.take()
            && let Some(child) = self.entries.get_mut(&id)
        {
            child.started = false;
        }
    }
    fn reconcile_driver(&mut self) {
        let valid = self.driver_life.upgrade().is_some_and(|life| {
            if self.driver_running {
                !life.ended.load(Ordering::Acquire)
            } else {
                self.driver_starting && life.submitting.load(Ordering::Acquire)
            }
        });
        if !valid {
            self.clear_driver();
        }
    }
}
struct DriverGuard(Arc<DriverLife>);
impl Drop for DriverGuard {
    fn drop(&mut self) {
        self.0.ended.store(true, Ordering::Release);
        if let Some(owner) = self.0.owner.upgrade() {
            let mut children = lock(&owner.children);
            if Weak::ptr_eq(&children.driver_life, &Arc::downgrade(&self.0))
                && (children.driver_running || !self.0.submitting.load(Ordering::Acquire))
            {
                children.clear_driver();
            }
        }
    }
}
/// 同步提交者拥有 `Starting`，panic 退出也释放预备责任；驱动守卫单独拥有 `Running`。
struct DriverSubmission(Arc<DriverLife>);
impl Drop for DriverSubmission {
    fn drop(&mut self) {
        self.0.submitting.store(false, Ordering::Release);
        if let Some(owner) = self.0.owner.upgrade() {
            let mut children = lock(&owner.children);
            if Weak::ptr_eq(&children.driver_life, &Arc::downgrade(&self.0)) {
                children.reconcile_driver();
            }
        }
    }
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

    /// 同步封闭新登记和全部子 scope，异步 join/回调仍由 shutdown 完成。
    pub(crate) fn seal(&self) {
        let callbacks = lock(&self.inner.callbacks);
        self.inner.sealed.store(true, Ordering::Release);
        let parent = self.registrar();
        let children: Vec<_> = {
            let mut children = lock(&self.inner.children);
            children
                .entries
                .values_mut()
                .map(|child| {
                    let claim = CloseClaim::take(child, &parent);
                    (Arc::clone(&child.owner), claim)
                })
                .collect()
        };
        drop(callbacks);
        Self::reclaim_observed(&self.inner);
        self.inner.tasks.cancel();
        #[cfg(test)]
        task_completion_tests_v7::after_claim(&self.inner);
        for (owner, mut claim) in children {
            let failed = claim.as_mut().is_some_and(CloseClaim::invoke);
            owner.seal();
            if let Some(claim) = &mut claim {
                claim.finish(failed);
            }
        }
    }
    fn shutdown_child(child: Arc<Self>, deadline: Instant) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move { child.shutdown(deadline).await })
    }

    fn reclaim_observed(inner: &Inner) {
        let observed: Vec<_> = lock(&inner.observed_tasks)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for completion in observed {
            let reclaim = lock(&completion.state).reclaim.take();
            if let Some(reclaim) = reclaim {
                // 回调只认领框架frame；其已知值在此锁外逐项处置，不poll/销毁producer Fut。
                if super::lifecycle::catch_isolated(reclaim).is_err() {
                    ResourceTaskCompletion { inner: completion }.note_unknown();
                }
            }
        }
    }

    async fn wait_observed(inner: &Inner, deadline: Instant, outcome: &mut Outcome) {
        let observed: Vec<_> = lock(&inner.observed_tasks)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        #[cfg(test)]
        task_completion_tests_v7::after_observed_snapshot(inner);
        for completion in observed {
            let completion = ResourceTaskCompletion { inner: completion };
            // 完成后的未知失败已记入 child_failures；这里只计入等待超时。
            if tokio::time::timeout_at(deadline, completion.wait())
                .await
                .is_err()
            {
                if !outcome.timed_out {
                    outcome.failures += 1;
                }
                outcome.timed_out = true;
            }
        }
    }

    async fn run_cleanup(mut cleanup: CleanupItem, deadline: Instant) -> Outcome {
        let Some(callback) = cleanup.value.take() else {
            return Outcome::default();
        };
        let (future, _) = super::lifecycle::disposal::scope(Some(cleanup.credential()), || {
            let result = super::lifecycle::catch_isolated(callback);
            if result.is_err() {
                super::lifecycle::disposal::failure(None);
            }
            result
        });
        let Ok(future) = future else {
            cleanup.discard();
            return Outcome {
                failures: 1,
                ..Outcome::default()
            };
        };
        let mut future = CleanupRun {
            future: Some(future),
            ticket: cleanup.ticket.clone(),
        };
        // 空Item不再结束同一ticket；取消时由真正持有Fut的CleanupRun收口。
        cleanup.transferred = true;
        let ticket = cleanup.ticket.clone();
        let result = tokio::time::timeout_at(deadline, &mut future).await;
        future.clear();
        let timed_out = result.is_err();
        let failed = !matches!(&result, Ok(Ok(Ok(()))));
        super::lifecycle::disposal::dispose(
            result,
            Some(super::lifecycle::disposal::Credential::Cleanup(ticket)),
        );
        let unknown = future.finish();
        Outcome {
            failures: usize::from(failed || unknown),
            timed_out,
            interrupted: false,
        }
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
            let parent = self.registrar();
            let children: Vec<_> = if callbacks.is_some() {
                lock(&self.inner.children)
                    .entries
                    .drain()
                    .map(|(_, mut child)| {
                        let claim = CloseClaim::take(&mut child, &parent);
                        (child.owner, claim)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            (callbacks.take().map(CleanupQueue), children)
        };
        let deadline = *self.inner.deadline.get().unwrap_or(&deadline);
        if let (Some(mut callbacks), children) = callbacks {
            // 在任何用户代码（包括关闭 scope 缓存时的 Drop）执行前建立完成保护。
            let completion = Completion {
                inner: Arc::clone(&self.inner),
                completed: false,
            };
            let close_failures = 0;
            let children: Vec<_> = children
                .into_iter()
                .map(|(owner, mut claim)| {
                    if let Some(claim) = &mut claim {
                        let failed = claim.invoke();
                        claim.finish(failed);
                    }
                    owner
                })
                .collect();
            Self::reclaim_observed(&self.inner);
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
                outcome.failures += inner.tasks.take_failures();
                Self::wait_observed(&inner, deadline, &mut outcome).await;
                for child in children {
                    if Self::shutdown_child(child, deadline).await.is_err() {
                        outcome.failures += 1;
                    }
                }
                outcome.failures += inner.child_failures.swap(0, Ordering::AcqRel);
                while let Some(cleanup) = callbacks.pop() {
                    let cleaned = Self::run_cleanup(cleanup, deadline).await;
                    outcome.failures += cleaned.failures;
                    outcome.timed_out |= cleaned.timed_out;
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
        let callbacks = lock(&self.inner.callbacks).take().map(CleanupQueue);
        Self::reclaim_observed(&self.inner);
        if let Some(mut callbacks) = callbacks {
            let failures = callbacks.discard();
            self.inner
                .child_failures
                .fetch_add(failures, Ordering::AcqRel);
        }
        // 已开始的 shutdown 自己持有 JoinSet；未显式关闭时 Drop 只负责中止所有者的任务。
        drop(self.inner.tasks.stop());
        self.inner
            .child_failures
            .fetch_add(self.inner.tasks.take_failures(), Ordering::AcqRel);
    }
}

// 只接受框架清理钩子；不作为用户资源回调接口。
type TaskFinished = Box<dyn FnOnce() + Send>;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum CompletionProgress {
    #[default]
    AwaitingRegistrationAndDrop,
    AwaitingRegistration,
    AwaitingDrop,
    Ready,
    Claimed,
}

impl CompletionProgress {
    fn mark(&mut self, registered: bool) {
        *self = match (*self, registered) {
            (Self::AwaitingRegistrationAndDrop, true) => Self::AwaitingDrop,
            (Self::AwaitingRegistrationAndDrop, false) => Self::AwaitingRegistration,
            (Self::AwaitingRegistration, true) | (Self::AwaitingDrop, false) => Self::Ready,
            (progress, _) => progress,
        };
    }
}

#[derive(Default)]
struct TaskCompletionState {
    progress: CompletionProgress,
    interrupted: bool,
    failure_recorded: bool,
    owner: Option<Weak<Inner>>,
    observed: bool,
    callback: Option<TaskFinished>,
    reclaim: Option<TaskFinished>,
    inline: Option<inline_owner::InlineState>,
}

impl TaskCompletionState {
    fn record_failure(&mut self) -> Option<Arc<Inner>> {
        self.interrupted = true;
        if self.failure_recorded {
            return None;
        }
        self.failure_recorded = true;
        let owner = self
            .observed
            .then(|| self.owner.as_ref().and_then(Weak::upgrade))
            .flatten();
        if let Some(owner) = &owner {
            owner.child_failures.fetch_add(1, Ordering::AcqRel);
        }
        owner
    }
}

struct TaskCompletionInner {
    state: Mutex<TaskCompletionState>,
    finished: AtomicBool,
    interrupted: AtomicBool,
    changed: Notify,
}

pub(crate) use super::lifecycle::TaskSubmissionFailure as ResourceTaskRejection;

pub(crate) use super::lifecycle::TaskExecutionFailure as ResourceTaskFailure;

/// 受管 future 及其全部捕获析构后的完成栅栏；不代表业务结果成功。
#[derive(Clone)]
pub(crate) struct ResourceTaskCompletion {
    inner: Arc<TaskCompletionInner>,
}

impl ResourceTaskCompletion {
    /// 钩子只能清框架索引/等待边；禁止执行用户代码或持有用户值。
    pub(crate) fn new(callback: impl FnOnce() + Send + 'static) -> Self {
        Self {
            inner: Arc::new(TaskCompletionInner {
                state: Mutex::new(TaskCompletionState {
                    callback: Some(Box::new(callback)),
                    ..TaskCompletionState::default()
                }),
                finished: AtomicBool::new(false),
                interrupted: AtomicBool::new(false),
                changed: Notify::new(),
            }),
        }
    }

    pub(crate) fn note_unknown(&self) {
        self.try_note_unknown();
    }

    /// 只向尚未认领完成的原责任报告；计数先于同一短锁下的完成认领。
    pub(crate) fn try_note_unknown(&self) -> bool {
        let owner = {
            let mut state = lock(&self.inner.state);
            if state.progress == CompletionProgress::Claimed || !state.observed {
                return false;
            }
            state.record_failure()
        };
        // 保活Arc的最后析构不能留在completion锁内。
        drop(owner);
        true
    }

    /// 只影响本调用诊断，不重复计入已由原责任接收的owner失败。
    pub(crate) fn note_diagnostic(&self) {
        let mut state = lock(&self.inner.state);
        if state.progress != CompletionProgress::Claimed {
            state.interrupted = true;
        }
    }

    fn mark(&self, registered: bool, interrupted: bool) {
        let (action, failed_owner) = {
            let mut state = lock(&self.inner.state);
            let failed_owner = if interrupted {
                state.record_failure()
            } else {
                None
            };
            state.progress.mark(registered);
            let inline_retired = state
                .inline
                .as_ref()
                .is_none_or(|inline| inline.released && inline.slots == 0);
            let action = if state.progress == CompletionProgress::Ready && inline_retired {
                state.progress = CompletionProgress::Claimed;
                Some((state.callback.take(), state.interrupted))
            } else {
                None
            };
            (action, failed_owner)
        };
        // 原子计数已在短事务中完成；只在锁外释放保活owner引用。
        drop(failed_owner);
        if let Some((callback, interrupted)) = action {
            // 登记失败也先离开 owner/task-set 锁，再进入框架索引锁。
            let callback_interrupted = callback
                .is_some_and(|callback| super::lifecycle::catch_isolated(callback).is_err());
            if callback_interrupted {
                self.mark(false, true);
            }
            self.inner
                .interrupted
                .store(interrupted || callback_interrupted, Ordering::Release);
            self.inner.finished.store(true, Ordering::Release);
            self.inner.changed.notify_waiters();
        }
    }

    /// 仅供根关闭在State中压缩已经完成的框架证据；不执行回调。
    pub(crate) fn finished_interrupted(&self) -> Option<bool> {
        self.inner
            .finished
            .load(Ordering::Acquire)
            .then(|| self.inner.interrupted.load(Ordering::Acquire))
    }

    /// 等待用户 future 与捕获已销毁，且框架索引清理已经完成。
    pub(crate) async fn wait(&self) -> Result<()> {
        loop {
            let notified = self.inner.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.inner.finished.load(Ordering::Acquire) {
                return if self.inner.interrupted.load(Ordering::Acquire) {
                    Err(Error::custom(
                        "resource_task_interrupted",
                        "受管任务或其捕获析构异常，不能确认业务成功",
                    ))
                } else {
                    Ok(())
                };
            }
            notified.await;
        }
    }
}

#[cfg(test)]
struct ObservedTask<F> {
    task: Option<tokio::sync::oneshot::Receiver<F>>,
    completion: Option<ResourceTaskCompletion>,
}
#[cfg(test)]
impl<F> Drop for ObservedTask<F> {
    fn drop(&mut self) {
        let credential = self.completion.as_ref().map(|completion| {
            super::lifecycle::disposal::Credential::Completion(completion.clone())
        });
        let report = super::lifecycle::disposal::dispose(self.task.take(), credential);
        if let Some(completion) = self.completion.take() {
            if report.seen {
                completion.note_diagnostic();
            }
            completion.mark(false, report.owned);
        }
    }
}
#[cfg(test)]
impl<F> ObservedTask<F> {
    fn start<Fut>(mut self, cancel: Cancellation) -> ObservedFuture<impl Future<Output = ()>>
    where
        F: FnOnce(Cancellation) -> Fut,
        Fut: Future<Output = ()>,
    {
        let task = self.task.take();
        let completion = self.completion.take();
        let interrupted = Arc::new(AtomicUsize::new(0));
        let signal = Arc::clone(&interrupted);
        ObservedFuture {
            future: Some(Box::pin(async move {
                if let Some(receiver) = task
                    && let Ok(task) = receiver.await
                {
                    super::lifecycle::controlled_task(task, cancel, signal).await;
                }
            })),
            completion,
            interrupted,
        }
    }
}
#[cfg(test)]
struct ObservedFuture<Fut> {
    future: Option<Pin<Box<Fut>>>,
    completion: Option<ResourceTaskCompletion>,
    interrupted: Arc<AtomicUsize>,
}
#[cfg(test)]
impl<Fut> ObservedFuture<Fut> {
    fn finish(&mut self, interrupted: bool) {
        // 用户析构异常也不能把已经发入 channel 的候选结果当成成功。
        let credential = self.completion.as_ref().map(|completion| {
            super::lifecycle::disposal::Credential::Completion(completion.clone())
        });
        let report = super::lifecycle::disposal::dispose(self.future.take(), credential);
        if let Some(completion) = self.completion.take() {
            if report.seen {
                completion.note_diagnostic();
            }
            completion.mark(
                false,
                interrupted || report.owned || self.interrupted.load(Ordering::Acquire) != 0,
            );
        }
    }
}
#[cfg(test)]
impl<Fut> Drop for ObservedFuture<Fut> {
    fn drop(&mut self) {
        self.finish(false);
    }
}
#[cfg(test)]
impl<Fut: Future<Output = ()>> Future for ObservedFuture<Fut> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let credential = this.completion.as_ref().map(|completion| {
            super::lifecycle::disposal::Credential::Completion(completion.clone())
        });
        let (polled, report) = super::lifecycle::disposal::scope(credential, || {
            let polled = super::lifecycle::catch_isolated(|| {
                this.future
                    .as_mut()
                    .map_or(Poll::Ready(()), |future| future.as_mut().poll(context))
            });
            if polled.is_err() {
                super::lifecycle::disposal::failure(None);
            }
            polled
        });
        if report.seen
            && let Some(completion) = &this.completion
        {
            completion.note_diagnostic();
        }
        match polled {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(())) => {
                this.finish(false);
                Poll::Ready(())
            }
            Err(()) => {
                this.finish(report.owned);
                Poll::Ready(())
            }
        }
    }
}

/// 不保存用户值；资格来自实际ResourceCleanup或已登记callback槽，到finish才结束。
#[derive(Clone)]
pub(crate) struct ResourceDisposalTicket {
    account: super::lifecycle::disposal::Account,
}
impl ResourceDisposalTicket {
    fn admitted() -> Self {
        Self {
            account: super::lifecycle::disposal::Account::admitted(),
        }
    }
    pub(crate) fn record(&self) -> bool {
        self.account.record()
    }
}

/// 仅框架已有数据的同步退役；复用同一owner的observed弱索引，不执行任务。
pub(crate) struct ResourceCleanup {
    completion: ResourceTaskCompletion,
    owner: ResourceRegistrar,
    finished: bool,
    disposal: ResourceDisposalTicket,
}
impl ResourceCleanup {
    fn admitted(completion: ResourceTaskCompletion, owner: ResourceRegistrar) -> Self {
        Self {
            completion,
            owner,
            finished: false,
            disposal: ResourceDisposalTicket::admitted(),
        }
    }
    pub(crate) fn disposal_ticket(&self) -> ResourceDisposalTicket {
        self.disposal.clone()
    }
    pub(crate) fn finish(&mut self, failures: usize) {
        if self.finished {
            return;
        }
        let failures = failures.max(usize::from(self.disposal.account.finish()));
        self.owner.record_cleanup_failures(failures);
        // completion自身不设owner，避免与上述逐项计数重复。
        self.completion.mark(false, failures != 0);
        self.finished = true;
    }
}
impl Drop for ResourceCleanup {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(1);
        }
    }
}

/// 资源登记的弱句柄；不保存应用或容器的强引用。
#[derive(Clone)]
pub struct ResourceRegistrar {
    inner: Weak<Inner>,
}

impl ResourceRegistrar {
    pub(crate) fn task_failures(&self) -> Arc<AtomicUsize> {
        self.inner.upgrade().map_or_else(
            || Arc::new(AtomicUsize::new(0)),
            |inner| inner.tasks.failure_counter(),
        )
    }

    /// 只给旧deferred JoinError边界构造fixture，不是普通spawn的替代验收。
    #[cfg(test)]
    pub(crate) fn test_legacy_spawn<F, Fut>(&self, task: F) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.inner
            .upgrade()
            .ok_or_else(resources_closed)?
            .tasks
            .handle()
            .test_legacy_spawn(task)
    }

    #[cfg(test)]
    pub(crate) fn test_outcome_failures(&self) -> Option<usize> {
        self.inner
            .upgrade()
            .and_then(|inner| inner.outcome.get().map(|outcome| outcome.failures))
    }

    #[cfg(test)]
    pub(crate) fn test_locks_available(&self) -> bool {
        self.inner.upgrade().is_some_and(|inner| {
            let callbacks_free = inner.callbacks.try_lock().is_ok();
            callbacks_free && inner.tasks.handle().test_lock_available()
        })
    }

    /// State→callbacks短登记已有cache的退役；sealed后仍可清理，但不越过shutdown接管。
    pub(crate) fn begin_cleanup(&self) -> Option<ResourceCleanup> {
        let inner = self.inner.upgrade()?;
        let callbacks = lock(&inner.callbacks);
        if callbacks.is_none() {
            return None;
        }
        let completion = ResourceTaskCompletion::new(|| {});
        let mut observed = lock(&inner.observed_tasks);
        observed.retain(|entry| {
            entry
                .upgrade()
                .is_some_and(|entry| !entry.finished.load(Ordering::Acquire))
        });
        lock(&completion.inner.state).observed = true;
        observed.push(Arc::downgrade(&completion.inner));
        drop(observed);
        completion.mark(true, false);
        Some(ResourceCleanup::admitted(completion, self.clone()))
    }

    /// raw交付引用的退役责任；sealed只可清理、不可执行新业务，take callbacks后拒绝登记。
    #[cfg(test)]
    pub(crate) fn begin_reclaim(
        &self,
        reclaim: impl FnOnce(ResourceDisposalTicket) + Send + 'static,
    ) -> Option<(ResourceCleanup, bool)> {
        let inner = self.inner.upgrade()?;
        let callbacks = lock(&inner.callbacks);
        if callbacks.is_none() {
            return None;
        }
        let completion = ResourceTaskCompletion::new(|| {});
        let cleanup = ResourceCleanup::admitted(completion.clone(), self.clone());
        let ticket = cleanup.disposal_ticket();
        {
            let mut state = lock(&completion.inner.state);
            state.observed = true;
            state.reclaim = Some(Box::new(move || reclaim(ticket)));
        }
        let mut observed = lock(&inner.observed_tasks);
        observed.retain(|entry| {
            entry
                .upgrade()
                .is_some_and(|entry| !entry.finished.load(Ordering::Acquire))
        });
        observed.push(Arc::downgrade(&completion.inner));
        drop(observed);
        completion.mark(true, false);
        let open = !inner.sealed.load(Ordering::Acquire);
        Some((cleanup, open))
    }

    /// 只累计实际owner的锁外析构未知，不运行用户代码或取得owner闸门。
    pub(crate) fn record_cleanup_failures(&self, failures: usize) {
        if failures != 0
            && let Some(inner) = self.inner.upgrade()
        {
            inner.child_failures.fetch_add(failures, Ordering::AcqRel);
        }
    }

    #[cfg(test)]
    pub(crate) fn test_after_seal_claim(&self, action: impl FnOnce() + Send + 'static) {
        task_completion_tests_v7::install_claim_hook(self.inner.clone(), Box::new(action));
    }
    #[cfg(test)]
    pub(crate) fn test_after_observed_snapshot(&self, action: impl FnOnce() + Send + 'static) {
        task_completion_tests_v7::install_snapshot_hook(self.inner.clone(), Box::new(action));
    }

    /// 比较真实资源 owner 身份，不公开地址，也不创建/延长 owner。
    pub(crate) fn same_owner(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.inner, &other.inner)
    }

    /// 在实际 owner 关闭闸门下作框架短提交；不得执行用户代码/析构或跨 await。
    ///
    /// 调用方持 State 时按 State→callbacks 次序；seal/shutdown 在调用子 scope
    /// 关闭钩子前释放 callbacks，因此不建立 callbacks→State 的反向依赖。
    pub(crate) fn with_open_owner<T>(&self, commit: impl FnOnce() -> T) -> Result<T> {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        let callbacks = lock(&inner.callbacks);
        if callbacks.is_none() || inner.sealed.load(Ordering::Acquire) {
            return Err(resources_closed());
        }
        Ok(commit())
    }

    /// 原producer跨锁外交接前先保完成责任；commit只移动框架持有权，不执行用户代码。
    pub(crate) fn reserve_observed<T>(
        &self,
        completion: &ResourceTaskCompletion,
        commit: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        let callbacks = lock(&inner.callbacks);
        if callbacks.is_none() || inner.sealed.load(Ordering::Acquire) {
            return Err(resources_closed());
        }
        let value = commit()?;
        {
            let mut state = lock(&completion.inner.state);
            state.owner = Some(self.inner.clone());
            state.observed = true;
        }
        let mut observed = lock(&inner.observed_tasks);
        // inline 不经过 spawn；每次准入也压缩原索引，pending 仍由同一栅栏保留。
        // 已完成的未知失败已计入 owner.child_failures，不依赖弱条目的长期保留。
        observed.retain(|entry| {
            entry
                .upgrade()
                .is_some_and(|entry| !entry.finished.load(Ordering::Acquire))
        });
        observed.push(Arc::downgrade(&completion.inner));
        drop(observed);
        // 只占原索引；正式handoff/拒绝F清完才登记最终registered。
        Ok(value)
    }

    /// 登记受管任务并在 future/捕获已析构后完成框架清理栅栏。
    #[cfg(test)]
    pub(crate) fn spawn_observed<F, Fut>(
        &self,
        completion: ResourceTaskCompletion,
        task: F,
    ) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        lock(&completion.inner.state).owner = Some(self.inner.clone());
        let (sender, receiver) = tokio::sync::oneshot::channel::<F>();
        // 用户 F 此时仍在本栈，不进入 owner/task-set 锁内的失败析构路径。
        let submission_completion = completion.clone();
        let observed = ObservedTask {
            task: Some(receiver),
            completion: Some(completion),
        };
        let completion = submission_completion;
        let result = (|| {
            let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
            tokio::runtime::Handle::try_current().map_err(|_| {
                Error::custom("resource_runtime_missing", "受管任务需要 Tokio 运行时")
            })?;
            let callbacks = lock(&inner.callbacks);
            if callbacks.is_none() || inner.sealed.load(Ordering::Acquire) {
                return Err(resources_closed());
            }
            lock(&completion.inner.state).observed = true;
            let mut observations = lock(&inner.observed_tasks);
            observations.retain(|entry| {
                entry
                    .upgrade()
                    .is_some_and(|entry| !entry.finished.load(Ordering::Acquire))
            });
            let weak = Arc::downgrade(&completion.inner);
            if !observations.iter().any(|entry| Weak::ptr_eq(entry, &weak)) {
                observations.push(weak);
            }
            drop(observations);
            Ok(inner
                .tasks
                .handle()
                .submit(move |cancel| observed.start(cancel)))
        })();
        let (outcome, report) = super::lifecycle::disposal::scope(
            Some(super::lifecycle::disposal::Credential::Completion(
                completion.clone(),
            )),
            || {
                match result {
                    Ok(submission) => submission.finish(),
                    Err(error) => super::lifecycle::TaskSubmissionOutcome {
                        registration: Err(error),
                        cleanup_interrupted: false,
                    },
                }
                .handoff(sender, task)
            },
        );
        if report.seen {
            completion.note_diagnostic();
        }
        completion.mark(true, report.owned);
        match outcome.registration {
            Err(error) => Err(error),
            Ok(()) if outcome.cleanup_interrupted => {
                Err(super::lifecycle::task_cleanup_interrupted())
            }
            Ok(()) => Ok(()),
        }
    }

    /// 提交子 scope 回收请求；同一父所有者至多运行一个回收驱动。
    pub(crate) fn request_child_cleanup(&self, id: u64, deadline: Instant) -> Result<()> {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        let callbacks = lock(&inner.callbacks);
        if callbacks.is_none() || inner.sealed.load(Ordering::Acquire) {
            return Ok(()); // 已封闭的父负责全部仍跟踪的子资源。
        }
        if inner.tasks.is_cancelled() {
            return Err(Error::custom(
                "resource_cleanup_cancelled",
                "父任务已取消，清理请求未取得驱动责任；需原owner关闭接管",
            ));
        }
        let life = Arc::new(DriverLife {
            owner: Arc::downgrade(&inner),
            ended: AtomicBool::new(false),
            submitting: AtomicBool::new(true),
        });
        {
            let mut children = lock(&inner.children);
            children.reconcile_driver();
            let Some(child) = children.entries.get_mut(&id) else {
                return Ok(());
            };
            child.requested = Some(deadline);
            if children.driver_running {
                inner.child_requested.notify_one();
                return Ok(());
            }
            if children.driver_starting {
                return Err(Error::custom(
                    "resource_cleanup_starting",
                    "子清理驱动尚在准入，当前请求未取得运行责任",
                ));
            }
            children.driver_starting = true;
            children.driver_life = Arc::downgrade(&life);
        }
        let _submission = DriverSubmission(Arc::clone(&life));
        let guard = DriverGuard(Arc::clone(&life));
        let weak = Arc::downgrade(&inner);
        let notify = Arc::clone(&inner.child_requested);
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let submission = inner
            .tasks
            .handle()
            .submit_cleanup_driver(move |cancel| async move {
                let _guard = guard;
                if receiver.await.is_ok() {
                    Self::drive_child_cleanup(weak, notify, cancel).await;
                }
            });
        drop(callbacks);
        let outcome = submission.finish();
        if outcome.cleanup_interrupted {
            inner.child_failures.fetch_add(1, Ordering::AcqRel);
        }
        outcome.into_result()?;
        // 单位启动信号与Running同一短提交；这里没有用户F/析构，也不跨await。
        let callbacks = lock(&inner.callbacks);
        let parent_closed = callbacks.is_none() || inner.sealed.load(Ordering::Acquire);
        let mut children = lock(&inner.children);
        children.driver_starting = false;
        if parent_closed {
            return Ok(());
        }
        let unavailable = || {
            Error::custom(
                "resource_cleanup_driver_unavailable",
                "子清理驱动未收到启动交接，当前请求未取得运行责任",
            )
        };
        if life.ended.load(Ordering::Acquire) {
            return Err(unavailable());
        }
        if inner.tasks.is_cancelled() {
            return Err(Error::custom(
                "resource_cleanup_cancelled",
                "父任务已取消，未交付清理驱动；需原owner关闭接管",
            ));
        }
        if sender.send(()).is_err() {
            return Err(unavailable());
        }
        children.driver_running = true;
        drop(children);
        drop(callbacks);
        inner.child_requested.notify_one();
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
                    None
                } else if let Some((id, child)) = children
                    .entries
                    .iter_mut()
                    .find(|(_, child)| child.requested.is_some() && !child.started)
                {
                    child.started = true;
                    let next = (*id, Arc::clone(&child.owner), child.requested);
                    children.driver_child = Some(next.0);
                    Some(next)
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
            let removed = {
                let mut children = lock(&inner.children);
                if children.driver_child == Some(id) {
                    children.driver_child = None;
                }
                let removed = children.entries.remove(&id);
                if removed.is_some() && failed {
                    inner.child_failures.fetch_add(1, Ordering::AcqRel);
                }
                removed
            };
            let child = removed.as_ref().map(|entry| Arc::clone(&entry.owner.inner));
            let interrupted = super::lifecycle::drop_isolated(removed);
            let discarded = child
                .as_ref()
                .map_or(0, |child| child.child_failures.load(Ordering::Acquire));
            inner
                .child_failures
                .fetch_add(discarded + usize::from(interrupted), Ordering::AcqRel);
        }
    }

    /// 返回同一资源所有者的后台任务句柄，供内置缓存刷新任务使用。
    pub(crate) fn background_handle(&self) -> Result<BackgroundTaskHandle> {
        let inner = self.inner.upgrade().ok_or_else(resources_closed)?;
        if lock(&inner.callbacks).is_none() || inner.sealed.load(Ordering::Acquire) {
            return Err(resources_closed());
        }
        Ok(inner.tasks.handle().with_resources(self.clone()))
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
    ///
    /// 登记拒绝与捕获/旧任务回收异常分别保留；清理未知不会作为成功返回。
    /// 已准入任务的执行/析构异常累计到同一owner，由shutdown报告失败。
    pub fn spawn<F, Fut>(&self, task: F) -> Result<()>
    where
        F: FnOnce(Cancellation) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let mut cleanup = self.begin_cleanup();
        let credential = cleanup.as_ref().map(|cleanup| {
            super::lifecycle::disposal::Credential::Cleanup(cleanup.disposal_ticket())
        });
        let inner = self.inner.upgrade();
        let failures = inner.as_ref().map_or_else(
            || Arc::new(AtomicUsize::new(0)),
            |inner| inner.tasks.failure_counter(),
        );
        let (sender, receiver) = tokio::sync::oneshot::channel::<F>();
        let submission = (|| {
            let inner = inner.as_ref().ok_or_else(resources_closed)?;
            tokio::runtime::Handle::try_current().map_err(|_| {
                Error::custom("resource_runtime_missing", "受管任务需要 Tokio 运行时")
            })?;
            let callbacks = lock(&inner.callbacks);
            if callbacks.is_none() || inner.sealed.load(Ordering::Acquire) {
                return Err(resources_closed());
            }
            Ok(inner.tasks.handle().submit(move |cancel| async move {
                if let Ok(task) = receiver.await {
                    super::lifecycle::controlled_task(task, cancel, failures).await;
                }
            }))
        })();
        let (outcome, _) = super::lifecycle::disposal::scope(credential, || {
            match submission {
                Ok(submission) => submission.finish(),
                Err(error) => super::lifecycle::TaskSubmissionOutcome {
                    registration: Err(error),
                    cleanup_interrupted: false,
                },
            }
            .handoff(sender, task)
        });
        if let Some(cleanup) = &mut cleanup {
            cleanup.finish(0);
        }
        outcome.into_result()
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

#[cfg(test)]
#[path = "resources/task_completion_tests.rs"]
mod task_completion_tests;

#[cfg(test)]
#[path = "resources/task_completion_tests_v5.rs"]
mod task_completion_tests_v5;

#[cfg(test)]
#[path = "resources/task_completion_tests_v6.rs"]
mod task_completion_tests_v6;

#[cfg(test)]
#[path = "resources/task_completion_tests_v7.rs"]
mod task_completion_tests_v7;

#[path = "resources/inline_owner.rs"]
mod inline_owner;

pub(crate) use inline_owner::{ResourceInlineOwner, ResourceInlineSlot};
