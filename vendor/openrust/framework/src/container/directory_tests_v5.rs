//! v4独审：panic payload完整析构必须先于完成通知。
use super::parameters::{ParameterKey, ParameterOverrides, ParameterValue};
use super::*;
use std::sync::Condvar;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
fn factory(container: &Container) -> Result<super::directory::FactoryHandle> {
    container
        .bindings_snapshot()?
        .into_iter()
        .next()
        .map(|entry| entry.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))
}
async fn target() -> Result<(Container, Arc<ResourceOwner>, Resolver)> {
    let owner = Arc::new(ResourceOwner::new());
    let mut builder = ContainerBuilder::new();
    builder.resources = Some(owner.registrar());
    builder.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let container = builder.build();
    let resolver = (*container.resolve::<Resolver>().await?).clone();
    Ok((container, owner, resolver))
}
struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Release {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.0;
        *lock(mutex) = true;
        changed.notify_all();
    }
}
struct BlockingPayload {
    entered: Arc<Notify>,
    gate: Arc<(Mutex<bool>, Condvar)>,
}
impl Drop for BlockingPayload {
    fn drop(&mut self) {
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
fn fail(payload: impl Any + Send) -> ! {
    std::panic::resume_unwind(Box::new(payload))
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn poll_panic_payload_drop_precedes_raw_waiter_and_root_completion() -> Result<()> {
    let (target, owner, resolver) = target().await?;
    let entered = Arc::new(Notify::new());
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(Arc::clone(&gate));
    let wait = Arc::clone(&entered);
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |_| {
        let payload = BlockingPayload {
            entered: Arc::clone(&wait),
            gate: Arc::clone(&gate),
        };
        Box::pin(async move { fail(payload) })
    });
    let source = source.build();
    let mut raw = factory(&source)?.invoke_internal::<usize>(resolver, ParameterOverrides::new());
    assert!(futures_util::poll!(raw.as_mut()).is_pending());
    entered.notified().await;
    let fence = lock(&target.inner.state)
        .raw_attempts
        .values()
        .next()
        .map(|record| record.completion.clone())
        .ok_or_else(|| container_error("test_fence_missing", "payload仍在析构时不能删除raw"))?;
    assert!(fence.finished_interrupted().is_none());
    assert!(futures_util::poll!(raw.as_mut()).is_pending());
    let mut shutdown =
        Box::pin(target.shutdown_root(tokio::time::Instant::now() + Duration::from_secs(3)));
    assert!(futures_util::poll!(shutdown.as_mut()).is_pending());
    drop(release);
    assert!(matches!(raw.await,Err(error)if error.code()=="resource_task_interrupted"));
    assert!(matches!(shutdown.await,Err(error)if error.code()=="resource_task_interrupted"));
    assert_eq!(fence.finished_interrupted(), Some(true));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
struct NextPayload(Arc<AtomicUsize>);
impl Drop for NextPayload {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        fail("PRIVATE-SECOND-UNWIND");
    }
}
struct CancelCapture(Arc<AtomicUsize>);
impl Drop for CancelCapture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        fail(NextPayload(Arc::clone(&self.0)));
    }
}
#[tokio::test]
async fn cancelled_future_finite_capture_and_payload_unwinds_still_publish_fence() -> Result<()> {
    let (target, owner, resolver) = target().await?;
    let attempts = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let entered = Arc::clone(&started);
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |resolver| {
        let entered = Arc::clone(&entered);
        Box::pin(async move {
            let _resolver = resolver;
            entered.notify_one();
            std::future::pending::<Result<Arc<usize>>>().await
        })
    });
    let source = source.build();
    let mut params = ParameterOverrides::new();
    params.insert(
        ParameterKey::name("private"),
        ParameterValue::new(CancelCapture(Arc::clone(&attempts))),
    );
    let mut raw = factory(&source)?.invoke_internal::<usize>(resolver, params);
    assert!(futures_util::poll!(raw.as_mut()).is_pending());
    started.notified().await;
    target.seal_root();
    let result = tokio::time::timeout(Duration::from_secs(3), raw)
        .await
        .map_err(|_| container_error("test_fence_timeout", "连续有限unwind不能遗失completion"))?;
    assert!(matches!(result,Err(error)if error.code()=="resource_task_interrupted"));
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(target.shutdown_root(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_task_interrupted")
    );
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
