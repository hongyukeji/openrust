//! v3独审的首次封闭、复合回收与预备准入反例。
use super::parameters::ParameterOverrides;
use super::*;
use crate::foundation::resources::{ResourceTaskCompletion, ResourceTaskRejection};
use std::sync::Condvar;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

fn factory(container: &Container, key: &ServiceKey) -> Result<super::directory::FactoryHandle> {
    container
        .bindings_snapshot()?
        .into_iter()
        .find(|entry| &entry.key == key)
        .map(|entry| entry.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))
}
struct ReleaseGate(Arc<(Mutex<bool>, Condvar)>);
impl Drop for ReleaseGate {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.0;
        *lock(mutex) = true;
        changed.notify_all();
    }
}
struct Wake(Arc<Notify>);
impl Drop for Wake {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
struct FinalCapture {
    fail: bool,
    entered: Arc<Notify>,
    gate: Arc<(Mutex<bool>, Condvar)>,
}
impl Drop for FinalCapture {
    fn drop(&mut self) {
        if self.fail {
            std::panic::resume_unwind(Box::new("PRIVATE-FINAL-CAPTURE"));
        }
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
struct CloseFixture {
    release: Option<ReleaseGate>,
    target: Container,
    owner: Arc<ResourceOwner>,
    root: Arc<ResourceOwner>,
    raw: Vec<BoxFuture<'static, Result<Arc<usize>>>>,
    failed: ResourceTaskCompletion,
    blocked: Arc<Notify>,
    root_entered: Arc<Notify>,
    root_gate: Arc<Notify>,
}
async fn close_fixture() -> Result<CloseFixture> {
    let owner = Arc::new(ResourceOwner::new());
    let mut target = ContainerBuilder::new();
    target.resources = Some(owner.registrar());
    target.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = target.build();
    let resolver = target.resolve::<Resolver>().await?;
    let root = Arc::clone(&lock(&target.inner.state).root_resources);
    let root_entered = Arc::new(Notify::new());
    let root_gate = Arc::new(Notify::new());
    let entered = Arc::clone(&root_entered);
    let root_wait = Arc::clone(&root_gate);
    root.registrar().on_shutdown(move || {
        Box::pin(async move {
            entered.notify_one();
            root_wait.notified().await;
            Ok(())
        })
    })?;
    let blocked = Arc::new(Notify::new());
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let started = [Arc::new(Notify::new()), Arc::new(Notify::new())];
    let mut source = ContainerBuilder::new();
    for (index, signal) in started.iter().enumerate() {
        let capture = FinalCapture {
            fail: index == 0,
            entered: Arc::clone(&blocked),
            gate: Arc::clone(&gate),
        };
        let started = Arc::clone(signal);
        source.register_key::<usize, _>(
            ServiceKey::named(if index == 0 { "failed" } else { "blocked" }),
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
    let mut raw = Vec::new();
    let release = ReleaseGate(Arc::clone(&gate));
    for key in ["failed", "blocked"] {
        raw.push(
            factory(&source, &ServiceKey::named(key))?
                .invoke_internal::<usize>((*resolver).clone(), ParameterOverrides::new()),
        );
    }
    drop(source);
    for call in &mut raw {
        assert!(futures_util::poll!(call.as_mut()).is_pending());
    }
    for started in started {
        started.notified().await;
    }
    let failed = lock(&target.inner.state)
        .raw_attempts
        .values()
        .find(|record| record.attempt.service.name() == "failed")
        .map(|record| record.completion.clone())
        .ok_or_else(|| container_error("test_raw_missing", "缺少失败raw"))?;
    Ok(CloseFixture {
        release: Some(release),
        target,
        owner,
        root,
        raw,
        failed,
        blocked,
        root_entered,
        root_gate,
    })
}
async fn split_close_retains_evidence(cancel_first: bool) -> Result<()> {
    let mut fixture = close_fixture().await?;
    let release = fixture
        .release
        .take()
        .ok_or_else(|| container_error("test_gate_missing", "缺少释放闸门"))?;
    let wake = Wake(Arc::clone(&fixture.root_gate));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    if cancel_first {
        let mut first = Box::pin(fixture.target.shutdown_root(deadline));
        assert!(futures_util::poll!(first.as_mut()).is_pending());
        fixture.root_entered.notified().await;
        drop(first);
    } else {
        // 与Application.begin_close/StartupResources相同的同步seal→异步shutdown顺序。
        fixture.target.seal_root();
    }
    assert!(
        matches!(fixture.failed.wait().await,Err(error)if error.code()=="resource_task_interrupted")
    );
    fixture.blocked.notified().await;
    assert_eq!(lock(&fixture.target.inner.state).raw_attempts.len(), 1);
    let mut retry = Box::pin(fixture.target.shutdown_root(if cancel_first {
        deadline + Duration::from_secs(30)
    } else {
        deadline
    }));
    assert!(futures_util::poll!(retry.as_mut()).is_pending());
    if !cancel_first {
        fixture.root_entered.notified().await;
    }
    drop(wake);
    fixture.root.shutdown(deadline).await?;
    assert!(futures_util::poll!(retry.as_mut()).is_pending());
    tokio::time::sleep_until(deadline).await;
    // 重试不可续期到+30秒；即使首项已失败也必须先尝试另一阻塞析构至共同期限。
    let result = futures_util::poll!(retry.as_mut());
    assert!(
        matches!(result,std::task::Poll::Ready(Err(error))if error.code()=="resource_task_interrupted")
    );
    drop(retry);
    drop(release);
    for raw in fixture.raw {
        assert!(raw.await.is_err());
    }
    assert!(
        matches!(fixture.target.shutdown_root(deadline+Duration::from_secs(30)).await,Err(error)if error.code()=="resource_task_interrupted")
    );
    {
        let state = lock(&fixture.target.inner.state);
        assert!(state.raw_attempts.is_empty());
        assert!(
            state
                .root_raw_close
                .as_ref()
                .is_some_and(|close| close.pending.is_empty() && close.interrupted)
        );
    }
    assert!(
        matches!(fixture.owner.shutdown(deadline+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_seal_and_cancelled_shutdown_keep_removed_failures_and_common_deadline() -> Result<()>
{
    split_close_retains_evidence(false).await?;
    split_close_retains_evidence(true).await
}

struct Signal(Arc<Notify>);
impl Drop for Signal {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
struct BrokenPayload {
    attempts: Arc<AtomicUsize>,
    registrar: ResourceRegistrar,
    outside: Arc<AtomicBool>,
}
impl Drop for BrokenPayload {
    fn drop(&mut self) {
        self.attempts.fetch_add(1, Ordering::AcqRel);
        if !self.registrar.test_locks_available() {
            self.outside.store(false, Ordering::Release);
        }
        std::panic::resume_unwind(Box::new("PRIVATE-OLD-PAYLOAD"));
    }
}
fn fail(payload: impl Any + Send + 'static) {
    std::panic::resume_unwind(Box::new(payload));
}
#[tokio::test(flavor = "current_thread")]
async fn two_independent_payload_drops_keep_capacity_and_cleanup_evidence() -> Result<()> {
    let owner = Arc::new(ResourceOwner::new());
    let mut target = ContainerBuilder::new();
    target.resources = Some(owner.registrar());
    target.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = target.build();
    let resolver = target.resolve::<Resolver>().await?;
    owner.registrar().spawn(|cancel| async move {
        cancel.cancelled().await;
    })?;
    let attempts = Arc::new(AtomicUsize::new(0));
    let outside = Arc::new(AtomicBool::new(true));
    let mut signals = Vec::new();
    for _ in 0..2 {
        let done = Arc::new(Notify::new());
        let signal = Signal(Arc::clone(&done));
        let payload = BrokenPayload {
            attempts: Arc::clone(&attempts),
            registrar: owner.registrar(),
            outside: Arc::clone(&outside),
        };
        owner.registrar().test_legacy_spawn(move |_| async move {
            let _signal = signal;
            fail(payload);
        })?;
        signals.push(done);
    }
    // 两份任务先一批登记再让出current_thread，避免第二次spawn提前回收第一份。
    for done in signals {
        done.notified().await;
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |_| {
        count.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Ok(Arc::new(1)) })
    });
    let source = source.build();
    let handle = factory(&source, &ServiceKey::typed::<usize>())?;
    let mut raw = handle.invoke_internal::<usize>((*resolver).clone(), ParameterOverrides::new());
    let polled = crate::foundation::lifecycle::with_test_task_limit(1, || {
        let mut context = std::task::Context::from_waker(futures_util::task::noop_waker_ref());
        raw.as_mut().poll(&mut context)
    });
    let std::task::Poll::Ready(Err(error)) = polled else {
        return Err(container_error("test_rejection_missing", "原工厂应被拒绝"));
    };
    assert_eq!(error.code(), "resource_task_interrupted");
    let evidence = std::error::Error::source(&error)
        .and_then(|source| source.downcast_ref::<ResourceTaskRejection>())
        .ok_or_else(|| container_error("test_evidence_missing", "缺少双错误来源"))?;
    assert_eq!(evidence.registration.code(), "background_task_capacity");
    assert_eq!(evidence.cleanup.code(), "resource_task_interrupted");
    assert!(!format!("{evidence:?} {evidence}").contains("PRIVATE-OLD-PAYLOAD"));
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(outside.load(Ordering::Acquire));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
