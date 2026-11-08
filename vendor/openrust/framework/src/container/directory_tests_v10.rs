//! v10：真末析构单次归责、泛型交付及同步重入责任，不以Err总数冒充ledger。
use super::directory::FactoryHandle;
use super::*;
use crate::foundation::lifecycle::disposal::{self, Credential};
use crate::foundation::resources::ResourceTaskCompletion;
use std::cell::RefCell;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

type SendHook = Box<dyn FnOnce(ResourceTaskCompletion) -> BoxFuture<'static, ()> + Send>;
std::thread_local! {
    static AFTER_RELEASE: RefCell<Option<Box<dyn FnOnce()+Send>>> = const {RefCell::new(None)};
    static SENT: RefCell<Option<SendHook>> = const {RefCell::new(None)};
}
pub(super) fn after_previous_release() {
    if let Some(hook) = AFTER_RELEASE.take() {
        hook();
    }
}
pub(super) async fn after_send(completion: ResourceTaskCompletion) {
    if let Some(hook) = SENT.take() {
        hook(completion).await;
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
        .map(|row| row.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))?;
    Ok((source, handle))
}
struct Bad {
    count: Arc<AtomicUsize>,
}
impl Drop for Bad {
    fn drop(&mut self) {
        self.count.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("有限参数Drop"));
    }
}
fn parameters(count: &Arc<AtomicUsize>) -> ParameterOverrides {
    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        parameters::ParameterKey::name("bad"),
        parameters::ParameterValue::new(Bad {
            count: Arc::clone(count),
        }),
    );
    parameters
}
pub(super) fn signal_send(
    block: Option<Arc<Notify>>,
) -> tokio::sync::oneshot::Receiver<ResourceTaskCompletion> {
    let (sent, seen) = tokio::sync::oneshot::channel();
    SENT.set(Some(Box::new(move |completion| {
        Box::pin(async move {
            let _ = sent.send(completion);
            if let Some(block) = block {
                block.notified().await;
            }
        })
    })));
    seen
}
async fn completion(
    seen: tokio::sync::oneshot::Receiver<ResourceTaskCompletion>,
) -> Result<ResourceTaskCompletion> {
    tokio::time::timeout(Duration::from_secs(3), seen)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)
}
async fn original(
    resolver: Resolver,
    count: &Arc<AtomicUsize>,
) -> Result<(Arc<Resolver>, ResourceTaskCompletion)> {
    let (_source, handle) =
        source::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }))?;
    let seen = signal_send(None);
    let value = handle
        .invoke_internal::<Resolver>(resolver, parameters(count))
        .await?;
    let completion = completion(seen).await?;
    assert_eq!(completion.finished_interrupted(), Some(false));
    assert_eq!(count.load(Ordering::Acquire), 0);
    Ok((value, completion))
}
async fn close(owner: &Arc<ResourceOwner>, expected: usize) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let result = owner.shutdown(deadline).await;
    if expected == 0 {
        assert!(result.is_ok());
    } else {
        assert!(matches!(result,Err(error) if error.code()=="resource_shutdown_failed"));
    }
    assert_eq!(owner.registrar().test_outcome_failures(), Some(expected));
    let repeated = owner.shutdown(deadline).await;
    if expected == 0 {
        assert!(repeated.is_ok());
    } else {
        assert!(matches!(repeated,Err(error) if error.code()=="resource_shutdown_failed"));
    }
    assert_eq!(owner.registrar().test_outcome_failures(), Some(expected));
    Ok(())
}
fn has_code(error: &Error, code: &str) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if error
            .downcast_ref::<Error>()
            .is_some_and(|error| error.code() == code)
        {
            return true;
        }
        source = error.source();
    }
    false
}
/// 私有fixture只迁移真实A参数及completion；B的container/scope/owner来自真实B解析。
fn rehome(mut old: Resolver, mut current: Resolver) -> Resolver {
    current.factory_parameters = std::mem::replace(
        &mut old.factory_parameters,
        Arc::new(FactoryParameters::empty()),
    );
    current.factory_completion = old.factory_completion.take();
    current
}
struct Release(std::sync::mpsc::SyncSender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[tokio::test]
async fn nonlast_release_cannot_read_other_calls_failure() -> Result<()> {
    let (owner, target, seed) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let (saved, original_completion) = original(seed, &count).await?;
    let b = (*saved).clone();
    let c = (*saved).clone();
    drop(saved);
    let (_source, handle) = source::<usize, _>(|_| Box::pin(async { Ok(Arc::new(7)) }))?;
    let b_handle = handle.clone();
    let (entered, reached_release) = std::sync::mpsc::sync_channel(1);
    let (resume, wait) = std::sync::mpsc::sync_channel(1);
    let release = Release(resume);
    let runtime = tokio::runtime::Handle::current();
    let actor = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        AFTER_RELEASE.set(Some(Box::new(move || {
            let _ = entered.send(());
            let _ = wait.recv_timeout(Duration::from_secs(3));
        })));
        b_handle.invoke_internal::<usize>(b, ParameterOverrides::new())
    });
    reached_release
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    assert_eq!(count.load(Ordering::Acquire), 0);
    let c_call = handle.invoke_internal::<usize>(c, ParameterOverrides::new());
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert_eq!(original_completion.finished_interrupted(), Some(false));
    drop(release);
    let b_call = actor
        .join()
        .map_err(|_| container_error("test_actor_panicked", "非末释放不得panic"))?;
    assert_eq!(*b_call.await?, 7);
    assert!(matches!(c_call.await,Err(error) if has_code(&error,"resource_task_interrupted")));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert_eq!(count.load(Ordering::Acquire), 1);
    close(&owner, 1).await?;
    assert_eq!(original_completion.finished_interrupted(), Some(false));
    Ok(())
}

