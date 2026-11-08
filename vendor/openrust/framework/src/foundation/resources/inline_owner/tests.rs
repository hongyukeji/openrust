//! 内联观察使用真实 owner；未执行的源码回归，不靠 spawned 用户任务代替调用者持有物。
use super::*;
use crate::foundation::resources::{
    CompletionProgress, ResourceInlineOwner as InlineOwner, ResourceInlineSlot as InlineSlot,
    ResourceOwner,
};
use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::time::Instant;

fn admitted(owner: &ResourceOwner) -> Result<InlineOwner> {
    let mut inline = InlineOwner::new();
    inline.reserve(&owner.registrar(), || Ok(()))?;
    inline.register()?;
    Ok(inline)
}

struct DropCount(Arc<AtomicUsize>);
impl Drop for DropCount {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

#[tokio::test]
async fn reservation_cancelled_before_register_completes_original_observation() -> Result<()> {
    let owner = ResourceOwner::new();
    let mut inline = ResourceInlineOwner::new();
    let completion = inline.completion();
    inline.reserve(&owner.registrar(), || Ok(()))?;
    assert_eq!(lock(&owner.inner.observed_tasks).len(), 1);
    assert!(
        lock(&completion.inner.state).progress == CompletionProgress::AwaitingRegistrationAndDrop
    );
    owner.seal();
    assert_eq!(completion.finished_interrupted(), None);
    drop(inline);
    assert_eq!(completion.finished_interrupted(), Some(false));
    completion.wait().await?;
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    Ok(())
}

#[tokio::test]
async fn final_independent_slot_owns_fence_after_guard_finish() -> Result<()> {
    let owner = ResourceOwner::new();
    let inline = admitted(&owner)?;
    let completion = inline.completion();
    let drops = Arc::new(AtomicUsize::new(0));
    let first: InlineSlot<DropCount> = inline.slot(DropCount(Arc::clone(&drops)))?;
    let second = inline.slot(DropCount(Arc::clone(&drops)))?;
    assert_eq!(lock(&owner.inner.observed_tasks).len(), 1);
    owner.seal();
    inline.finish();
    assert_eq!(completion.finished_interrupted(), None);
    drop(first);
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), None);
    drop(second);
    assert_eq!(drops.load(Ordering::Acquire), 2);
    assert_eq!(completion.finished_interrupted(), Some(false));
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    Ok(())
}

#[tokio::test]
async fn sealed_owner_rejects_admission_without_running_commit_or_charging_history() -> Result<()> {
    let owner = ResourceOwner::new();
    owner.seal();
    let mut inline = ResourceInlineOwner::new();
    let completion = inline.completion();
    let calls = AtomicUsize::new(0);
    let result = inline.reserve(&owner.registrar(), || {
        calls.fetch_add(1, Ordering::AcqRel);
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(lock(&owner.inner.observed_tasks).is_empty());
    assert!(!lock(&completion.inner.state).observed);
    drop(inline);
    assert_eq!(completion.finished_interrupted(), Some(false));
    assert_eq!(owner.inner.child_failures.load(Ordering::Acquire), 0);
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    Ok(())
}

#[tokio::test]
async fn rejected_commit_preserves_primary_and_never_registers_owner() -> Result<()> {
    let owner = ResourceOwner::new();
    let mut inline = ResourceInlineOwner::new();
    let result: Result<()> = inline.reserve(&owner.registrar(), || {
        Err(Error::custom("inline_source_replaced", "测试来源已经替换"))
    });
    assert_eq!(
        result.err().map(|error| error.code().to_owned()).as_deref(),
        Some("inline_source_replaced")
    );
    assert!(lock(&owner.inner.observed_tasks).is_empty());
    assert!(inline.register().is_err());
    drop(inline);
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    Ok(())
}

struct PendingFuture {
    polls: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl Future for PendingFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        self.polls.fetch_add(1, Ordering::AcqRel);
        Poll::Pending
    }
}
impl Drop for PendingFuture {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::AcqRel);
    }
}

