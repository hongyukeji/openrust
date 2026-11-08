//! 普通公开回调与私有擦除入口的真实处置责任。
use super::super::directory::FactoryHandle;
use super::super::directory_tests_v10::signal_send;
use super::super::{ParameterOverrides, ResourceOwner, parameters};
use super::{
    Container, ContainerBuilder, Error, Resolver, Result, ServiceKey, Value, container_error,
    erase_extender, lock,
};
use crate::BoxFuture;
use crate::foundation::lifecycle::{TaskExecutionFailure, catch_isolated};
use crate::foundation::resources::ResourceTaskCompletion;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::Instant;

const LIMIT: Duration = Duration::from_secs(3);

fn test_error(message: &str) -> Error {
    container_error("callback_disposal_fixture", message)
}

fn owner_builder() -> (Arc<ResourceOwner>, ContainerBuilder) {
    let owner = Arc::new(ResourceOwner::new());
    let mut builder = ContainerBuilder::new();
    builder.resources = Some(owner.registrar());
    (owner, builder)
}

fn factory_source<T, F>(factory: F) -> Result<(Container, FactoryHandle)>
where
    T: Send + Sync + 'static,
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
        .ok_or_else(|| test_error("缺少原工厂"))?;
    Ok((source, handle))
}

struct Release(mpsc::SyncSender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

struct PayloadGate {
    entered: mpsc::SyncSender<()>,
    release: mpsc::Receiver<()>,
}
struct Payload {
    cleared: Arc<AtomicUsize>,
    gate: Option<PayloadGate>,
}
impl Drop for Payload {
    fn drop(&mut self) {
        if let Some(gate) = self.gate.take() {
            let _ = gate.entered.send(());
            let _ = gate.release.recv_timeout(LIMIT);
        }
        self.cleared.fetch_add(1, Ordering::AcqRel);
    }
}
struct Bad {
    dropped: Arc<AtomicUsize>,
    payload: Mutex<Option<Payload>>,
}
impl Drop for Bad {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::AcqRel);
        let payload = self
            .payload
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(payload) = payload {
            std::panic::resume_unwind(Box::new(payload));
        }
    }
}
#[derive(Default)]
struct Counts {
    dropped: Arc<AtomicUsize>,
    payload: Arc<AtomicUsize>,
}
impl Counts {
    fn params(&self, gate: Option<PayloadGate>) -> ParameterOverrides {
        let mut input = ParameterOverrides::new();
        input.insert(
            parameters::ParameterKey::name("bad"),
            parameters::ParameterValue::new(Bad {
                dropped: Arc::clone(&self.dropped),
                payload: Mutex::new(Some(Payload {
                    cleared: Arc::clone(&self.payload),
                    gate,
                })),
            }),
        );
        input
    }
    fn assert_cleared(&self) {
        assert_eq!(self.dropped.load(Ordering::Acquire), 1);
        assert_eq!(self.payload.load(Ordering::Acquire), 1);
    }
}

struct Original {
    owner: Arc<ResourceOwner>,
    completion: ResourceTaskCompletion,
}
impl Original {
    async fn assert_unchanged(&self) {
        assert_eq!(self.completion.finished_interrupted(), Some(false));
        assert_closed(&self.owner, 0).await;
    }
}
async fn original(counts: &Counts, gate: Option<PayloadGate>) -> Result<(Original, Resolver)> {
    let (owner, builder) = owner_builder();
    let target = builder.build();
    let resolver = target.resolver(u64::MAX, Arc::default(), owner.registrar());
    let (_source, handle) =
        factory_source::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }))?;
    let result = handle
        .invoke_internal::<Resolver>(resolver, counts.params(gate))
        .await?;
    let resolver = (*result).clone();
    drop(result);
    let completion = resolver
        .factory_completion
        .clone()
        .ok_or_else(|| test_error("缺少已完成原票据"))?;
    assert_eq!(completion.finished_interrupted(), Some(false));
    assert_eq!(counts.dropped.load(Ordering::Acquire), 0);
    Ok((Original { owner, completion }, resolver))
}

