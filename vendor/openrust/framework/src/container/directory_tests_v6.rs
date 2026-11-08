//! 实际scope聚合与独立runtime直接销毁Pending observed future。
use super::parameters::{ParameterKey, ParameterOverrides, ParameterValue};
use super::*;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
fn fail(payload: impl Any + Send) -> ! {
    std::panic::resume_unwind(Box::new(payload))
}
struct Next(Arc<AtomicUsize>);
impl Drop for Next {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        fail("PRIVATE-FINITE-PAYLOAD");
    }
}
struct Bad<const ID: usize> {
    attempts: Arc<AtomicUsize>,
    mask: Arc<AtomicUsize>,
    free: Arc<AtomicBool>,
    inner: Weak<Inner>,
    owner: ResourceRegistrar,
}
impl<const ID: usize> Drop for Bad<ID> {
    fn drop(&mut self) {
        self.mask.fetch_or(1 << ID, Ordering::AcqRel);
        self.attempts.fetch_add(1, Ordering::AcqRel);
        if self
            .inner
            .upgrade()
            .is_none_or(|inner| inner.state.try_lock().is_err())
            || !self.owner.test_locks_available()
        {
            self.free.store(false, Ordering::Release);
        }
        fail(Next(Arc::clone(&self.attempts)));
    }
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::internal)
}
async fn scope_cache_cleanup(mode: usize) -> Result<()> {
    let parent = ResourceOwner::new();
    let mut builder = ContainerBuilder::new();
    if mode == 3 {
        builder.resources = Some(parent.registrar());
    }
    let container = builder.build();
    let scope = if mode >= 2 {
        Some(container.scope()?)
    } else {
        None
    };
    let owner = scope.as_ref().map_or_else(
        || Arc::clone(&lock(&container.inner.state).root_resources),
        |scope| Arc::clone(&scope.resources),
    );
    let attempts = Arc::new(AtomicUsize::new(0));
    let mask = Arc::new(AtomicUsize::new(0));
    let free = Arc::new(AtomicBool::new(true));
    let captures = || {
        (
            Arc::clone(&attempts),
            Arc::clone(&mask),
            Arc::clone(&free),
            Arc::downgrade(&container.inner),
            owner.registrar(),
        )
    };
    let (drop_attempts, drop_mask, locks_free, cache_inner, registrar) = captures();
    container
        .scoped::<Bad<0>, _>(move |_| {
            let value = Bad::<0> {
                attempts: Arc::clone(&drop_attempts),
                mask: Arc::clone(&drop_mask),
                free: Arc::clone(&locks_free),
                inner: cache_inner.clone(),
                owner: registrar.clone(),
            };
            Box::pin(async move { Ok(Arc::new(value)) })
        })
        .await?;
    let (drop_attempts, drop_mask, locks_free, cache_inner, registrar) = captures();
    container
        .scoped::<Bad<1>, _>(move |_| {
            let value = Bad::<1> {
                attempts: Arc::clone(&drop_attempts),
                mask: Arc::clone(&drop_mask),
                free: Arc::clone(&locks_free),
                inner: cache_inner.clone(),
                owner: registrar.clone(),
            };
            Box::pin(async move { Ok(Arc::new(value)) })
        })
        .await?;
    if let Some(scope) = &scope {
        drop(scope.resolve::<Bad<0>>().await?);
        drop(scope.resolve::<Bad<1>>().await?);
    } else {
        drop(container.resolve::<Bad<0>>().await?);
        drop(container.resolve::<Bad<1>>().await?);
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let result = match mode {
        0 => {
            container.seal_root();
            assert!(
                matches!(owner.registrar().spawn(|_|async{}),Err(error)if error.code()=="resources_closed")
            );
            container.shutdown_root(deadline).await
        }
        1 => container.forget_scoped_instances(deadline).await,
        2 => {
            scope
                .as_ref()
                .ok_or_else(|| container_error("test_scope_missing", "缺少scope"))?
                .shutdown(deadline)
                .await
        }
        _ => {
            parent.seal();
            assert!(
                matches!(owner.registrar().spawn(|_|async{}),Err(error)if error.code()=="resources_closed")
            );
            parent.shutdown(deadline).await
        }
    };
    assert!(matches!(result,Err(error)if error.code()=="resource_shutdown_failed"));
    assert_eq!(attempts.load(Ordering::Acquire), 4);
    assert_eq!(mask.load(Ordering::Acquire), 3);
    assert!(free.load(Ordering::Acquire));
    assert!(
        matches!(owner.shutdown(deadline).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
#[tokio::test]
async fn seal_reset_scope_and_parent_close_isolate_each_cached_value_and_keep_owner_error()
-> Result<()> {
    for mode in 0..4 {
        scope_cache_cleanup(mode).await?;
    }
    Ok(())
}
#[tokio::test]
async fn scope_close_isolates_two_local_extender_captures_and_records_the_actual_owner()
-> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    let container = builder.build();
    let scope = container.scope()?;
    let attempts = Arc::new(AtomicUsize::new(0));
    let mask = Arc::new(AtomicUsize::new(0));
    let free = Arc::new(AtomicBool::new(true));
    let a = Bad::<0> {
        attempts: Arc::clone(&attempts),
        mask: Arc::clone(&mask),
        free: Arc::clone(&free),
        inner: Arc::downgrade(&container.inner),
        owner: scope.resources.registrar(),
    };
    let b = Bad::<1> {
        attempts: Arc::clone(&attempts),
        mask: Arc::clone(&mask),
        free: Arc::clone(&free),
        inner: Arc::downgrade(&container.inner),
        owner: scope.resources.registrar(),
    };
    scope
        .extend::<usize, _>(move |value, _| {
            std::hint::black_box(&a);
            Box::pin(async move { Ok(value) })
        })
        .await?;
    scope
        .extend::<usize, _>(move |value, _| {
            std::hint::black_box(&b);
            Box::pin(async move { Ok(value) })
        })
        .await?;
    drop(scope.resolve::<usize>().await?);
    scope.close();
    assert!(
        matches!(scope.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    assert_eq!(attempts.load(Ordering::Acquire), 4);
    assert_eq!(mask.load(Ordering::Acquire), 3);
    assert!(free.load(Ordering::Acquire));
    Ok(())
}
struct Capture(Arc<AtomicUsize>);
impl Drop for Capture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        fail(Next(Arc::clone(&self.0)));
    }
}
#[test]
fn actual_runtime_drop_of_pending_observed_future_finishes_payload_fence_and_raw_index()
-> Result<()> {
    let owner = Arc::new(ResourceOwner::new());
    let mut target = ContainerBuilder::new();
    target.resources = Some(owner.registrar());
    target.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = target.build();
    let first = runtime()?;
    let resolver = first.block_on(target.resolve::<Resolver>())?;
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
    let handle = source
        .bindings_snapshot()?
        .into_iter()
        .next()
        .map(|entry| entry.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))?;
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut params = ParameterOverrides::new();
    params.insert(
        ParameterKey::name("private"),
        ParameterValue::new(Capture(Arc::clone(&attempts))),
    );
    let mut raw = handle.invoke_internal::<usize>((*resolver).clone(), params);
    first.block_on(async {
        assert!(futures_util::poll!(raw.as_mut()).is_pending());
        started.notified().await;
    });
    let fence = lock(&target.inner.state)
        .raw_attempts
        .values()
        .next()
        .map(|record| record.completion.clone())
        .ok_or_else(|| container_error("test_raw_missing", "缺少已Pending的raw"))?;
    assert!(fence.finished_interrupted().is_none());
    // 不seal、不取消caller；直接销毁真正执行ObservedFuture的runtime A。
    drop(first);
    runtime()?.block_on(async {
        let waited=tokio::time::timeout(Duration::from_secs(3),fence.wait()).await.map_err(|_|container_error("test_fence_timeout","直接Drop后completion不能遗失"))?;
        assert!(matches!(waited,Err(error)if error.code()=="resource_task_interrupted"));
        assert!(matches!(raw.await,Err(error)if error.code()=="resource_task_interrupted"));
        assert_eq!(attempts.load(Ordering::Acquire),2);assert!(lock(&target.inner.state).raw_attempts.is_empty());
        assert!(matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed"));
        Ok(())
    })
}

struct BlockingCache {
    entered: std::sync::mpsc::SyncSender<()>,
    gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
    attempts: Arc<AtomicUsize>,
}
impl Drop for BlockingCache {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let (mutex, changed) = &*self.gate;
        let mut released = lock(mutex);
        while !*released {
            released = changed
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        self.attempts.fetch_add(1, Ordering::AcqRel);
        fail(Next(Arc::clone(&self.attempts)));
    }
}
struct ReleaseCache(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl Drop for ReleaseCache {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.0;
        *lock(mutex) = true;
        changed.notify_all();
    }
}
#[tokio::test]
async fn concurrent_scope_close_and_owner_shutdown_wait_for_actual_cache_retirement() -> Result<()>
{
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let attempts = Arc::new(AtomicUsize::new(0));
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let wait = Arc::clone(&gate);
    let count = Arc::clone(&attempts);
    let mut builder = ContainerBuilder::new();
    builder.scoped::<BlockingCache, _>(move |_| {
        let value = BlockingCache {
            entered: entered_tx.clone(),
            gate: Arc::clone(&wait),
            attempts: Arc::clone(&count),
        };
        Box::pin(async move { Ok(Arc::new(value)) })
    });
    let container = builder.build();
    let scope = Arc::new(container.scope()?);
    drop(scope.resolve::<BlockingCache>().await?);
    let resources = Arc::clone(&scope.resources);
    let release = ReleaseCache(gate);
    let close = Arc::clone(&scope);
    let runtime = tokio::runtime::Handle::current();
    let thread = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        close.close();
    });
    entered_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    let mut shutdown =
        Box::pin(resources.shutdown(tokio::time::Instant::now() + Duration::from_secs(3)));
    assert!(futures_util::poll!(shutdown.as_mut()).is_pending());
    tokio::task::yield_now().await; // 无fence版本的空owner清理已可完成，不能把首次异步pending当证据。
    assert!(futures_util::poll!(shutdown.as_mut()).is_pending());
    drop(release);
    thread
        .join()
        .map_err(|_| container_error("test_close_panicked", "有限cache清理不能unwind出close"))?;
    assert!(matches!(shutdown.await,Err(error)if error.code()=="resource_shutdown_failed"));
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(
        !lock(&container.inner.state)
            .slots
            .keys()
            .any(|key| key.scope == scope.id)
    );
    Ok(())
}