struct Wrapper {
    _resolver: Resolver,
}
fn direct(resolver: Resolver) -> Arc<Resolver> {
    Arc::new(resolver)
}
fn wrapped(resolver: Resolver) -> Arc<Wrapper> {
    Arc::new(Wrapper {
        _resolver: resolver,
    })
}
async fn generic_result<T: Send + Sync + 'static>(
    build: fn(Resolver) -> Arc<T>,
    mode: usize,
) -> Result<()> {
    let (owner, target, seed) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let (_source, handle) = source::<T, _>(move |resolver| {
        let value = build(resolver);
        Box::pin(async move { Ok(value) })
    })?;
    let release = Arc::new(Notify::new());
    let producer_signal = signal_send((mode == 2).then(|| Arc::clone(&release)));
    let mut call = handle.invoke_internal::<T>(seed, parameters(&count));
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let completed = completion(producer_signal).await?;
    if mode == 2 {
        assert_eq!(completed.finished_interrupted(), None);
        // caller已消费unit通知，确实Pending等同一producer fence。
        assert!(futures_util::poll!(call.as_mut()).is_pending());
    } else {
        completed.wait().await?;
        assert_eq!(completed.finished_interrupted(), Some(false));
    }
    assert_eq!(count.load(Ordering::Acquire), 0);
    if mode == 1 {
        // caller不再poll且仍活着，owner自己从原索引认领结果。
        close(&owner, 1).await?;
        assert_eq!(count.load(Ordering::Acquire), 1);
        drop(call);
    } else if mode == 3 {
        let value = call.await?;
        assert_eq!(count.load(Ordering::Acquire), 0);
        close(&owner, 0).await?;
        drop(value);
        assert_eq!(count.load(Ordering::Acquire), 1);
        close(&owner, 0).await?;
    } else {
        assert!(crate::foundation::lifecycle::catch_isolated(|| drop(call)).is_ok());
        assert_eq!(count.load(Ordering::Acquire), 1);
        release.notify_one();
        if mode == 2 {
            assert!(
                matches!(completed.wait().await, Err(error) if error.code() == "resource_task_interrupted")
            );
        } else {
            completed.wait().await?;
        }
        close(&owner, 1).await?;
    }
    assert_eq!(completed.finished_interrupted(), Some(mode == 2));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    Ok(())
}
#[tokio::test]
async fn generic_undelivered_and_external_delivered_values_have_distinct_owners() -> Result<()> {
    for mode in 0..4 {
        generic_result(direct, mode).await?;
        // 嵌套共享所有权必须沿两层析构传递清理票据。
        generic_result(|resolver| Arc::new(Arc::new(resolver)), mode).await?;
        generic_result(wrapped, mode).await?;
    }
    Ok(())
}