async fn assert_closed(owner: &ResourceOwner, failures: usize) {
    for _ in 0..2 {
        let result = owner.shutdown(Instant::now() + LIMIT).await;
        if failures == 0 {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(error) if error.code() == "resource_shutdown_failed"));
        }
        assert_eq!(owner.registrar().test_outcome_failures(), Some(failures));
    }
}

fn has_code(error: &Error, code: &str) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if error
            .downcast_ref::<Error>()
            .or_else(|| error.downcast_ref::<Arc<Error>>().map(Arc::as_ref))
            .is_some_and(|error| error.code() == code)
        {
            return true;
        }
        source = error.source();
    }
    false
}
fn has_pair(error: &Error, primary: &str) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(error) = source {
        if error
            .downcast_ref::<TaskExecutionFailure>()
            .is_some_and(|failure| {
                failure.primary.code() == primary
                    && failure.cleanup.code() == "resource_task_interrupted"
            })
        {
            return true;
        }
        source = error.source();
    }
    false
}
fn failure<T>(result: Result<T>) -> Result<Error> {
    result
        .err()
        .ok_or_else(|| test_error("清理异常时不应交付成功"))
}

/// poll 只移交结果。捕获在 Ready 或 Pending 之后仍由真实 Future 持有。
struct Retained<T, H> {
    result: Option<Result<T>>,
    _held: H,
    polls: Option<Arc<AtomicUsize>>,
}
impl<T: Unpin, H: Unpin> Future for Retained<T, H> {
    type Output = Result<T>;
    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(polls) = &this.polls {
            polls.fetch_add(1, Ordering::AcqRel);
        }
        this.result.take().map_or(Poll::Pending, Poll::Ready)
    }
}
fn retained<T: Send + Unpin + 'static, H: Send + Unpin + 'static>(
    input: &Mutex<Option<H>>,
    result: Option<Result<T>>,
    polls: Option<Arc<AtomicUsize>>,
) -> BoxFuture<'static, Result<T>> {
    match lock(input).take() {
        Some(held) => Box::pin(Retained {
            result,
            _held: held,
            polls,
        }),
        None => Box::pin(async { Err(test_error("捕获应只移交一次")) }),
    }
}

#[tokio::test]
async fn ordinary_resolve_ready_retirement_keeps_owner_pending_and_never_caches_success()
-> Result<()> {
    let counts = Counts::default();
    let (entered, seen) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::sync_channel(1);
    let release = Release(release);
    let (old, resolver) = original(
        &counts,
        Some(PayloadGate {
            entered,
            release: wait,
        }),
    )
    .await?;
    let (owner, mut builder) = owner_builder();
    let resolver = Mutex::new(Some(resolver));
    builder.singleton::<usize, _>(move |_| retained(&resolver, Some(Ok(Arc::new(7))), None));
    let container = builder.build();
    let caller = container.clone();
    let runtime = tokio::runtime::Handle::current();
    let actor = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        let mut call = Box::pin(caller.resolve::<usize>());
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        call.as_mut().poll(&mut cx)
    });
    seen.recv_timeout(LIMIT).map_err(Error::internal)?;
    assert_eq!(counts.dropped.load(Ordering::Acquire), 1);
    assert_eq!(counts.payload.load(Ordering::Acquire), 0);
    assert_eq!(old.completion.finished_interrupted(), Some(false));
    assert!(container.inner.state.try_lock().is_ok());
    assert!(owner.registrar().test_locks_available());
    assert!(container.get_ready::<usize>().is_none());
    let observed = Arc::new(Notify::new());
    let notify = Arc::clone(&observed);
    owner
        .registrar()
        .test_after_observed_snapshot(move || notify.notify_one());
    let mut closing = Box::pin(owner.shutdown(Instant::now() + LIMIT));
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    tokio::time::timeout(LIMIT, observed.notified())
        .await
        .map_err(Error::internal)?;
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    assert_eq!(owner.registrar().test_outcome_failures(), None);
    assert!(owner.registrar().test_locks_available());
    drop(release);
    let polled = actor
        .join()
        .map_err(|_| test_error("有限析构越出普通 resolve"))?;
    let Poll::Ready(result) = polled else {
        return Err(test_error("普通 resolve 未同步返回诊断"));
    };
    let error = failure(result)?;
    assert_eq!(error.code(), "service_initialization_failed");
    assert!(has_code(&error, "resource_task_interrupted"));
    assert!(container.get_ready::<usize>().is_none());
    counts.assert_cleared();
    assert!(matches!(closing.await, Err(error) if error.code() == "resource_shutdown_failed"));
    assert_closed(&owner, 1).await;
    old.assert_unchanged().await;
    Ok(())
}

