//! v9：正式登记握手、活跃caller退役等待与嵌套raw输入实际清除。
use super::directory::FactoryHandle;
use super::*;
use crate::foundation::resources::{ResourceTaskCompletion, ResourceTaskRejection};
use std::cell::RefCell;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
type NestedCall = BoxFuture<'static, Result<Arc<usize>>>;
type ReservationHook = Box<dyn FnOnce(ResourceTaskCompletion) + Send>;
std::thread_local! {static RESERVE:RefCell<Option<ReservationHook>>=const{RefCell::new(None)};}
pub(super) fn after_reservation(completion: ResourceTaskCompletion) {
    if let Some(action) = RESERVE.take() {
        action(completion);
    }
}
async fn target() -> Result<(Arc<ResourceOwner>, Container, Resolver)> {
    let owner = Arc::new(ResourceOwner::new());
    let mut builder = ContainerBuilder::new();
    builder.resources = Some(owner.registrar());
    builder.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = builder.build();
    let resolver = target.resolve::<Resolver>().await?;
    Ok((owner, target, (*resolver).clone()))
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
        .map(|item| item.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))?;
    Ok((source, handle))
}
struct Release(std::sync::mpsc::SyncSender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
struct Bad {
    panic_on_drop: bool,
    entered: Option<std::sync::mpsc::SyncSender<()>>,
    release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    count: Arc<AtomicUsize>,
    locks: Arc<AtomicBool>,
    owner: ResourceRegistrar,
    target: Container,
}
impl Drop for Bad {
    fn drop(&mut self) {
        if let Some(entered) = &self.entered {
            let _ = entered.send(());
        }
        let receiver = lock(&self.release).take();
        if let Some(receiver) = receiver {
            let _ = receiver.recv_timeout(Duration::from_secs(3));
        }
        let _ = self.target.contains::<usize>();
        self.locks
            .store(self.owner.test_locks_available(), Ordering::Release);
        self.count.fetch_add(1, Ordering::AcqRel);
        if self.panic_on_drop {
            std::panic::resume_unwind(Box::new("有限独立旧输入或工厂析构异常"));
        }
    }
}
fn immediate(
    count: &Arc<AtomicUsize>,
    locks: &Arc<AtomicBool>,
    owner: &Arc<ResourceOwner>,
    target: &Container,
) -> Bad {
    Bad {
        panic_on_drop: true,
        entered: None,
        release: Mutex::new(None),
        count: Arc::clone(count),
        locks: Arc::clone(locks),
        owner: owner.registrar(),
        target: target.clone(),
    }
}
fn gated(
    count: &Arc<AtomicUsize>,
    locks: &Arc<AtomicBool>,
    owner: &Arc<ResourceOwner>,
    target: &Container,
) -> (Bad, std::sync::mpsc::Receiver<()>, Release) {
    let (entered, seen) = std::sync::mpsc::sync_channel(1);
    let (release, waiting) = std::sync::mpsc::sync_channel(1);
    (
        Bad {
            panic_on_drop: true,
            entered: Some(entered),
            release: Mutex::new(Some(waiting)),
            count: Arc::clone(count),
            locks: Arc::clone(locks),
            owner: owner.registrar(),
            target: target.clone(),
        },
        seen,
        Release(release),
    )
}
async fn snapshot(
    owner: &Arc<ResourceOwner>,
    closing: &mut std::pin::Pin<Box<impl Future<Output = Result<()>>>>,
    seen: &Notify,
) -> Result<()> {
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    tokio::time::timeout(Duration::from_secs(3), seen.notified())
        .await
        .map_err(Error::internal)?;
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    // owner参数保真实owner生命期，不以模拟事件替代正式shutdown观察。
    assert!(owner.registrar().test_locks_available());
    Ok(())
}
#[tokio::test]
async fn rejected_handoff_stays_pending_during_last_factory_drop_then_reports_both() -> Result<()> {
    let (owner, target, resolver) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let locks = Arc::new(AtomicBool::new(false));
    let (captured, drop_entered, drop_release) = gated(&count, &locks, &owner, &target);
    let (source, handle) = source::<usize, _>(move |_| {
        let _ = &captured;
        Box::pin(async { Ok(Arc::new(1)) })
    })?;
    let mut call = handle.invoke_internal::<usize>(resolver, parameters::ParameterOverrides::new());
    drop(source);
    drop(handle);
    let (reserved, reservation) = std::sync::mpsc::sync_channel(1);
    let (admit, wait_admit) = std::sync::mpsc::sync_channel(1);
    let admit = Release(admit);
    let runtime = tokio::runtime::Handle::current();
    let actor = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        RESERVE.set(Some(Box::new(move |completion| {
            let _ = reserved.send(completion);
            let _ = wait_admit.recv_timeout(Duration::from_secs(3));
        })));
        let mut context = std::task::Context::from_waker(futures_util::task::noop_waker_ref());
        call.as_mut().poll(&mut context)
    });
    let completion = reservation
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    assert_eq!(completion.finished_interrupted(), None);
    let seen = Arc::new(Notify::new());
    let notify = Arc::clone(&seen);
    owner
        .registrar()
        .test_after_observed_snapshot(move || notify.notify_one());
    let mut closing =
        Box::pin(owner.shutdown(tokio::time::Instant::now() + Duration::from_secs(3)));
    snapshot(&owner, &mut closing, &seen).await?;
    drop(admit);
    drop_entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    // 真正F拒绝析构已进入，registered仍不得完成，raw索引也不能提前移除。
    assert_eq!(count.load(Ordering::Acquire), 0);
    assert_eq!(completion.finished_interrupted(), None);
    assert_eq!(lock(&target.inner.state).raw_attempts.len(), 1);
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    assert!(!actor.is_finished());
    drop(drop_release);
    let result = actor
        .join()
        .map_err(|_| container_error("test_actor_panicked", "有限factory析构不得越出原调用"))?;
    let std::task::Poll::Ready(Err(error)) = result else {
        return Err(container_error(
            "test_rejection_missing",
            "拒绝清理后应完成本次调用",
        ));
    };
    assert_eq!(error.code(), "resource_task_interrupted");
    let evidence = std::error::Error::source(&error)
        .and_then(|source| source.downcast_ref::<ResourceTaskRejection>())
        .ok_or_else(|| container_error("test_dual_source_missing", "登记与清理必须独立保留"))?;
    assert_eq!(evidence.registration.code(), "resources_closed");
    assert_eq!(evidence.cleanup.code(), "resource_task_interrupted");
    assert_eq!(completion.finished_interrupted(), Some(true));
    assert!(
        matches!(completion.wait().await,Err(error)if error.code()=="resource_task_interrupted")
    );
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(locks.load(Ordering::Acquire));
    assert!(matches!(closing.await,Err(error)if error.code()=="resource_shutdown_failed"));
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    Ok(())
}
async fn caller_retirement_case(cancel: bool) -> Result<()> {
    let (owner, target, resolver) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let locks = Arc::new(AtomicBool::new(false));
    let (value, entered, release) = gated(&count, &locks, &owner, &target);
    let (_source, handle) = source::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }))?;
    let mut parameters = parameters::ParameterOverrides::new();
    parameters.insert(
        parameters::ParameterKey::Index(0),
        parameters::ParameterValue::new(value),
    );
    let mut call = handle.invoke_internal::<usize>(resolver, parameters);
    let actor = Arc::clone(&owner);
    let sealing = std::thread::spawn(move || actor.seal());
    entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    assert_eq!(count.load(Ordering::Acquire), 0);
    // 不只观察owner：本次活跃caller真正poll到同一次退役等待中的Pending。
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let seen = Arc::new(Notify::new());
    let notify = Arc::clone(&seen);
    owner
        .registrar()
        .test_after_observed_snapshot(move || notify.notify_one());
    let mut closing =
        Box::pin(owner.shutdown(tokio::time::Instant::now() + Duration::from_secs(3)));
    snapshot(&owner, &mut closing, &seen).await?;
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    if cancel {
        let (sent, done) = std::sync::mpsc::sync_channel(1);
        let dropped = std::thread::spawn(move || {
            let result = crate::foundation::lifecycle::catch_isolated(|| drop(call));
            let _ = sent.send(result.is_ok());
        });
        assert!(
            done.recv_timeout(Duration::from_secs(3))
                .map_err(Error::internal)?
        );
        dropped
            .join()
            .map_err(|_| container_error("test_drop_panicked", "caller Drop不可同步等owner"))?;
        assert_eq!(count.load(Ordering::Acquire), 0);
        drop(release);
    } else {
        drop(release);
        let error = call
            .await
            .err()
            .ok_or_else(|| container_error("test_primary_missing", "认领中的清理不能提前成功"))?;
        assert_eq!(error.code(), "resource_task_interrupted");
        let source = std::error::Error::source(&error)
            .and_then(std::error::Error::source)
            .and_then(|source| source.downcast_ref::<Error>())
            .ok_or_else(|| container_error("test_primary_missing", "缺少原拒绝主错"))?;
        assert_eq!(source.code(), "resources_closed");
    }
    sealing
        .join()
        .map_err(|_| container_error("test_seal_panicked", "有限参数Drop不能越出seal"))?;
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(locks.load(Ordering::Acquire));
    assert!(matches!(closing.await,Err(error)if error.code()=="resource_shutdown_failed"));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    Ok(())
}
#[tokio::test]
async fn active_caller_waits_retirement_and_cancellation_does_not_synchronously_wait() -> Result<()>
{
    caller_retirement_case(false).await?;
    caller_retirement_case(true).await
}
#[tokio::test]
async fn nested_unpolled_future_does_not_own_previous_raw_parameters_or_completion() -> Result<()> {
    let (owner, target, resolver) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let locks = Arc::new(AtomicBool::new(false));
    let value = immediate(&count, &locks, &owner, &target);
    let (_b_source, b) = source::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))?;
    let slot: Arc<Mutex<Option<NestedCall>>> = Arc::new(Mutex::new(None));
    let stored = Arc::clone(&slot);
    let (_a_source, a) = source::<usize, _>(move |resolver| {
        let b = b.clone();
        let stored = Arc::clone(&stored);
        Box::pin(async move {
            let nested =
                b.invoke_internal::<usize>(resolver, parameters::ParameterOverrides::new());
            *lock(&stored) = Some(nested);
            Ok(Arc::new(1))
        })
    })?;
    let mut parameters = parameters::ParameterOverrides::new();
    parameters.insert(
        parameters::ParameterKey::Index(0),
        parameters::ParameterValue::new(value),
    );
    let mut call = a.invoke_internal::<usize>(resolver, parameters);
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let completion = lock(&target.inner.state)
        .raw_attempts
        .values()
        .next()
        .map(|record| record.completion.clone())
        .ok_or_else(|| container_error("test_attempt_missing", "缺少实际A调用"))?;
    assert!(matches!(call.await,Err(error)if error.code()=="resource_task_interrupted"));
    assert_eq!(completion.finished_interrupted(), Some(true));
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(locks.load(Ordering::Acquire));
    let nested = lock(&slot)
        .take()
        .ok_or_else(|| container_error("test_nested_missing", "A未构造B"))?;
    // 只保B future；从不poll，测试方没有任何A参数/Resolver/结果的强引用。
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(crate::foundation::lifecycle::catch_isolated(|| drop(nested)).is_ok());
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), Some(true));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    Ok(())
}
async fn previously_delivered_resolver_case(mode: usize) -> Result<()> {
    let (owner, target, resolver) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let locks = Arc::new(AtomicBool::new(false));
    let value = immediate(&count, &locks, &owner, &target);
    let (_a_source, a) =
        source::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }))?;
    let mut parameters = parameters::ParameterOverrides::new();
    parameters.insert(
        parameters::ParameterKey::Index(0),
        parameters::ParameterValue::new(value),
    );
    let mut call = a.invoke_internal::<Resolver>(resolver, parameters);
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let completion = lock(&target.inner.state)
        .raw_attempts
        .values()
        .next()
        .map(|record| record.completion.clone())
        .ok_or_else(|| container_error("test_attempt_missing", "缺少实际A调用"))?;
    let delivered = call.await?;
    assert_eq!(completion.finished_interrupted(), Some(false));
    assert_eq!(count.load(Ordering::Acquire), 0);
    if mode == 1 {
        owner.seal();
    } else if mode == 2 {
        owner
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
            .await?;
    }
    let resolver = Arc::try_unwrap(delivered)
        .map_err(|_| container_error("test_external_clone", "应只保一份已交付Resolver"))?;
    let (_b_source, b) = source::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))?;
    let call = b.invoke_internal::<usize>(resolver, parameters::ParameterOverrides::new());
    // 构造本身已实际清完旧输入，当前future里不藏旧Arc，旧A完成结果保持历史。
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(locks.load(Ordering::Acquire));
    assert_eq!(completion.finished_interrupted(), Some(false));
    assert!(matches!(call.await,Err(error)if error.code()=="resource_task_interrupted"));
    assert_eq!(completion.finished_interrupted(), Some(false));
    let result = owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await;
    if mode == 2 {
        assert!(result.is_ok());
    } else {
        assert!(matches!(result,Err(error)if error.code()=="resource_shutdown_failed"));
    }
    assert_eq!(count.load(Ordering::Acquire), 1);
    Ok(())
}
#[tokio::test]
async fn consumed_old_resolver_uses_current_cleanup_without_rewriting_completed_history()
-> Result<()> {
    for mode in 0..3 {
        previously_delivered_resolver_case(mode).await?;
    }
    Ok(())
}

