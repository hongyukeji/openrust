//! v2独审资源反例；探针只做非阻塞锁观测，负例不真的挂死测试runtime。
use super::parameters::{ParameterKey, ParameterOverrides, ParameterValue};
use super::*;
use crate::foundation::resources::{ResourceTaskFailure, ResourceTaskRejection};
use std::cell::RefCell;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
type CommitProbe = Box<dyn FnOnce(&ResourceRegistrar) -> Result<()>>;
std::thread_local! {static OWNER_PROBE:RefCell<Option<CommitProbe>>=const{RefCell::new(None)};}
pub(super) fn before_owner_commit(owner: &ResourceRegistrar) -> Result<()> {
    if let Some(probe) = OWNER_PROBE.take() {
        probe(owner)
    } else {
        Ok(())
    }
}
fn factory(container: &Container, key: &ServiceKey) -> Result<super::directory::FactoryHandle> {
    container
        .bindings_snapshot()?
        .into_iter()
        .find(|entry| &entry.key == key)
        .map(|entry| entry.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))
}

use std::sync::Condvar;

struct ClosingCapture {
    mode: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    gate: Arc<(Mutex<bool>, Condvar)>,
}
impl Drop for ClosingCapture {
    fn drop(&mut self) {
        match self.mode.load(Ordering::Acquire) {
            1 => std::panic::resume_unwind(Box::new("受控非根最终析构失败")),
            2 => {
                self.entered.notify_one();
                let (mutex, changed) = &*self.gate;
                let mut released = lock(mutex);
                while !*released {
                    released = changed
                        .wait(released)
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                }
            }
            _ => {}
        }
    }
}
struct ReleaseCaptures(Arc<(Mutex<bool>, Condvar)>);
impl Drop for ReleaseCaptures {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.0;
        *lock(mutex) = true;
        changed.notify_all();
    }
}
struct WakeRoot(Arc<Notify>);
impl Drop for WakeRoot {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_capacity_rejection_drop_and_raw_commit_do_not_reverse_owner_and_state_locks()
-> Result<()> {
    use std::sync::mpsc;
    struct RejectedCapture {
        registrar: ResourceRegistrar,
        container: Container,
        entered: mpsc::SyncSender<()>,
        free: Arc<AtomicBool>,
    }
    impl Drop for RejectedCapture {
        fn drop(&mut self) {
            self.free
                .store(self.registrar.test_locks_available(), Ordering::Release);
            let _ = self.entered.send(());
            let _ = self.container.contains::<usize>();
        }
    }
    let owner = Arc::new(ResourceOwner::new());
    let mut target = ContainerBuilder::new();
    target.resources = Some(owner.registrar());
    target.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = target.build();
    let resolver = target.resolve::<Resolver>().await?;
    owner.registrar().spawn(|cancel| async move {
        cancel.cancelled().await;
    })?;
    let (start_tx, start_rx) = mpsc::sync_channel(1);
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let entered_rx = Arc::new(Mutex::new(entered_rx));
    let free = Arc::new(AtomicBool::new(false));
    let rejected = RejectedCapture {
        registrar: owner.registrar(),
        container: target.clone(),
        entered: entered_tx,
        free: Arc::clone(&free),
    };
    let registrar = owner.registrar();
    let runtime = tokio::runtime::Handle::current();
    let rejected_thread = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        start_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(Error::internal)?;
        crate::foundation::lifecycle::with_test_task_limit(1, || {
            registrar.spawn(move |_| async move {
                let _ = &rejected;
            })
        })
    });
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |_| {
        let start = start_tx.clone();
        let entered = Arc::clone(&entered_rx);
        OWNER_PROBE.set(Some(Box::new(move |owner| {
            start.send(()).map_err(Error::internal)?;
            lock(&entered)
                .recv_timeout(Duration::from_secs(3))
                .map_err(Error::internal)?;
            if owner.test_locks_available() {
                Ok(())
            } else {
                Err(container_error(
                    "test_lock_inversion",
                    "普通拒绝仍持owner/task-set锁；不进入会死锁的提交",
                ))
            }
        })));
        Box::pin(async { Ok(Arc::new(1)) })
    });
    let source = source.build();
    let handle = factory(&source, &ServiceKey::typed::<usize>())?;
    let raw = handle
        .invoke_internal::<usize>((*resolver).clone(), ParameterOverrides::new())
        .await;
    let rejected = rejected_thread
        .join()
        .map_err(|_| container_error("test_thread_failed", "拒绝线程失败"))?;
    assert!(matches!(rejected,Err(error)if error.code()=="background_task_capacity"));
    assert!(free.load(Ordering::Acquire));
    assert_eq!(*raw?, 1);
    owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    Ok(())
}