#[tokio::test]
async fn ordinary_extender_rejects_unpublished_replacement_and_listener_retains_committed_value()
-> Result<()> {
    let extender_counts = Counts::default();
    let (old, resolver) = original(&extender_counts, None).await?;
    let (owner, mut builder) = owner_builder();
    builder.instance(Arc::new(7usize));
    let container = builder.build();
    let before = container.resolve::<usize>().await?;
    let resolver = Mutex::new(Some(resolver));
    let error = failure(
        container
            .extend::<usize, _>(move |value, _| {
                retained(&resolver, Some(Ok(Arc::new(*value + 1))), None)
            })
            .await,
    )?;
    assert_eq!(error.code(), "service_initialization_failed");
    assert!(has_code(&error, "resource_task_interrupted"));
    assert!(Arc::ptr_eq(&before, &container.resolve::<usize>().await?));
    extender_counts.assert_cleared();
    assert_closed(&owner, 1).await;
    old.assert_unchanged().await;

    let listener_counts = Counts::default();
    let (old, resolver) = original(&listener_counts, None).await?;
    let (owner, mut builder) = owner_builder();
    builder.instance(Arc::new(9usize));
    let resolver = Mutex::new(Some(resolver));
    builder.rebinding::<usize, _>(move |_, _| {
        retained(
            &resolver,
            Some(Err(Error::custom("listener_body", "监听器原错误"))),
            None,
        )
    })?;
    let container = builder.build();
    assert_eq!(*container.resolve::<usize>().await?, 9);
    let committed = Arc::new(11usize);
    let error = failure(container.instance(Arc::clone(&committed)).await)?;
    assert_eq!(error.code(), "resource_task_interrupted");
    assert!(has_pair(&error, "listener_body"));
    assert!(Arc::ptr_eq(
        &committed,
        &container.resolve::<usize>().await?
    ));
    listener_counts.assert_cleared();
    assert_closed(&owner, 1).await;
    old.assert_unchanged().await;
    Ok(())
}

#[tokio::test]
async fn pending_cancel_and_unpolled_callback_capture_use_real_owners() -> Result<()> {
    let pending_counts = Counts::default();
    let (old, resolver) = original(&pending_counts, None).await?;
    let (owner, mut builder) = owner_builder();
    let resolver = Mutex::new(Some(resolver));
    let polls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&polls);
    builder.singleton::<usize, _>(move |_| retained(&resolver, None, Some(Arc::clone(&count))));
    let container = builder.build();
    let mut call = Box::pin(container.resolve::<usize>());
    assert!(futures_util::poll!(call.as_mut()).is_pending());
    assert_eq!(polls.load(Ordering::Acquire), 1);
    assert_eq!(pending_counts.dropped.load(Ordering::Acquire), 0);
    assert!(catch_isolated(|| drop(call)).is_ok());
    assert!(container.get_ready::<usize>().is_none());
    pending_counts.assert_cleared();
    assert_closed(&owner, 1).await;
    old.assert_unchanged().await;

    let unpolled_counts = Counts::default();
    let (old, captured) = original(&unpolled_counts, None).await?;
    let (owner, builder) = owner_builder();
    let target = builder.build();
    let resolver = target.resolver(u64::MAX, Arc::default(), owner.registrar());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let (source, handle) = factory_source::<usize, _>(move |_| {
        std::hint::black_box(&captured);
        observed.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Ok(Arc::new(1)) })
    })?;
    let factory = lock(&source.inner.state)
        .bindings
        .get(&ServiceKey::typed::<usize>())
        .and_then(|binding| binding.factory.clone())
        .ok_or_else(|| test_error("缺少真实擦除工厂"))?;
    let call = factory(resolver);
    drop(handle);
    drop(factory);
    drop(source);
    assert_eq!(unpolled_counts.dropped.load(Ordering::Acquire), 0);
    assert!(catch_isolated(|| drop(call)).is_ok());
    assert_eq!(calls.load(Ordering::Acquire), 0);
    unpolled_counts.assert_cleared();
    assert_closed(&owner, 1).await;
    old.assert_unchanged().await;
    Ok(())
}

