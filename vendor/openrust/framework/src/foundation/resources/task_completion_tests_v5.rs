//! v4独审：提前拒绝、正常join及跨runtime的driver责任。
use super::*;
use std::time::Duration;
fn fail(payload: impl std::any::Any + Send) -> ! {
    std::panic::resume_unwind(Box::new(payload))
}
struct Next(Arc<AtomicUsize>);
impl Drop for Next {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        fail("PRIVATE-NESTED-PAYLOAD");
    }
}
struct Broken(Arc<AtomicUsize>);
impl Drop for Broken {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        fail(Next(Arc::clone(&self.0)));
    }
}
struct Signal(Arc<Notify>);
impl Drop for Signal {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
#[tokio::test(flavor = "current_thread")]
async fn shutdown_alone_reaps_each_bad_join_payload_then_attempts_callbacks() -> Result<()> {
    let owner = ResourceOwner::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let cleaned = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&cleaned);
    owner.registrar().on_shutdown(move || {
        Box::pin(async move {
            count.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })
    })?;
    let child_cleaned = Arc::new(AtomicUsize::new(0));
    child(
        &owner.registrar(),
        Arc::clone(&child_cleaned),
        Arc::new(Notify::new()),
    )?;
    let mut signals = Vec::new();
    for _ in 0..2 {
        let done = Arc::new(Notify::new());
        let signal = Signal(Arc::clone(&done));
        let payload = Broken(Arc::clone(&attempts));
        owner.registrar().test_legacy_spawn(move |_| async move {
            let _signal = signal;
            fail(payload)
        })?;
        signals.push(done);
    }
    for done in signals {
        done.notified().await;
    }
    // 此后不再submit/try_join_next，只有shutdown的join_until负责旧payload。
    assert!(
        matches!(owner.shutdown(Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    assert_eq!(attempts.load(Ordering::Acquire), 4);
    assert_eq!(cleaned.load(Ordering::Acquire), 1);
    assert_eq!(child_cleaned.load(Ordering::Acquire), 1);
    Ok(())
}
fn assert_rejected(error: &Error, expected: &str, attempts: &AtomicUsize) -> Result<()> {
    assert_eq!(error.code(), "resource_task_interrupted");
    let source = std::error::Error::source(error)
        .and_then(|source| source.downcast_ref::<ResourceTaskRejection>())
        .ok_or_else(|| Error::custom("test_evidence_missing", "缺少拒绝及清理双诊断"))?;
    assert_eq!(source.registration.code(), expected);
    assert_eq!(source.cleanup.code(), "background_task_cleanup_interrupted");
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(!format!("{error:?} {source}").contains("PRIVATE-NESTED-PAYLOAD"));
    Ok(())
}
#[test]
fn ordinary_pre_admission_rejections_keep_original_error_and_isolate_user_capture() -> Result<()> {
    for mode in 0..3 {
        let owner = ResourceOwner::new();
        let registrar = owner.registrar();
        let inner = Arc::clone(&owner.inner);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::internal)?;
        if mode == 0 {
            drop(owner);
        } else if mode == 2 {
            owner.seal();
        }
        // mode0必须释放最后owner，inner仅在需要计数的路径保留。
        let observed = if mode == 0 {
            drop(inner);
            None
        } else {
            Some(inner)
        };
        let attempts = Arc::new(AtomicUsize::new(0));
        let capture = Broken(Arc::clone(&attempts));
        let action = || {
            registrar.spawn(move |_| async move {
                let _capture = capture;
            })
        };
        let result = if mode == 2 {
            let _runtime = runtime.enter();
            action()
        } else {
            action()
        };
        let error = result
            .err()
            .ok_or_else(|| Error::custom("test_rejection_missing", "本次登记应拒绝"))?;
        assert_rejected(
            &error,
            if mode == 1 {
                "resource_runtime_missing"
            } else {
                "resources_closed"
            },
            &attempts,
        )?;
        if let Some(inner) = observed {
            assert_eq!(inner.child_failures.load(Ordering::Acquire), 1);
        }
    }
    Ok(())
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::internal)
}
fn child(registrar: &ResourceRegistrar, count: Arc<AtomicUsize>, done: Arc<Notify>) -> Result<u64> {
    let owner = Arc::new(ResourceOwner::new());
    owner.registrar().on_shutdown(move || {
        Box::pin(async move {
            count.fetch_add(1, Ordering::AcqRel);
            done.notify_one();
            Ok(())
        })
    })?;
    registrar.track_child(owner, || {})
}
async fn wait_count(done: &Notify, count: &AtomicUsize, expected: usize) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let notified = done.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if count.load(Ordering::Acquire) == expected {
            return Ok(());
        }
        tokio::time::timeout_at(deadline, notified)
            .await
            .map_err(|_| Error::custom("test_cleanup_timeout", "驱动未完成实际子清理"))?;
    }
}
fn driver_drop_across_runtimes(mode: usize) -> Result<()> {
    let owner = ResourceOwner::new();
    let registrar = owner.registrar();
    let count = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(Notify::new());
    let first = runtime()?;
    let cleanup_drops = Arc::new(AtomicUsize::new(0));
    let first_id = if mode == 2 {
        let child = Arc::new(ResourceOwner::new());
        for _ in 0..2 {
            let capture = Broken(Arc::clone(&cleanup_drops));
            child.registrar().on_shutdown(move || {
                Box::pin(async move {
                    let _capture = capture;
                    Ok(())
                })
            })?;
        }
        let capture = Broken(Arc::clone(&cleanup_drops));
        let count = Arc::clone(&count);
        let entered = Arc::clone(&done);
        child.registrar().on_shutdown(move || {
            Box::pin(async move {
                let _capture = capture;
                count.fetch_add(1, Ordering::AcqRel);
                entered.notify_one();
                std::future::pending::<Result<()>>().await
            })
        })?;
        registrar.track_child(child, || {})?
    } else {
        child(&registrar, Arc::clone(&count), Arc::clone(&done))?
    };
    if mode == 0 {
        let _runtime = first.enter();
        registrar.request_child_cleanup(first_id, Instant::now() + Duration::from_secs(3))?;
        // 驱动尚未首次poll；空future本身必须已经拥有RAII责任。
    } else {
        first.block_on(async {
            registrar.request_child_cleanup(first_id, Instant::now() + Duration::from_secs(3))?;
            wait_count(&done, &count, 1).await?;
            if mode == 1 {
                tokio::time::timeout(Duration::from_secs(3), async {
                    while lock(&owner.inner.children).entries.contains_key(&first_id) {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .map_err(|_| Error::custom("test_driver_wait_timeout", "首个子项尚未退出driver"))?;
            }
            Ok::<(), Error>(())
        })?;
    }
    drop(first);
    assert_eq!(
        cleanup_drops.load(Ordering::Acquire),
        if mode == 2 { 6 } else { 0 }
    );
    assert!(!lock(&owner.inner.children).driver_running);
    assert!(!lock(&owner.inner.children).driver_starting);
    if mode == 2 {
        assert!(
            lock(&owner.inner.children)
                .entries
                .get(&first_id)
                .is_some_and(|entry| !entry.started)
        );
    }
    let second = runtime()?;
    second.block_on(async {
        if mode != 0 {
            let id = child(&registrar, Arc::clone(&count), Arc::clone(&done))?;
            registrar.request_child_cleanup(id, Instant::now() + Duration::from_secs(3))?;
        } else {
            registrar.request_child_cleanup(first_id, Instant::now() + Duration::from_secs(3))?;
        }
        wait_count(&done, &count, if mode == 0 { 1 } else { 2 }).await?;
        let result = owner
            .shutdown(Instant::now() + Duration::from_secs(3))
            .await;
        if mode == 2 {
            assert!(matches!(result,Err(error)if error.code()=="resource_shutdown_failed"));
        } else {
            result?;
        }
        Ok::<(), Error>(())
    })?;
    // in-flight子项重接原ResourceOwner一次性结果，不再次执行已进入的业务回调。
    assert_eq!(count.load(Ordering::Acquire), if mode == 0 { 1 } else { 2 });
    Ok(())
}
#[test]
fn driver_raii_covers_before_first_poll_running_and_inflight_child_on_runtime_drop() -> Result<()> {
    for mode in 0..3 {
        driver_drop_across_runtimes(mode)?;
    }
    Ok(())
}

struct ReleaseGc(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl Drop for ReleaseGc {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.0;
        *lock(mutex) = true;
        changed.notify_all();
    }
}
struct BlockGc {
    entered: std::sync::mpsc::SyncSender<()>,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
}
impl Drop for BlockGc {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let (mutex, changed) = &*self.gate;
        let mut released = lock(mutex);
        while !*released {
            released = changed
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}
fn failed_user_handoff_is_rejection(direct: bool) -> Result<()> {
    let owner = ResourceOwner::new();
    let registrar = owner.registrar();
    let runtime_a = runtime()?;
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release = ReleaseGc(Arc::clone(&gate));
    let old = BlockGc {
        entered: entered_tx,
        gate,
    };
    runtime_a.block_on(async {
        let done = Arc::new(Notify::new());
        let signal = Signal(Arc::clone(&done));
        registrar.test_legacy_spawn(move |_| async move {
            let _signal = signal;
            fail(old)
        })?;
        done.notified().await;
        Ok::<(), Error>(())
    })?;
    let runtime_handle = runtime_a.handle().clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let attempts = Arc::new(AtomicUsize::new(0));
    let capture = Broken(Arc::clone(&attempts));
    let submit = registrar.clone();
    let thread = std::thread::spawn(move || {
        let _runtime = runtime_handle.enter();
        let task = move |_| async move {
            let _capture = capture;
            count.fetch_add(1, Ordering::AcqRel);
        };
        if direct {
            submit.background_handle()?.spawn(task)
        } else {
            submit.spawn(task)
        }
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    // 空driver已登记、真实F尚留在提交线程；runtime销毁明确关闭接收端。
    drop(runtime_a);
    drop(release);
    let result = thread
        .join()
        .map_err(|_| Error::custom("test_thread_failed", "受控拒绝线程不应unwind"))?;
    let error = result
        .err()
        .ok_or_else(|| Error::custom("test_handoff_missing", "已知接收端关闭不能返回Ok"))?;
    assert_rejected(&error, "background_task_handoff_failed", &attempts)?;
    assert_eq!(calls.load(Ordering::Acquire), 0);
    runtime()?.block_on(async {
        assert!(matches!(owner.shutdown(Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed"));
        Ok(())
    })
}
#[test]
fn ordinary_and_direct_handle_failed_user_handoff_never_return_ok() -> Result<()> {
    failed_user_handoff_is_rejection(false)?;
    failed_user_handoff_is_rejection(true)
}

#[tokio::test]
async fn cleanup_callback_timeout_isolates_finite_capture_drop_then_attempts_next_callback()
-> Result<()> {
    let owner = ResourceOwner::new();
    let attempts = Arc::new(AtomicUsize::new(0));
    let cleaned = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(Notify::new());
    let notify = Arc::clone(&done);
    let count = Arc::clone(&cleaned);
    owner.registrar().on_shutdown(move || {
        Box::pin(async move {
            count.fetch_add(1, Ordering::AcqRel);
            notify.notify_one();
            Ok(())
        })
    })?;
    let capture = Broken(Arc::clone(&attempts));
    owner.registrar().on_shutdown(move || {
        Box::pin(async move {
            let _capture = capture;
            std::future::pending::<Result<()>>().await
        })
    })?;
    assert!(
        matches!(owner.shutdown(Instant::now()+Duration::from_millis(20)).await,Err(error)if error.code()=="resource_shutdown_timeout")
    );
    wait_count(&done, &cleaned, 1).await?;
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(cleaned.load(Ordering::Acquire), 1);
    assert!(
        owner
            .inner
            .outcome
            .get()
            .is_some_and(|outcome| outcome.timed_out && !outcome.interrupted)
    );
    Ok(())
}