#[tokio::test]
async fn rejected_raw_parameter_drop_reports_both_registration_and_cleanup_without_secret_payload()
-> Result<()> {
    struct BrokenParameter;
    impl Drop for BrokenParameter {
        fn drop(&mut self) {
            std::panic::resume_unwind(Box::new("PRIVATE-PARAMETER"));
        }
    }
    for closed in [false, true] {
        let owner = Arc::new(ResourceOwner::new());
        let mut target = ContainerBuilder::new();
        target.resources = Some(owner.registrar());
        target.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
        let target = target.build();
        let resolver = target.resolve::<Resolver>().await?;
        if closed {
            owner.seal();
        } else {
            owner.registrar().spawn(|cancel| async move {
                cancel.cancelled().await;
            })?;
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&calls);
        let mut source = ContainerBuilder::new();
        source.bind::<usize, _>(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Arc::new(1)) })
        });
        let source = source.build();
        let handle = factory(&source, &ServiceKey::typed::<usize>())?;
        let mut params = ParameterOverrides::new();
        params.insert(
            ParameterKey::name("private"),
            ParameterValue::new(BrokenParameter),
        );
        let mut raw = handle.invoke_internal::<usize>((*resolver).clone(), params);
        let polled = crate::foundation::lifecycle::with_test_task_limit(1, || {
            let mut context = std::task::Context::from_waker(futures_util::task::noop_waker_ref());
            raw.as_mut().poll(&mut context)
        });
        let std::task::Poll::Ready(Err(error)) = polled else {
            return Err(container_error(
                "test_rejection_missing",
                "拒绝应返回清理异常",
            ));
        };
        assert_eq!(error.code(), "resource_task_interrupted");
        let evidence = std::error::Error::source(&error)
            .ok_or_else(|| container_error("test_evidence_missing", "缺少双错误来源"))?;
        // 封闭在准入前退役调用参数；容量耗尽在已准入的任务提交阶段拒绝。
        let (primary, cleanup) = if closed {
            let failure = evidence
                .downcast_ref::<ResourceTaskFailure>()
                .ok_or_else(|| {
                    container_error("test_evidence_missing", "缺少调用参数退役双错误")
                })?;
            (&failure.primary, &failure.cleanup)
        } else {
            let failure = evidence
                .downcast_ref::<ResourceTaskRejection>()
                .ok_or_else(|| {
                    container_error("test_evidence_missing", "缺少任务提交拒绝双错误")
                })?;
            (&failure.registration, &failure.cleanup)
        };
        assert_eq!(
            primary.code(),
            if closed {
                "resources_closed"
            } else {
                "background_task_capacity"
            }
        );
        assert_eq!(cleanup.code(), "resource_task_interrupted");
        assert!(!format!("{evidence:?} {evidence}").contains("PRIVATE-PARAMETER"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(lock(&target.inner.state).raw_attempts.is_empty());
        assert!(
            matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn root_seal_snapshots_nonroot_failures_and_waits_every_fence_despite_first_error()
-> Result<()> {
    let owner = ResourceOwner::new();
    let mut target = ContainerBuilder::new();
    target.resources = Some(owner.registrar());
    target.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = target.build();
    let resolver = target.resolve::<Resolver>().await?;
    let root_owner = Arc::clone(&lock(&target.inner.state).root_resources);
    let root_entered = Arc::new(Notify::new());
    let root_gate = Arc::new(Notify::new());
    let wake_root = WakeRoot(Arc::clone(&root_gate));
    let entered = Arc::clone(&root_entered);
    let gate = Arc::clone(&root_gate);
    root_owner.registrar().on_shutdown(move || {
        Box::pin(async move {
            entered.notify_one();
            gate.notified().await;
            Ok(())
        })
    })?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = ReleaseCaptures(Arc::clone(&gate));
    let modes = [Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0))];
    let started = [Arc::new(Notify::new()), Arc::new(Notify::new())];
    let blocked = [Arc::new(Notify::new()), Arc::new(Notify::new())];
    let mut source = ContainerBuilder::new();
    for index in 0..2 {
        let capture = ClosingCapture {
            mode: Arc::clone(&modes[index]),
            entered: Arc::clone(&blocked[index]),
            gate: Arc::clone(&gate),
        };
        let started = Arc::clone(&started[index]);
        source.register_key::<usize, _>(
            ServiceKey::named(if index == 0 { "a" } else { "b" }),
            Lifetime::Bind,
            move |_| {
                std::hint::black_box(&capture);
                let started = Arc::clone(&started);
                Box::pin(async move {
                    started.notify_one();
                    std::future::pending::<Result<Arc<usize>>>().await
                })
            },
            false,
        );
    }
    let source = source.build();
    let a = factory(&source, &ServiceKey::named("a"))?;
    let b = factory(&source, &ServiceKey::named("b"))?;
    let mut raw_a = a.invoke_internal::<usize>((*resolver).clone(), ParameterOverrides::new());
    let mut raw_b = b.invoke_internal::<usize>((*resolver).clone(), ParameterOverrides::new());
    drop(a);
    drop(b);
    drop(source);
    assert!(futures_util::poll!(raw_a.as_mut()).is_pending());
    assert!(futures_util::poll!(raw_b.as_mut()).is_pending());
    started[0].notified().await;
    started[1].notified().await;
    let (first, failed_fence) = {
        let state = lock(&target.inner.state);
        let record = state
            .raw_attempts
            .values()
            .next()
            .ok_or_else(|| container_error("test_raw_missing", "缺少在途raw"))?;
        (
            usize::from(record.attempt.service.name() == "b"),
            record.completion.clone(),
        )
    };
    modes[first].store(1, Ordering::Release);
    modes[1 - first].store(2, Ordering::Release);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut shutdown = Box::pin(target.shutdown_root(deadline));
    assert!(futures_util::poll!(shutdown.as_mut()).is_pending());
    root_entered.notified().await;
    assert!(
        matches!(failed_fence.wait().await,Err(error)if error.code()=="resource_task_interrupted")
    );
    blocked[1 - first].notified().await;
    drop(wake_root);
    root_owner.shutdown(deadline).await?;
    // 首个栅栏已失败且根清理已完成，仍必须等待另一个实际析构。
    assert!(futures_util::poll!(shutdown.as_mut()).is_pending());
    drop(release);
    assert!(matches!(shutdown.await,Err(error)if error.code()=="resource_task_interrupted"));
    assert!(raw_a.await.is_err());
    assert!(raw_b.await.is_err());
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(owner.shutdown(deadline).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