struct HeldResolver {
    _resolver: Resolver,
}

#[tokio::test]
async fn early_erased_input_rejection_preserves_primary_and_cleans_input() -> Result<()> {
    let counts = Counts::default();
    let (old, resolver) = original(&counts, None).await?;
    let (owner, builder) = owner_builder();
    let target = builder.build();
    let current = target.resolver(u64::MAX, Arc::default(), owner.registrar());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let erased = erase_extender::<usize, _>(move |value, _| {
        observed.fetch_add(1, Ordering::AcqRel);
        Box::pin(async move { Ok(value) })
    });
    let input: Value = Arc::new(HeldResolver {
        _resolver: resolver,
    });
    let error = failure((erased.0)(input, current).await)?;
    assert_eq!(error.code(), "resource_task_interrupted");
    assert!(has_pair(&error, "service_type_mismatch"));
    assert_eq!(calls.load(Ordering::Acquire), 0);
    counts.assert_cleared();
    assert_closed(&owner, 1).await;
    old.assert_unchanged().await;
    Ok(())
}

#[tokio::test]
async fn ordinary_clear_reports_local_failure_but_original_live_completion_gets_only_charge()
-> Result<()> {
    let counts = Counts::default();
    let (origin, builder) = owner_builder();
    let target = builder.build();
    let initial_resolver = target.resolver(u64::MAX, Arc::default(), origin.registrar());
    let (capture_tx, capture_rx) = tokio::sync::oneshot::channel();
    let capture = Mutex::new(Some(capture_tx));
    let release = Arc::new(Notify::new());
    let (_source, handle) = factory_source::<usize, _>(move |resolver| {
        if let Some(sender) = lock(&capture).take() {
            let _ = sender.send(resolver);
        }
        Box::pin(async { Ok(Arc::new(5)) })
    })?;
    let sent = signal_send(Some(Arc::clone(&release)));
    let mut original_call = handle.invoke_internal::<usize>(initial_resolver, counts.params(None));
    assert!(futures_util::poll!(original_call.as_mut()).is_pending());
    let resolver = tokio::time::timeout(LIMIT, capture_rx)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    let completion = resolver
        .factory_completion
        .clone()
        .ok_or_else(|| test_error("缺少活跃原票据"))?;
    // 只在原 producer 已释放它的 Resolver、但 fence 尚未发布时交给普通 B。
    tokio::time::timeout(LIMIT, sent)
        .await
        .map_err(Error::internal)?
        .map_err(Error::internal)?;
    assert_eq!(completion.finished_interrupted(), None);
    let (current, mut builder) = owner_builder();
    let resolver = Mutex::new(Some(resolver));
    builder.singleton::<usize, _>(move |_| retained(&resolver, Some(Ok(Arc::new(7))), None));
    let container = builder.build();
    let error = failure(container.resolve::<usize>().await)?;
    assert!(has_code(&error, "resource_task_interrupted"));
    assert!(container.get_ready::<usize>().is_none());
    counts.assert_cleared();
    assert_eq!(completion.finished_interrupted(), None);
    assert_closed(&current, 0).await;
    release.notify_one();
    let error = failure(
        tokio::time::timeout(LIMIT, original_call)
            .await
            .map_err(Error::internal)?,
    )?;
    assert!(has_code(&error, "resource_task_interrupted"));
    assert_eq!(completion.finished_interrupted(), Some(true));
    assert_closed(&origin, 1).await;
    Ok(())
}

