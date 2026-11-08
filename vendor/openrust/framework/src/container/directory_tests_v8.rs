//! v8：交付前的Result/最后factory归框架，owner可主动退役且不追溯改历史。
use super::directory::FactoryHandle;
use super::*;
use crate::foundation::resources::{ResourceRegistrar, ResourceTaskCompletion};
use std::cell::RefCell;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
type SendHook = Box<dyn FnOnce(ResourceTaskCompletion) -> BoxFuture<'static, ()> + Send>;
std::thread_local! {
    static SEND_HOOK:RefCell<Option<SendHook>>=const {RefCell::new(None)};
    static REGISTER_HOOK:RefCell<Option<Box<dyn FnOnce()+Send>>>=const {RefCell::new(None)};
    static RESERVATION_HOOK:RefCell<Option<Box<dyn FnOnce()+Send>>>=const {RefCell::new(None)};
    static NOTIFIED:RefCell<Option<Arc<AtomicUsize>>>=const {RefCell::new(None)};
}
pub(super) async fn after_send(completion: ResourceTaskCompletion) {
    let hook = SEND_HOOK.take();
    if let Some(hook) = hook {
        hook(completion).await;
    }
}
pub(super) fn after_reclaim_registration() {
    if let Some(hook) = REGISTER_HOOK.take() {
        hook();
    }
}
pub(super) fn after_reservation() {
    if let Some(hook) = RESERVATION_HOOK.take() {
        hook();
    }
}
pub(super) fn after_notification() {
    NOTIFIED.with_borrow(|slot| {
        if let Some(count) = slot {
            count.fetch_add(1, Ordering::AcqRel);
        }
    });
}
async fn target() -> Result<(Arc<ResourceOwner>, Container, Resolver)> {
    let owner = Arc::new(ResourceOwner::new());
    let mut builder = ContainerBuilder::new();
    builder.resources = Some(owner.registrar());
    builder.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let container = builder.build();
    let resolver = container.resolve::<Resolver>().await?;
    Ok((owner, container, (*resolver).clone()))
}
fn source<T, F>(factory: F) -> Result<(Container, FactoryHandle)>
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static,
{
    let mut builder = ContainerBuilder::new();
    builder.bind::<T, _>(factory);
    let source = builder.build();
    let handle = source
        .bindings_snapshot()?
        .into_iter()
        .next()
        .map(|row| row.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))?;
    Ok((source, handle))
}
struct Bad {
    count: Arc<AtomicUsize>,
    locks: Arc<AtomicBool>,
    owner: ResourceRegistrar,
    target: Container,
}
impl Drop for Bad {
    fn drop(&mut self) {
        let _ = self.target.contains::<usize>();
        self.locks
            .store(self.owner.test_locks_available(), Ordering::Release);
        self.count.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("独立有限交付析构异常"));
    }
}
struct SourceBad {
    _value: Bad,
}
impl std::fmt::Debug for SourceBad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("private source")
    }
}
impl std::fmt::Display for SourceBad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PRIVATE-DELIVERY-SOURCE")
    }
}
impl std::error::Error for SourceBad {}
fn bad(
    count: &Arc<AtomicUsize>,
    locks: &Arc<AtomicBool>,
    owner: &Arc<ResourceOwner>,
    target: &Container,
) -> Bad {
    Bad {
        count: Arc::clone(count),
        locks: Arc::clone(locks),
        owner: owner.registrar(),
        target: target.clone(),
    }
}
fn body_error(value: Bad) -> Error {
    Error::http_with_source(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "ct8_body_error",
        "PRIVATE-DELIVERY-MESSAGE",
        SourceBad { _value: value },
    )
}
fn has_code(error: &Error, code: &str) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    for _ in 0..20 {
        let Some(error) = current else {
            return false;
        };
        if error
            .downcast_ref::<Error>()
            .is_some_and(|error| error.code() == code)
        {
            return true;
        }
        current = error.source();
    }
    false
}
fn isolated(name: &str, marker: &str, body: impl FnOnce() -> Result<()>) -> Result<()> {
    if std::env::var_os(marker).is_some() {
        body()?;
        std::io::Write::write_all(
            &mut std::io::stdout().lock(),
            format!("{marker}:completed\n").as_bytes(),
        )
        .map_err(Error::internal)?;
        return Ok(());
    }
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, module)| module);
    let output = std::process::Command::new(std::env::current_exe().map_err(Error::internal)?)
        .args(["--exact", &format!("{module}::{name}"), "--nocapture"])
        .env(marker, "1")
        .output()
        .map_err(Error::internal)?;
    assert!(
        output.status.success(),
        "受控交付子进程必须正常结束：{}",
        output.status
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("{marker}:completed")));
    Ok(())
}
fn runtime(body: impl Future<Output = Result<()>>) -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::internal)?
        .block_on(body)
}
struct Release(Arc<Notify>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}
async fn sent_result_case(mode: usize, error_result: bool) -> Result<()> {
    let (owner, target, resolver) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let locks = Arc::new(AtomicBool::new(false));
    let counter = Arc::clone(&count);
    let unlocked = Arc::clone(&locks);
    let observed_owner = Arc::clone(&owner);
    let observed_target = target.clone();
    let (_source, handle) = source::<Bad, _>(move |_| {
        let value = bad(&counter, &unlocked, &observed_owner, &observed_target);
        Box::pin(async move {
            if error_result {
                Err(body_error(value))
            } else {
                Ok(Arc::new(value))
            }
        })
    })?;
    let (sent, seen) = tokio::sync::oneshot::channel();
    let release = Arc::new(Notify::new());
    let resume = Arc::clone(&release);
    SEND_HOOK.set(Some(Box::new(move |completion| {
        Box::pin(async move {
            let _ = sent.send(completion);
            if mode == 1 {
                resume.notified().await;
            }
        })
    })));
    let _release = Release(Arc::clone(&release));
    let notified = Arc::new(AtomicUsize::new(0));
    NOTIFIED.set(Some(Arc::clone(&notified)));
    let mut call = handle.invoke_internal::<Bad>(resolver, parameters::ParameterOverrides::new());
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let completion = tokio::time::timeout(Duration::from_secs(3), seen)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    assert_eq!(count.load(Ordering::Acquire), 0);
    if mode == 1 {
        // 已从通知槽取出，但用户Result仍归frame；caller正在等producer fence。
        assert!(futures_util::poll!(call.as_mut()).is_pending());
        assert_eq!(notified.load(Ordering::Acquire), 1);
    } else {
        completion.wait().await?;
        assert_eq!(notified.load(Ordering::Acquire), 0);
    }
    if mode == 2 {
        // 不再poll、也不先Drop caller；关闭本身必须主动回收并按实际owner报告未知。
        assert!(
            matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
        );
        assert_eq!(count.load(Ordering::Acquire), 1);
    }
    assert!(crate::foundation::lifecycle::catch_isolated(|| drop(call)).is_ok());
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(locks.load(Ordering::Acquire));
    release.notify_one();
    completion.wait().await?;
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    NOTIFIED.take();
    Ok(())
}
#[test]
fn sent_and_received_pending_results_are_isolated_and_owner_can_reclaim() -> Result<()> {
    isolated(
        "sent_and_received_pending_results_are_isolated_and_owner_can_reclaim",
        "OPENRUST_CT8_RAW_DELIVERY_CHILD",
        || {
            runtime(async {
                for error_result in [false, true] {
                    for mode in 0..3 {
                        sent_result_case(mode, error_result).await?;
                    }
                }
                Ok(())
            })
        },
    )
}
async fn execution_postcheck_case(error_result: bool) -> Result<()> {
    let (owner, target, resolver) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let locks = Arc::new(AtomicBool::new(false));
    let cancel = crate::core::Cancellation::new();
    let signal = cancel.clone();
    let counter = Arc::clone(&count);
    let unlocked = Arc::clone(&locks);
    let observer = Arc::clone(&owner);
    let observed_target = target.clone();
    let (_source, handle) = source::<Bad, _>(move |_| {
        let value = bad(&counter, &unlocked, &observer, &observed_target);
        let signal = signal.clone();
        Box::pin(async move {
            signal.cancel();
            if error_result {
                Err(body_error(value))
            } else {
                Ok(Arc::new(value))
            }
        })
    })?;
    let context =
        crate::core::ExecutionContext::new(crate::core::ExecutionMetadata::default(), None, cancel);
    let result = crate::core::with_execution_context(
        context,
        handle.invoke_internal::<Bad>(resolver, parameters::ParameterOverrides::new()),
    )
    .await;
    let error = result.err().ok_or_else(|| {
        container_error("test_execution_primary_missing", "执行已取消不能交付成功")
    })?;
    assert_eq!(error.code(), "resource_task_interrupted");
    assert!(has_code(&error, "execution_cancelled"));
    assert!(!format!("{error:?} {error}").contains("PRIVATE-DELIVERY"));
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(locks.load(Ordering::Acquire));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
#[test]
fn execution_cancelled_in_ready_poll_keeps_primary_and_cleanup_unknown() -> Result<()> {
    isolated(
        "execution_cancelled_in_ready_poll_keeps_primary_and_cleanup_unknown",
        "OPENRUST_CT8_POSTCHECK_CHILD",
        || {
            runtime(async {
                execution_postcheck_case(false).await?;
                execution_postcheck_case(true).await
            })
        },
    )
}
async fn last_factory_case(mode: usize) -> Result<()> {
    let (owner, target, resolver) = target().await?;
    if mode >= 3 {
        owner
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
            .await?;
    }
    let count = Arc::new(AtomicUsize::new(0));
    let locks = Arc::new(AtomicBool::new(false));
    let captured = bad(&count, &locks, &owner, &target);
    let (source, handle) = source::<usize, _>(move |_| {
        let _ = &captured;
        Box::pin(async { Ok(Arc::new(7)) })
    })?;
    let call = handle.invoke_internal::<usize>(resolver, parameters::ParameterOverrides::new());
    drop(source);
    drop(handle);
    assert_eq!(count.load(Ordering::Acquire), 0);
    if mode == 1 {
        owner.seal();
        assert_eq!(count.load(Ordering::Acquire), 1);
    }
    if matches!(mode, 1 | 2 | 4) {
        let error = call.await.err().ok_or_else(|| {
            container_error("test_factory_unknown_missing", "最后工厂捕获未知不能成功")
        })?;
        assert_eq!(error.code(), "resource_task_interrupted");
        if mode != 2 {
            assert!(has_code(&error, "resources_closed"));
        }
    } else {
        assert!(crate::foundation::lifecycle::catch_isolated(|| drop(call)).is_ok());
    }
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(locks.load(Ordering::Acquire));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    let closed = owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await;
    if mode >= 3 {
        // 在创建前已发布Ok的新拒绝调用，只提供本次未知，不篡改或晚计数旧历史。
        assert!(closed.is_ok());
    } else {
        assert!(matches!(closed,Err(error)if error.code()=="resource_shutdown_failed"));
    }
    Ok(())
}
#[test]
fn last_internal_factory_arc_is_guarded_unpolled_rejected_completed_and_late() -> Result<()> {
    isolated(
        "last_internal_factory_arc_is_guarded_unpolled_rejected_completed_and_late",
        "OPENRUST_CT8_LAST_FACTORY_CHILD",
        || {
            runtime(async {
                for mode in 0..5 {
                    last_factory_case(mode).await?;
                }
                Ok(())
            })
        },
    )
}
#[tokio::test]
async fn successful_final_delivery_preserves_same_external_arc_and_clean_close() -> Result<()> {
    let (owner, _target, resolver) = target().await?;
    let value = Arc::new(19usize);
    let original = Arc::clone(&value);
    let (_source, handle) = source::<usize, _>(move |_| {
        let value = Arc::clone(&value);
        Box::pin(async move { Ok(value) })
    })?;
    let result = handle
        .invoke_internal::<usize>(resolver, parameters::ParameterOverrides::new())
        .await?;
    assert!(Arc::ptr_eq(&result, &original));
    owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    assert_eq!(*result, 19);
    drop(result);
    owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    Ok(())
}

struct ThreadRelease(std::sync::mpsc::SyncSender<()>);
impl Drop for ThreadRelease {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
#[tokio::test]
async fn reserved_producer_responsibility_survives_owner_take_before_spawn_rejection() -> Result<()>
{
    let (owner, target, resolver) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let locks = Arc::new(AtomicBool::new(false));
    let captured = bad(&count, &locks, &owner, &target);
    let (source, handle) = source::<usize, _>(move |_| {
        let _ = &captured;
        Box::pin(async { Ok(Arc::new(1)) })
    })?;
    let mut call = handle.invoke_internal::<usize>(resolver, parameters::ParameterOverrides::new());
    drop(source);
    drop(handle);
    let (claimed_tx, claimed_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let release = ThreadRelease(release_tx);
    let runtime = tokio::runtime::Handle::current();
    let thread = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        RESERVATION_HOOK.set(Some(Box::new(move || {
            let _ = claimed_tx.send(());
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        })));
        let mut context = std::task::Context::from_waker(futures_util::task::noop_waker_ref());
        call.as_mut().poll(&mut context)
    });
    claimed_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    // inputs已移到producer guard，但实际spawn还没有登记；close仍须等同一completion。
    assert_eq!(count.load(Ordering::Acquire), 0);
    let snapshot = Arc::new(Notify::new());
    let seen = Arc::clone(&snapshot);
    owner
        .registrar()
        .test_after_observed_snapshot(move || seen.notify_one());
    let mut closing =
        Box::pin(owner.shutdown(tokio::time::Instant::now() + Duration::from_secs(3)));
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    tokio::time::timeout(Duration::from_secs(3), snapshot.notified())
        .await
        .map_err(Error::internal)?;
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    assert_eq!(count.load(Ordering::Acquire), 0);
    drop(release);
    let polled = thread
        .join()
        .map_err(|_| container_error("test_thread_panicked", "有限捕获清理不能越出原工厂调用"))?;
    let std::task::Poll::Ready(Err(error)) = polled else {
        return Err(container_error(
            "test_rejection_missing",
            "已接管owner必须在本poll完成拒绝",
        ));
    };
    assert_eq!(error.code(), "resource_task_interrupted");
    assert!(
        has_code(&error, "resources_closed")
            || std::error::Error::source(&error)
                .and_then(|source| source
                    .downcast_ref::<crate::foundation::resources::ResourceTaskRejection>())
                .is_some_and(|source| source.registration.code() == "resources_closed")
    );
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(locks.load(Ordering::Acquire));
    assert!(matches!(closing.await,Err(error)if error.code()=="resource_shutdown_failed"));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    Ok(())
}

struct ParameterGate {
    entered: std::sync::mpsc::SyncSender<()>,
    release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    count: Arc<AtomicUsize>,
}
impl Drop for ParameterGate {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let receiver = lock(&self.release).take();
        if let Some(receiver) = receiver {
            let _ = receiver.recv_timeout(Duration::from_secs(3));
        }
        self.count.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("有限未准入参数析构异常"));
    }
}
#[tokio::test]
async fn reclaim_before_constructor_installs_ticket_waits_for_actual_parameter_retirement()
-> Result<()> {
    let (owner, target, resolver) = target().await?;
    let (_source, handle) = source::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }))?;
    let count = Arc::new(AtomicUsize::new(0));
    let (registered_tx, registered_rx) = std::sync::mpsc::sync_channel(1);
    let (install_tx, install_rx) = std::sync::mpsc::sync_channel(1);
    let install = ThreadRelease(install_tx);
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let release = ThreadRelease(release_tx);
    let mut parameters = parameters::ParameterOverrides::new();
    parameters.insert(
        parameters::ParameterKey::Index(0),
        parameters::ParameterValue::new(ParameterGate {
            entered: entered_tx,
            release: Mutex::new(Some(release_rx)),
            count: Arc::clone(&count),
        }),
    );
    let constructor = std::thread::spawn(move || {
        REGISTER_HOOK.set(Some(Box::new(move || {
            let _ = registered_tx.send(());
            let _ = install_rx.recv_timeout(Duration::from_secs(3));
        })));
        handle.invoke_internal::<usize>(resolver, parameters)
    });
    registered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    let actor = Arc::clone(&owner);
    let sealing = std::thread::spawn(move || actor.seal());
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    assert_eq!(count.load(Ordering::Acquire), 0);
    drop(install);
    let call = constructor
        .join()
        .map_err(|_| container_error("test_constructor_panicked", "构造future不得析构外逃"))?;
    let snapshot = Arc::new(Notify::new());
    let seen = Arc::clone(&snapshot);
    owner
        .registrar()
        .test_after_observed_snapshot(move || seen.notify_one());
    let mut closing =
        Box::pin(owner.shutdown(tokio::time::Instant::now() + Duration::from_secs(3)));
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    tokio::time::timeout(Duration::from_secs(3), snapshot.notified())
        .await
        .map_err(Error::internal)?;
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    assert_eq!(count.load(Ordering::Acquire), 0);
    drop(release);
    sealing
        .join()
        .map_err(|_| container_error("test_seal_panicked", "有限参数析构不得越出seal"))?;
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(matches!(closing.await,Err(error)if error.code()=="resource_shutdown_failed"));
    assert!(crate::foundation::lifecycle::catch_isolated(|| drop(call)).is_ok());
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    Ok(())
}