#[tokio::test]
async fn ordinary_drop_and_into_inner_report_only_actual_destructor() -> Result<()> {
    for inner_wins in [false, true] {
        let (owner, _target, seed) = target().await?;
        let count = Arc::new(AtomicUsize::new(0));
        let (saved, original_completion) = original(seed, &count).await?;
        let Resolver {
            factory_parameters: params,
            ..
        } = (*saved).clone();
        drop(saved);
        let other = Arc::clone(&params);
        let mut cleanup = owner
            .registrar()
            .begin_cleanup()
            .ok_or_else(|| container_error("test_cleanup_missing", "缺少真实清理票据"))?;
        let credential = Credential::Cleanup(cleanup.disposal_ticket());
        let ((), first) = disposal::scope(Some(credential.clone()), || {
            if inner_wins {
                drop(other);
            } else {
                assert!(Arc::into_inner(other).is_none());
            }
        });
        assert!(!first.seen);
        assert!(!first.owned);
        assert_eq!(count.load(Ordering::Acquire), 0);
        let (result, last) = disposal::scope(Some(credential), || {
            if inner_wins {
                let last = Arc::into_inner(params)
                    .ok_or_else(|| container_error("test_last_missing", "真实最后Arc应可取回"))?;
                drop(last);
            } else {
                drop(params);
            }
            Ok::<(), Error>(())
        });
        result?;
        assert!(last.seen);
        assert!(last.owned);
        assert_eq!(count.load(Ordering::Acquire), 1);
        cleanup.finish(0);
        close(&owner, 1).await?;
        assert_eq!(original_completion.finished_interrupted(), Some(false));
    }
    Ok(())
}

#[tokio::test]
async fn live_original_priority_and_claimed_original_fallback_charge_exact_owner() -> Result<()> {
    for original_live in [false, true] {
        let (a_owner, _a_target, a_seed) = target().await?;
        let (b_owner, _b_target, b_seed) = target().await?;
        let count = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Notify::new());
        let (old, original_completion, a_call) = if original_live {
            let (sent, seen) = tokio::sync::oneshot::channel();
            let sender = Mutex::new(Some(sent));
            let (_source, handle) = source::<usize, _>(move |resolver| {
                if let Some(sender) = lock(&sender).take() {
                    let _ = sender.send(resolver);
                }
                Box::pin(async { Ok(Arc::new(5)) })
            })?;
            let sent_completion = signal_send(Some(Arc::clone(&release)));
            let mut call = handle.invoke_internal::<usize>(a_seed, parameters(&count));
            assert!(futures_util::poll!(call.as_mut()).is_pending());
            let old = tokio::time::timeout(Duration::from_secs(3), seen)
                .await
                .map_err(Error::internal)?
                .map_err(Error::internal)?;
            let original = old
                .factory_completion
                .clone()
                .ok_or_else(|| container_error("test_completion_missing", "真实A必须携完成票据"))?;
            // RawInvocation 已返回并释放它的 Resolver，producer 仍在真实完成门控。
            completion(sent_completion).await?;
            (old, original, Some(call))
        } else {
            let (old, original) = original(a_seed, &count).await?;
            ((*old).clone(), original, None)
        };
        let (_source, handle) = source::<usize, _>(|_| Box::pin(async { Ok(Arc::new(9)) }))?;
        let call = handle.invoke_internal::<usize>(rehome(old, b_seed), ParameterOverrides::new());
        assert_eq!(count.load(Ordering::Acquire), 1);
        let error = call
            .await
            .err()
            .ok_or_else(|| container_error("test_expected_unknown", "本次保留必要未知诊断"))?;
        assert!(has_code(&error, "resource_task_interrupted"));
        if let Some(call) = a_call {
            assert_eq!(original_completion.finished_interrupted(), None);
            release.notify_one();
            assert!(
                matches!(call.await,Err(error) if has_code(&error,"resource_task_interrupted"))
            );
            original_completion.wait().await.err().ok_or_else(|| {
                container_error("test_original_should_fail", "真实A必须收到unknown")
            })?;
        } else {
            assert_eq!(original_completion.finished_interrupted(), Some(false));
        }
        close(&a_owner, usize::from(original_live)).await?;
        close(&b_owner, usize::from(!original_live)).await?;
        assert_eq!(count.load(Ordering::Acquire), 1);
    }
    Ok(())
}

