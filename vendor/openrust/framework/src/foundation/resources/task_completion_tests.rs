//! 所有者闸门和 future 析构完成栅栏的确定性边界。
use super::*;

#[test]
fn owner_gate_checks_and_commits_under_same_owner_mutex() -> Result<()> {
    let owner = ResourceOwner::new();
    let registrar = owner.registrar();
    let probe = Arc::clone(&owner.inner);
    assert!(registrar.with_open_owner(|| probe.callbacks.try_lock().is_err())?);
    owner.seal();
    let ran = AtomicBool::new(false);
    assert!(
        matches!(registrar.with_open_owner(||ran.store(true,Ordering::Release)),Err(error)if error.code()=="resources_closed")
    );
    assert!(!ran.load(Ordering::Acquire));
    Ok(())
}
struct MarkDrop(Arc<AtomicBool>);
impl Drop for MarkDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[tokio::test]
async fn observed_future_drops_all_captures_before_completion_hook_and_waiter() -> Result<()> {
    let owner = ResourceOwner::new();
    let dropped = Arc::new(AtomicBool::new(false));
    let hook = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&dropped);
    let done = Arc::clone(&hook);
    let completion = ResourceTaskCompletion::new(move || {
        done.store(observed.load(Ordering::Acquire), Ordering::Release);
    });
    let guard = MarkDrop(Arc::clone(&dropped));
    owner
        .registrar()
        .spawn_observed(completion.clone(), move |cancel| async move {
            let _keep = &guard;
            cancel.cancelled().await;
        })?;
    owner
        .shutdown(Instant::now() + std::time::Duration::from_secs(3))
        .await?;
    completion.wait().await?;
    assert!(dropped.load(Ordering::Acquire));
    assert!(hook.load(Ordering::Acquire));
    Ok(())
}

#[tokio::test]
async fn failed_registration_drops_submission_then_runs_hook_outside_owner_lock() -> Result<()> {
    let owner = ResourceOwner::new();
    owner.seal();
    let dropped = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&dropped);
    let inner = Arc::clone(&owner.inner);
    let outside = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&outside);
    let completion = ResourceTaskCompletion::new(move || {
        done.store(
            observed.load(Ordering::Acquire) && inner.callbacks.try_lock().is_ok(),
            Ordering::Release,
        );
    });
    let guard = MarkDrop(Arc::clone(&dropped));
    let status = owner
        .registrar()
        .spawn_observed(completion.clone(), move |_| async move {
            let _keep = &guard;
        });
    assert!(matches!(status,Err(error)if error.code()=="resources_closed"));
    completion.wait().await?;
    assert!(dropped.load(Ordering::Acquire));
    assert!(outside.load(Ordering::Acquire));
    Ok(())
}