#[tokio::test]
async fn owner_deadline_cannot_destroy_callers_pending_future() -> Result<()> {
    let owner = ResourceOwner::new();
    let inline = admitted(&owner)?;
    let completion = inline.completion();
    let polls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut slot = inline.slot(Box::pin(PendingFuture {
        polls: Arc::clone(&polls),
        drops: Arc::clone(&drops),
    }))?;
    poll_fn(|cx| {
        let result: Result<()> = (|| {
            let future = slot.get_mut().ok_or_else(invalid_state)?;
            assert!(inline.run(|| future.as_mut().poll(cx))?.is_pending());
            Ok(())
        })();
        Poll::Ready(result)
    })
    .await?;
    assert_eq!(polls.load(Ordering::Acquire), 1);
    inline.finish();
    let result = owner
        .shutdown(Instant::now() + Duration::from_millis(10))
        .await;
    assert_eq!(
        result.err().map(|error| error.code().to_owned()).as_deref(),
        Some("resource_shutdown_timeout")
    );
    assert_eq!(drops.load(Ordering::Acquire), 0);
    assert_eq!(completion.finished_interrupted(), None);
    drop(slot);
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), Some(false));
    // 真实晚退役不能把第一次 deadline 失败改写为成功。
    assert!(
        owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn never_polled_slot_survives_actual_owner_drop_until_caller_drop() -> Result<()> {
    let owner = ResourceOwner::new();
    let inline = admitted(&owner)?;
    let completion = inline.completion();
    let polls = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = inline.slot(Box::pin(PendingFuture {
        polls: Arc::clone(&polls),
        drops: Arc::clone(&drops),
    }))?;
    drop(owner);
    drop(inline);
    assert_eq!(polls.load(Ordering::Acquire), 0);
    assert_eq!(drops.load(Ordering::Acquire), 0);
    assert_eq!(completion.finished_interrupted(), None);
    drop(slot);
    assert_eq!(drops.load(Ordering::Acquire), 1);
    completion.wait().await?;
    Ok(())
}

struct Payload {
    remaining: usize,
    drops: Arc<AtomicUsize>,
}
impl Drop for Payload {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::AcqRel);
        if self.remaining != 0 {
            std::panic::resume_unwind(Box::new(Self {
                remaining: self.remaining - 1,
                drops: Arc::clone(&self.drops),
            }));
        }
    }
}
struct PollAndDropPanic {
    payload_drops: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
    registrar: ResourceRegistrar,
}
impl Future for PollAndDropPanic {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        std::panic::resume_unwind(Box::new(Payload {
            remaining: 2,
            drops: Arc::clone(&self.payload_drops),
        }))
    }
}
impl Drop for PollAndDropPanic {
    fn drop(&mut self) {
        // poll 的整个有限 payload 链必须先清完，再单独进入 Future Drop。
        assert_eq!(self.payload_drops.load(Ordering::Acquire), 3);
        assert!(self.registrar.test_locks_available());
        self.drops.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new(Payload {
            remaining: 1,
            drops: Arc::clone(&self.payload_drops),
        }));
    }
}

#[tokio::test]
async fn direct_poll_and_separate_drop_panic_finish_payloads_before_one_owner_failure() -> Result<()>
{
    let owner = ResourceOwner::new();
    let inline = admitted(&owner)?;
    let completion = inline.completion();
    let payload_drops = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let mut slot = inline.slot(Box::pin(PollAndDropPanic {
        payload_drops: Arc::clone(&payload_drops),
        drops: Arc::clone(&drops),
        registrar: owner.registrar(),
    }))?;
    poll_fn(|cx| {
        let result: Result<()> = (|| {
            let future = slot.get_mut().ok_or_else(invalid_state)?;
            assert!(inline.run(|| future.as_mut().poll(cx)).is_err());
            Ok(())
        })();
        Poll::Ready(result)
    })
    .await?;
    assert_eq!(payload_drops.load(Ordering::Acquire), 3);
    assert!(inline.check_cleanup().is_err());
    drop(inline);
    assert_eq!(completion.finished_interrupted(), None);
    slot.clear();
    assert_eq!(payload_drops.load(Ordering::Acquire), 5);
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), Some(true));
    assert_eq!(owner.inner.child_failures.load(Ordering::Acquire), 1);
    assert!(!disposal::seen());
    assert!(completion.wait().await.is_err());
    assert!(
        owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .is_err()
    );
    Ok(())
}

struct OriginalResponsibility(ResourceTaskCompletion);
impl Drop for OriginalResponsibility {
    fn drop(&mut self) {
        disposal::failure(Some(&disposal::Credential::Completion(self.0.clone())));
    }
}