#[tokio::test]
async fn taken_owner_pending_callback_inherits_enclosing_ordinary_cleanup() -> Result<()> {
    let counts = Counts::default();
    let (old, resolver) = original(&counts, None).await?;
    let (inner_owner, mut builder) = owner_builder();
    let resolver = Mutex::new(Some(resolver));
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&polls);
    builder.singleton::<usize, _>(move |_| retained(&resolver, None, Some(Arc::clone(&observed))));
    let inner = builder.build();
    let mut pending: BoxFuture<'static, Result<Arc<usize>>> =
        Box::pin(async move { inner.resolve::<usize>().await });
    assert!(futures_util::poll!(pending.as_mut()).is_pending());
    assert_eq!(polls.load(Ordering::Acquire), 1);
    // Pending 未持有短票据。B 的真实关闭能在捕获仍由 B Future 持有时接管并完成。
    assert_closed(&inner_owner, 0).await;
    assert!(inner_owner.registrar().begin_cleanup().is_none());
    assert_eq!(counts.dropped.load(Ordering::Acquire), 0);

    let (outer_owner, mut builder) = owner_builder();
    let pending = Mutex::new(Some(pending));
    builder.singleton::<usize, _>(move |_| retained(&pending, Some(Ok(Arc::new(7))), None));
    let outer = builder.build();
    let error = failure(outer.resolve::<usize>().await)?;
    assert!(has_code(&error, "resource_task_interrupted"));
    assert!(outer.get_ready::<usize>().is_none());
    counts.assert_cleared();
    assert_closed(&outer_owner, 1).await;
    assert_closed(&inner_owner, 0).await;
    old.assert_unchanged().await;
    Ok(())
}

#[tokio::test]
async fn rejected_ok_value_is_retired_in_same_cleanup_before_publication() -> Result<()> {
    let future_counts = Counts::default();
    let output_counts = Counts::default();
    let (old_future, future) = original(&future_counts, None).await?;
    let (old_output, output) = original(&output_counts, None).await?;
    let (owner, mut builder) = owner_builder();
    let future = Mutex::new(Some(future));
    let output = Mutex::new(Some(output));
    builder.singleton::<HeldResolver, _>(move |_| {
        let value = lock(&output)
            .take()
            .map(|resolver| {
                Arc::new(HeldResolver {
                    _resolver: resolver,
                })
            })
            .ok_or_else(|| test_error("缺少待拒绝的成功值"));
        retained(&future, Some(value), None)
    });
    let container = builder.build();
    let error = failure(container.resolve::<HeldResolver>().await)?;
    assert!(has_code(&error, "resource_task_interrupted"));
    assert!(container.get_ready::<HeldResolver>().is_none());
    future_counts.assert_cleared();
    output_counts.assert_cleared();
    assert_closed(&owner, 1).await;
    old_future.assert_unchanged().await;
    old_output.assert_unchanged().await;
    Ok(())
}

#[tokio::test]
async fn successful_ordinary_result_transfers_external_late_drop_without_rewriting_history()
-> Result<()> {
    let counts = Counts::default();
    let (old, resolver) = original(&counts, None).await?;
    let (owner, mut builder) = owner_builder();
    let resolver = Mutex::new(Some(resolver));
    builder.singleton::<HeldResolver, _>(move |_| {
        let value = lock(&resolver)
            .take()
            .map(|resolver| {
                Arc::new(HeldResolver {
                    _resolver: resolver,
                })
            })
            .ok_or_else(|| test_error("缺少正常交付的成功值"));
        Box::pin(async move { value })
    });
    let container = builder.build();
    let value = container.resolve::<HeldResolver>().await?;
    let cached = container
        .get_ready::<HeldResolver>()
        .ok_or_else(|| test_error("正常值应已发布"))?;
    assert!(Arc::ptr_eq(&value, &cached));
    assert_eq!(counts.dropped.load(Ordering::Acquire), 0);
    assert_closed(&owner, 0).await;
    old.assert_unchanged().await;
    drop(cached);
    drop(container);
    assert_eq!(counts.dropped.load(Ordering::Acquire), 0);
    drop(value);
    counts.assert_cleared();
    assert_closed(&owner, 0).await;
    old.assert_unchanged().await;
    Ok(())
}