#[tokio::test]
async fn unknown_poll_failure_completes_fence_as_error_and_is_recorded_by_owner() -> Result<()> {
    fn fail_poll() {
        std::panic::resume_unwind(Box::new("受控任务轮询失败"));
    }
    let owner = ResourceOwner::new();
    let cleaned = Arc::new(AtomicBool::new(false));
    let marker = Arc::clone(&cleaned);
    let completion = ResourceTaskCompletion::new(move || marker.store(true, Ordering::Release));
    owner
        .registrar()
        .spawn_observed(completion.clone(), |_| async {
            fail_poll();
        })?;
    assert!(
        matches!(completion.wait().await,Err(error)if error.code()=="resource_task_interrupted")
    );
    assert!(cleaned.load(Ordering::Acquire));
    assert!(
        matches!(owner.shutdown(Instant::now()+std::time::Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn all_spawn_variants_reap_user_panic_payload_outside_owner_and_taskset_locks() -> Result<()>
{
    use crate::container::ContainerBuilder;
    struct Payload {
        registrar: ResourceRegistrar,
        container: crate::container::Container,
        free: Arc<AtomicBool>,
        dropped: Arc<AtomicBool>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            self.free
                .store(self.registrar.test_locks_available(), Ordering::Release);
            let _ = self.container.contains::<usize>();
            self.dropped.store(true, Ordering::Release);
        }
    }
    struct Signal(Arc<Notify>);
    impl Drop for Signal {
        fn drop(&mut self) {
            self.0.notify_one();
        }
    }
    fn fail(payload: Payload) {
        std::panic::resume_unwind(Box::new(payload));
    }
    for mode in 0..3 {
        let owner = ResourceOwner::new();
        let registrar = owner.registrar();
        if mode == 2 {
            registrar.spawn(|cancel| async move {
                cancel.cancelled().await;
            })?;
        }
        let free = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let done = Arc::new(Notify::new());
        let payload = Payload {
            registrar: registrar.clone(),
            container: ContainerBuilder::new().build(),
            free: Arc::clone(&free),
            dropped: Arc::clone(&dropped),
        };
        let finished = Arc::clone(&done);
        registrar.test_legacy_spawn(move |_| async move {
            let _signal = Signal(finished);
            fail(payload);
        })?;
        // current_thread在本任务恢复前已完成panic任务的harness，JoinError已可回收。
        done.notified().await;
        if mode == 2 {
            let child = Arc::new(ResourceOwner::new());
            let id = registrar.track_child(child, || {})?;
            let result = crate::foundation::lifecycle::with_test_task_limit(0, || {
                registrar
                    .request_child_cleanup(id, Instant::now() + std::time::Duration::from_secs(3))
            });
            assert!(matches!(result,Err(error)if error.code()=="background_task_capacity"));
            assert!(!lock(&owner.inner.children).driver_running);
        } else if mode == 1 {
            let completion = ResourceTaskCompletion::new(|| {});
            registrar.spawn_observed(completion.clone(), |_| async {})?;
            completion.wait().await?;
        } else {
            registrar.spawn(|_| async {})?;
        }
        assert!(dropped.load(Ordering::Acquire));
        assert!(free.load(Ordering::Acquire));
        owner
            .shutdown(Instant::now() + std::time::Duration::from_secs(3))
            .await?;
    }
    Ok(())
}

struct ReleaseDriverGate(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl Drop for ReleaseDriverGate {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.0;
        *lock(mutex) = true;
        changed.notify_all();
    }
}
struct DriverPayload {
    registrar: ResourceRegistrar,
    entered: Arc<Notify>,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
    outside: Arc<AtomicBool>,
}
impl Drop for DriverPayload {
    fn drop(&mut self) {
        self.outside
            .store(self.registrar.test_locks_available(), Ordering::Release);
        self.entered.notify_one();
        let (mutex, changed) = &*self.gate;
        let mut released = lock(mutex);
        while !*released {
            released = changed
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}
struct DriverSignal(Arc<Notify>);
impl Drop for DriverSignal {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
fn driver_fail(payload: DriverPayload) {
    std::panic::resume_unwind(Box::new(payload));
}
async fn pending_driver_cannot_accept_another_request(abort_handoff: bool) -> Result<()> {
    let owner = Arc::new(ResourceOwner::new());
    let registrar = owner.registrar();
    registrar.spawn(|cancel| async move {
        cancel.cancelled().await;
    })?;
    let cleaned = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(Notify::new());
    let mut ids = Vec::new();
    for _ in 0..2 {
        let child = Arc::new(ResourceOwner::new());
        let count = Arc::clone(&cleaned);
        let notify = Arc::clone(&done);
        child.registrar().on_shutdown(move || {
            Box::pin(async move {
                count.fetch_add(1, Ordering::AcqRel);
                notify.notify_one();
                Ok(())
            })
        })?;
        ids.push(registrar.track_child(child, || {})?);
    }
    let entered = Arc::new(Notify::new());
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release = ReleaseDriverGate(Arc::clone(&gate));
    let outside = Arc::new(AtomicBool::new(false));
    let old_done = Arc::new(Notify::new());
    let payload = DriverPayload {
        registrar: registrar.clone(),
        entered: Arc::clone(&entered),
        gate,
        outside: Arc::clone(&outside),
    };
    let signal = DriverSignal(Arc::clone(&old_done));
    registrar.test_legacy_spawn(move |_| async move {
        let _signal = signal;
        driver_fail(payload);
    })?;
    old_done.notified().await;
    // 独立线程在GC Drop闸门等待，B可在同一运行时观察尚未admission的Starting。
    let runtime = tokio::runtime::Handle::current();
    let first = registrar.clone();
    let id = ids[0];
    let deadline = Instant::now() + std::time::Duration::from_secs(3);
    let thread = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        crate::foundation::lifecycle::with_test_task_limit(
            if abort_handoff { 256 } else { 0 },
            || first.request_child_cleanup(id, deadline),
        )
    });
    entered.notified().await;
    let second = registrar.request_child_cleanup(ids[1], deadline);
    assert!(matches!(second,Err(error)if error.code()=="resource_cleanup_starting"));
    assert_eq!(cleaned.load(Ordering::Acquire), 0);
    assert!(outside.load(Ordering::Acquire));
    if abort_handoff {
        // 受控终止已登记但尚未handoff的空driver；父回调仍开放，不能假称父已接管。
        let mut tasks = owner.inner.tasks.stop();
        tasks.abort_all();
        while let Some(result) = tasks.join_next().await {
            drop(result);
        }
    }
    drop(release);
    let first = thread
        .join()
        .map_err(|_| Error::custom("test_thread_failed", "驱动请求线程失败"))?;
    assert!(
        matches!(first,Err(error)if error.code()==if abort_handoff { "resource_cleanup_driver_unavailable" } else { "background_task_capacity" })
    );
    assert!(!lock(&owner.inner.children).driver_starting);
    assert!(!lock(&owner.inner.children).driver_running);
    if abort_handoff {
        // 已拒绝的两次请求仍由原owner跟踪，父关闭实际清理两个子资源。
        owner.shutdown(deadline).await?;
    } else {
        registrar.request_child_cleanup(ids[1], deadline)?;
        loop {
            let notified = done.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if cleaned.load(Ordering::Acquire) == 2 {
                break;
            }
            tokio::time::timeout_at(deadline, notified)
                .await
                .map_err(|_| Error::custom("test_cleanup_timeout", "驱动未清理两个请求"))?;
        }
        owner.shutdown(deadline).await?;
    }
    assert_eq!(cleaned.load(Ordering::Acquire), 2);
    Ok(())
}
#[tokio::test(flavor = "current_thread")]
async fn concurrent_request_cannot_succeed_on_failed_provisional_driver_or_handoff() -> Result<()> {
    pending_driver_cannot_accept_another_request(false).await?;
    pending_driver_cannot_accept_another_request(true).await
}