#[tokio::test]
async fn original_live_completion_wins_over_current_inline_disposal() -> Result<()> {
    let original_owner = ResourceOwner::new();
    let current_owner = ResourceOwner::new();
    let original = admitted(&original_owner)?;
    let current = admitted(&current_owner)?;
    let current_done = current.completion();
    let slot = current.slot(OriginalResponsibility(original.completion()))?;
    drop(slot);
    assert!(current.check_cleanup().is_err());
    assert_eq!(
        original_owner.inner.child_failures.load(Ordering::Acquire),
        1
    );
    assert_eq!(
        current_owner.inner.child_failures.load(Ordering::Acquire),
        0
    );
    current.finish();
    original.finish();
    assert_eq!(current_done.finished_interrupted(), Some(true));
    assert!(
        original_owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .is_err()
    );
    current_owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    Ok(())
}

#[tokio::test]
async fn successful_delivery_moves_value_but_releases_empty_slot_outside_locks() -> Result<()> {
    let owner = ResourceOwner::new();
    let inline = admitted(&owner)?;
    let completion = inline.completion();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut slot = inline.slot(DropCount(Arc::clone(&drops)))?;
    inline.check_cleanup()?;
    let delivered = owner
        .registrar()
        .with_open_owner(|| slot.take())?
        .ok_or_else(invalid_state)?;
    inline.finish();
    assert_eq!(completion.finished_interrupted(), None);
    drop(slot);
    completion.wait().await?;
    assert_eq!(drops.load(Ordering::Acquire), 0);
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    drop(delivered);
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), Some(false));
    Ok(())
}

#[tokio::test]
async fn shutdown_waits_for_post_take_slot_retirement_and_seal_to_register_gap() -> Result<()> {
    let owner = ResourceOwner::new();
    let mut inline = ResourceInlineOwner::new();
    inline.reserve(&owner.registrar(), || Ok(()))?;
    owner.seal();
    // 本次已经 reserve；register 只确认原责任，不重新开放业务准入。
    inline.register()?;
    let drops = Arc::new(AtomicUsize::new(0));
    let slot = inline.slot(DropCount(Arc::clone(&drops)))?;
    let completion = inline.completion();
    let mut shutdown = Box::pin(owner.shutdown(Instant::now() + Duration::from_secs(1)));
    poll_fn(|cx| {
        assert!(shutdown.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(lock(&owner.inner.callbacks).is_none());
    inline.finish();
    assert_eq!(completion.finished_interrupted(), None);
    assert_eq!(drops.load(Ordering::Acquire), 0);
    drop(slot);
    shutdown.await?;
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), Some(false));
    Ok(())
}

struct IndependentPanic(Arc<AtomicUsize>);
impl Drop for IndependentPanic {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("独立参数槽析构失败"));
    }
}

#[tokio::test]
async fn independently_panicking_slots_all_retire_before_one_completion_charge() -> Result<()> {
    let owner = ResourceOwner::new();
    let inline = admitted(&owner)?;
    let completion = inline.completion();
    let drops = Arc::new(AtomicUsize::new(0));
    let first = inline.slot(IndependentPanic(Arc::clone(&drops)))?;
    let second = inline.slot(IndependentPanic(Arc::clone(&drops)))?;
    drop(inline);
    drop(first);
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), None);
    drop(second);
    assert_eq!(drops.load(Ordering::Acquire), 2);
    assert_eq!(completion.finished_interrupted(), Some(true));
    assert_eq!(owner.inner.child_failures.load(Ordering::Acquire), 1);
    assert!(!disposal::seen());
    assert!(
        owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn closed_original_completion_does_not_hide_current_actual_disposal() -> Result<()> {
    let original_owner = ResourceOwner::new();
    let current_owner = ResourceOwner::new();
    let original = admitted(&original_owner)?;
    let previous = original.completion();
    original.finish();
    previous.wait().await?;
    let current = admitted(&current_owner)?;
    let slot = current.slot(OriginalResponsibility(previous.clone()))?;
    drop(slot);
    assert_eq!(previous.finished_interrupted(), Some(false));
    assert_eq!(
        original_owner.inner.child_failures.load(Ordering::Acquire),
        0
    );
    assert_eq!(
        current_owner.inner.child_failures.load(Ordering::Acquire),
        1
    );
    assert!(current.check_cleanup().is_err());
    current.finish();
    original_owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    assert!(
        current_owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .is_err()
    );
    Ok(())
}

struct ReadyFuture {
    result: Option<DropCount>,
    capture: Option<DropCount>,
}
impl Future for ReadyFuture {
    type Output = Result<DropCount>;
    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(self.result.take().ok_or_else(invalid_state))
    }
}
impl Drop for ReadyFuture {
    fn drop(&mut self) {
        drop(self.capture.take());
    }
}