struct Reenter {
    resolver: Option<Resolver>,
    handle: FactoryHandle,
    seen: Arc<AtomicBool>,
    primary: Arc<AtomicBool>,
}
impl Drop for Reenter {
    fn drop(&mut self) {
        if let Some(resolver) = self.resolver.take() {
            let mut call = self
                .handle
                .invoke_internal::<usize>(resolver, ParameterOverrides::new());
            let mut context = std::task::Context::from_waker(futures_util::task::noop_waker_ref());
            if let std::task::Poll::Ready(Err(error)) = call.as_mut().poll(&mut context) {
                self.seen.store(
                    has_code(&error, "resource_task_interrupted"),
                    Ordering::Release,
                );
                self.primary
                    .store(has_code(&error, "resources_closed"), Ordering::Release);
            }
        }
    }
}
#[tokio::test]
async fn diagnostic_only_closed_reentry_cannot_mask_outer_valid_delivery() -> Result<()> {
    let (origin_owner, _origin_target, origin_seed) = target().await?;
    let (b_owner, _b_target, b_seed) = target().await?;
    let (a_owner, a_target, a_seed) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let (old, original_completion) = original(origin_seed, &count).await?;
    let resolver = rehome((*old).clone(), b_seed);
    drop(old);
    close(&b_owner, 0).await?;
    let (_b_source, b_handle) = source::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }))?;
    let seen = Arc::new(AtomicBool::new(false));
    let primary = Arc::new(AtomicBool::new(false));
    let wrapped = Mutex::new(Some(Reenter {
        resolver: Some(resolver),
        handle: b_handle,
        seen: Arc::clone(&seen),
        primary: Arc::clone(&primary),
    }));
    let (_source, handle) = source::<Reenter, _>(move |_| {
        let value = lock(&wrapped)
            .take()
            .ok_or_else(|| container_error("test_reentry_missing", "重入只执行一次"));
        Box::pin(async move { value.map(Arc::new) })
    })?;
    let completion_seen = signal_send(None);
    let mut call = handle.invoke_internal::<Reenter>(a_seed, ParameterOverrides::new());
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    let completed = completion(completion_seen).await?;
    completed.wait().await?;
    assert_eq!(completed.finished_interrupted(), Some(false));
    assert_eq!(count.load(Ordering::Acquire), 0);
    close(&a_owner, 1).await?;
    assert!(seen.load(Ordering::Acquire));
    assert!(primary.load(Ordering::Acquire));
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert_eq!(original_completion.finished_interrupted(), Some(false));
    close(&origin_owner, 0).await?;
    close(&b_owner, 0).await?;
    drop(call);
    assert!(lock(&a_target.inner.state).raw_attempts.is_empty());
    Ok(())
}

struct Payload {
    _resolver: Resolver,
}
#[tokio::test]
async fn sealed_taken_ticket_survives_payload_and_nested_scope_then_restores() -> Result<()> {
    let (origin_owner, _target, seed) = target().await?;
    let (owner, _current_target, _current_seed) = target().await?;
    let count = Arc::new(AtomicUsize::new(0));
    let (saved, original_completion) = original(seed, &count).await?;
    let old = (*saved).clone();
    drop(saved);
    let mut cleanup = owner
        .registrar()
        .begin_cleanup()
        .ok_or_else(|| container_error("test_cleanup_missing", "缺少实际票据"))?;
    let ticket = Credential::Cleanup(cleanup.disposal_ticket());
    let observed = Arc::new(Notify::new());
    let notify = Arc::clone(&observed);
    owner
        .registrar()
        .test_after_observed_snapshot(move || notify.notify_one());
    let mut closing =
        Box::pin(owner.shutdown(tokio::time::Instant::now() + Duration::from_secs(3)));
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    tokio::time::timeout(Duration::from_secs(3), observed.notified())
        .await
        .map_err(Error::internal)?;
    assert!(owner.registrar().begin_cleanup().is_none());
    assert_eq!(owner.registrar().test_outcome_failures(), None);
    let (caught, report) = disposal::scope(Some(ticket), || {
        let (caught, inner) = disposal::scope(None, || {
            crate::foundation::lifecycle::catch_isolated(|| {
                std::panic::resume_unwind(Box::new(Payload { _resolver: old }));
            })
        });
        assert!(inner.seen);
        assert!(inner.owned);
        assert!(disposal::seen());
        caught
    });
    assert!(caught.is_err());
    assert!(report.seen);
    assert!(report.owned);
    assert!(!disposal::seen());
    assert_eq!(count.load(Ordering::Acquire), 1);
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    cleanup.finish(0);
    assert!(matches!(closing.await,Err(error) if error.code()=="resource_shutdown_failed"));
    close(&owner, 1).await?;
    close(&origin_owner, 0).await?;
    assert_eq!(original_completion.finished_interrupted(), Some(false));
    // helper本身unwind也先恢复前一TLS；不把guard暴露给跨await入口。
    let caught = crate::foundation::lifecycle::catch_isolated(|| {
        disposal::scope(None, || {
            std::panic::resume_unwind(Box::new("有限helper unwind"))
        });
    });
    assert!(caught.is_err());
    assert!(!disposal::seen());
    Ok(())
}