#[tokio::test]
async fn constructor_old_reference_cleanup_cannot_hide_new_inputs_from_owner_reclaim() -> Result<()>
{
    let (owner, target, resolver) = target().await?;
    let old_count = Arc::new(AtomicUsize::new(0));
    let old_locks = Arc::new(AtomicBool::new(false));
    let (mut old, entered, release) = gated(&old_count, &old_locks, &owner, &target);
    old.panic_on_drop = false;
    let (_a_source, a) =
        source::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }))?;
    let mut parameters = parameters::ParameterOverrides::new();
    parameters.insert(
        parameters::ParameterKey::Index(0),
        parameters::ParameterValue::new(old),
    );
    let mut call = a.invoke_internal::<Resolver>(resolver, parameters);
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let completion = lock(&target.inner.state)
        .raw_attempts
        .values()
        .next()
        .map(|record| record.completion.clone())
        .ok_or_else(|| container_error("test_attempt_missing", "缺少实际A调用"))?;
    let resolver = Arc::try_unwrap(call.await?)
        .map_err(|_| container_error("test_external_clone", "仅有已移交的一份Resolver"))?;
    assert_eq!(completion.finished_interrupted(), Some(false));
    let new_count = Arc::new(AtomicUsize::new(0));
    let new_locks = Arc::new(AtomicBool::new(false));
    let value = immediate(&new_count, &new_locks, &owner, &target);
    let (_b_source, b) = source::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))?;
    let mut parameters = parameters::ParameterOverrides::new();
    parameters.insert(
        parameters::ParameterKey::Index(0),
        parameters::ParameterValue::new(value),
    );
    let constructor = std::thread::spawn(move || b.invoke_internal::<usize>(resolver, parameters));
    entered
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    assert_eq!(old_count.load(Ordering::Acquire), 0);
    assert_eq!(new_count.load(Ordering::Acquire), 0);
    let seen = Arc::new(Notify::new());
    let notify = Arc::clone(&seen);
    owner
        .registrar()
        .test_after_observed_snapshot(move || notify.notify_one());
    let mut closing =
        Box::pin(owner.shutdown(tokio::time::Instant::now() + Duration::from_secs(3)));
    snapshot(&owner, &mut closing, &seen).await?;
    // 新参数已真实回收，旧参数清理仍在构造栈；同frame的startup责任阻止提前Ok。
    assert_eq!(new_count.load(Ordering::Acquire), 1);
    assert!(new_locks.load(Ordering::Acquire));
    assert_eq!(old_count.load(Ordering::Acquire), 0);
    assert_eq!(completion.finished_interrupted(), Some(false));
    drop(release);
    let nested = constructor
        .join()
        .map_err(|_| container_error("test_constructor_panicked", "旧输入清理不能越出构造调用"))?;
    assert_eq!(old_count.load(Ordering::Acquire), 1);
    assert!(old_locks.load(Ordering::Acquire));
    assert!(matches!(closing.await,Err(error)if error.code()=="resource_shutdown_failed"));
    assert!(crate::foundation::lifecycle::catch_isolated(|| drop(nested)).is_ok());
    assert_eq!(new_count.load(Ordering::Acquire), 1);
    assert_eq!(completion.finished_interrupted(), Some(false));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    Ok(())
}