#[tokio::test]
async fn ready_future_capture_retires_before_actual_output_handoff() -> Result<()> {
    let owner = ResourceOwner::new();
    let inline = admitted(&owner)?;
    let completion = inline.completion();
    let result_drops = Arc::new(AtomicUsize::new(0));
    let capture_drops = Arc::new(AtomicUsize::new(0));
    let mut future = inline.slot(Box::pin(ReadyFuture {
        result: Some(DropCount(Arc::clone(&result_drops))),
        capture: Some(DropCount(Arc::clone(&capture_drops))),
    }))?;
    let output = poll_fn(|cx| {
        let result: Result<DropCount> = (|| {
            let future = future.get_mut().ok_or_else(invalid_state)?;
            match inline.run(|| future.as_mut().poll(cx))? {
                Poll::Ready(result) => result,
                Poll::Pending => Err(invalid_state()),
            }
        })();
        Poll::Ready(result)
    })
    .await?;
    let mut output = inline.slot(output)?;
    assert_eq!(capture_drops.load(Ordering::Acquire), 0);
    future.clear();
    assert_eq!(capture_drops.load(Ordering::Acquire), 1);
    assert_eq!(result_drops.load(Ordering::Acquire), 0);
    inline.check_cleanup()?;
    let delivered = owner
        .registrar()
        .with_open_owner(|| output.take())?
        .ok_or_else(invalid_state)?;
    output.clear();
    assert_eq!(completion.finished_interrupted(), None);
    inline.finish();
    completion.wait().await?;
    owner
        .shutdown(Instant::now() + Duration::from_secs(1))
        .await?;
    drop(delivered);
    assert_eq!(result_drops.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), Some(false));
    Ok(())
}

#[tokio::test]
async fn repeated_inline_admission_compacts_finished_entries_but_preserves_pending() -> Result<()> {
    let owner = ResourceOwner::new();
    let pending = admitted(&owner)?;
    let pending_done = pending.completion();
    let drops = Arc::new(AtomicUsize::new(0));
    let pending_slot = pending.slot(DropCount(Arc::clone(&drops)))?;
    pending.finish();
    // 保活全部已完成 completion，证明不是仅清理已失效的 Weak。
    let mut completed = Vec::new();
    for _ in 0..64 {
        let next = admitted(&owner)?;
        let done = next.completion();
        assert_eq!(lock(&owner.inner.observed_tasks).len(), 2);
        assert_eq!(pending_done.finished_interrupted(), None);
        next.finish();
        done.wait().await?;
        completed.push(done);
    }
    assert_eq!(completed.len(), 64);
    assert!(
        completed
            .iter()
            .all(|entry| entry.finished_interrupted() == Some(false))
    );
    assert_eq!(lock(&owner.inner.observed_tasks).len(), 2);
    let mut shutdown = Box::pin(owner.shutdown(Instant::now() + Duration::from_secs(1)));
    poll_fn(|cx| {
        assert!(shutdown.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let mut rejected = ResourceInlineOwner::new();
    assert!(rejected.reserve(&owner.registrar(), || Ok(())).is_err());
    assert_eq!(lock(&owner.inner.observed_tasks).len(), 2);
    assert_eq!(drops.load(Ordering::Acquire), 0);
    assert_eq!(pending_done.finished_interrupted(), None);
    drop(pending_slot);
    shutdown.await?;
    assert_eq!(drops.load(Ordering::Acquire), 1);
    assert_eq!(pending_done.finished_interrupted(), Some(false));
    Ok(())
}

#[tokio::test]
async fn compaction_keeps_prior_completed_failure_in_real_owner_account() -> Result<()> {
    let owner = ResourceOwner::new();
    let first = admitted(&owner)?;
    let failed = first.completion();
    let drops = Arc::new(AtomicUsize::new(0));
    drop(first.slot(IndependentPanic(Arc::clone(&drops)))?);
    first.finish();
    assert_eq!(failed.finished_interrupted(), Some(true));
    let next = admitted(&owner)?;
    assert_eq!(lock(&owner.inner.observed_tasks).len(), 1);
    assert_eq!(owner.inner.child_failures.load(Ordering::Acquire), 1);
    next.finish();
    assert!(
        owner
            .shutdown(Instant::now() + Duration::from_secs(1))
            .await
            .is_err()
    );
    assert_eq!(owner.registrar().test_outcome_failures(), Some(1));
    assert_eq!(drops.load(Ordering::Acquire), 1);
    Ok(())
}
