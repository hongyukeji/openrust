//! v7正式scope/raw入口与seal认领前置门控。
use super::*;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
struct Bad<const ID: usize>(Arc<AtomicUsize>);
impl<const ID: usize> Drop for Bad<ID> {
    fn drop(&mut self) {
        self.0.fetch_or(1 << ID, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("受控cache析构异常"));
    }
}
struct Release(std::sync::mpsc::SyncSender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
#[tokio::test]
async fn claimed_parent_close_hook_is_waited_before_entering_scope_state_and_retirement()
-> Result<()> {
    let parent = Arc::new(ResourceOwner::new());
    let mut builder = ContainerBuilder::new();
    builder.resources = Some(parent.registrar());
    let dropped = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&dropped);
    builder.scoped::<Bad<0>, _>(move |_| {
        let value = Bad::<0>(Arc::clone(&count));
        Box::pin(async move { Ok(Arc::new(value)) })
    });
    let count = Arc::clone(&dropped);
    builder.scoped::<Bad<1>, _>(move |_| {
        let value = Bad::<1>(Arc::clone(&count));
        Box::pin(async move { Ok(Arc::new(value)) })
    });
    let container = builder.build();
    let scope = container.scope()?;
    drop(scope.resolve::<Bad<0>>().await?);
    drop(scope.resolve::<Bad<1>>().await?);
    let child = Arc::clone(&scope.resources);
    let (claimed_tx, claimed_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let release = Release(release_tx);
    let actor = Arc::clone(&parent);
    let runtime = tokio::runtime::Handle::current();
    let thread = std::thread::spawn(move || {
        let _runtime = runtime.enter();
        actor.registrar().test_after_seal_claim(move || {
            let _ = claimed_tx.send(());
            if release_rx.recv_timeout(Duration::from_secs(3)).is_err() {
                std::panic::resume_unwind(Box::new("认领门控未释放"));
            }
        });
        actor.seal();
    });
    claimed_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(Error::internal)?;
    // 精确停在take完成/调用前：此时没有进入close_scope，也没有开始析构。
    assert!(!scope.closed.closed.load(Ordering::Acquire));
    assert_eq!(dropped.load(Ordering::Acquire), 0);
    let snapshot = Arc::new(Notify::new());
    let seen = Arc::clone(&snapshot);
    child
        .registrar()
        .test_after_observed_snapshot(move || seen.notify_one());
    parent.seal(); // B正式重复seal，不能把A已认领的hook视为已回收。
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut closing = Box::pin(parent.shutdown(deadline));
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    snapshot.notified().await;
    let mut child_wait = Box::pin(child.shutdown(deadline));
    assert!(futures_util::poll!(child_wait.as_mut()).is_pending());
    assert!(futures_util::poll!(closing.as_mut()).is_pending());
    assert!(!scope.closed.closed.load(Ordering::Acquire));
    assert_eq!(dropped.load(Ordering::Acquire), 0);
    drop(release);
    thread
        .join()
        .map_err(|_| container_error("test_seal_panicked", "有限析构不能unwind出seal"))?;
    assert!(matches!(child_wait.await,Err(error)if error.code()=="resource_shutdown_failed"));
    assert!(matches!(closing.await,Err(error)if error.code()=="resource_shutdown_failed"));
    assert!(
        matches!(scope.shutdown(deadline).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    assert_eq!(dropped.load(Ordering::Acquire), 3);
    assert!(scope.closed.closed.load(Ordering::Acquire));
    Ok(())
}
struct Stats {
    polls: AtomicUsize,
    drops: AtomicUsize,
    first: AtomicBool,
    wrong: AtomicBool,
}
struct PollPayload(Arc<Stats>);
impl Drop for PollPayload {
    fn drop(&mut self) {
        self.0.first.store(true, Ordering::Release);
    }
}
struct Both<T> {
    stats: Arc<Stats>,
    _output: std::marker::PhantomData<fn() -> T>,
}
impl<T> Future for Both<T> {
    type Output = T;
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<T> {
        self.stats.polls.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new(PollPayload(Arc::clone(&self.stats))))
    }
}
impl<T> Drop for Both<T> {
    fn drop(&mut self) {
        self.stats.drops.fetch_add(1, Ordering::AcqRel);
        if !self.stats.first.load(Ordering::Acquire) {
            self.stats.wrong.store(true, Ordering::Release);
        }
        std::panic::resume_unwind(Box::new("独立future Drop异常"));
    }
}
fn stats() -> Arc<Stats> {
    Arc::new(Stats {
        polls: AtomicUsize::new(0),
        drops: AtomicUsize::new(0),
        first: AtomicBool::new(false),
        wrong: AtomicBool::new(false),
    })
}
fn check(stats: &Stats) {
    assert_eq!(stats.polls.load(Ordering::Acquire), 1);
    assert_eq!(stats.drops.load(Ordering::Acquire), 1);
    assert!(stats.first.load(Ordering::Acquire));
    assert!(!stats.wrong.load(Ordering::Acquire));
}
async fn scope_and_raw() -> Result<()> {
    let scope = Arc::new(ContainerBuilder::new().build().scope()?);
    let observed = stats();
    let body = Both::<()> {
        stats: Arc::clone(&observed),
        _output: std::marker::PhantomData,
    };
    scope.spawn(
        super::super::ExecutionMetadata::default(),
        None,
        move |_| body,
    )?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while observed.polls.load(Ordering::Acquire) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| container_error("test_poll_timeout", "scope任务未实际poll"))?;
    assert!(
        matches!(scope.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    check(&observed);
    let owner = Arc::new(ResourceOwner::new());
    let mut target = ContainerBuilder::new();
    target.resources = Some(owner.registrar());
    target.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = target.build();
    let resolver = target.resolve::<Resolver>().await?;
    let observed = stats();
    let counter = Arc::clone(&observed);
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |_| {
        Box::pin(Both::<Result<Arc<usize>>> {
            stats: Arc::clone(&counter),
            _output: std::marker::PhantomData,
        })
    });
    let source = source.build();
    let handle = source
        .bindings_snapshot()?
        .into_iter()
        .next()
        .map(|entry| entry.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))?;
    assert!(
        matches!(handle.invoke_internal::<usize>((*resolver).clone(),super::parameters::ParameterOverrides::new()).await,Err(error)if error.code()=="resource_task_interrupted")
    );
    check(&observed);
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
async fn scope_rejection_case() -> Result<()> {
    let scope = Arc::new(ContainerBuilder::new().build().scope()?);
    scope.close_cache();
    let mask = Arc::new(AtomicUsize::new(0));
    let captured = ParamBad::<0>(Arc::clone(&mask));
    let result = scope.spawn(
        super::super::ExecutionMetadata::default(),
        None,
        move |_| {
            let _owned = captured;
            std::future::ready(())
        },
    );
    let error = result
        .err()
        .ok_or_else(|| container_error("test_rejection_missing", "关闭scope不能准入"))?;
    assert_eq!(error.code(), "resource_task_interrupted");
    let source = std::error::Error::source(&error)
        .and_then(|source| {
            source.downcast_ref::<crate::foundation::resources::ResourceTaskRejection>()
        })
        .ok_or_else(|| container_error("test_source_missing", "必须保留拒绝与清理双诊断"))?;
    assert_eq!(source.registration.code(), "scope_closed");
    assert_eq!(source.cleanup.code(), "background_task_cleanup_interrupted");
    assert_eq!(mask.load(Ordering::Acquire), 1);
    assert!(
        matches!(scope.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
// 正式typed擦除层也必须保护真实用户Fut，而非仅保护外层async。
async fn extension_poll_drop_case(rebinding: bool) -> Result<()> {
    let owner = Arc::new(ResourceOwner::new());
    let mut builder = ContainerBuilder::new();
    builder.resources = Some(owner.registrar());
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    let observed = stats();
    let counter = Arc::clone(&observed);
    if rebinding {
        builder.rebinding::<usize, _>(move |_, _| {
            Box::pin(Both::<Result<()>> {
                stats: Arc::clone(&counter),
                _output: std::marker::PhantomData,
            })
        })?;
    } else {
        builder.extend::<usize, _>(move |_, _| {
            Box::pin(Both::<Result<Arc<usize>>> {
                stats: Arc::clone(&counter),
                _output: std::marker::PhantomData,
            })
        })?;
    }
    let container = builder.build();
    let result = if rebinding {
        container.instance::<usize>(Arc::new(2)).await
    } else {
        container.resolve::<usize>().await.map(|_| ())
    };
    if rebinding {
        assert!(matches!(result, Err(error) if error.code() == "resource_task_interrupted"));
    } else {
        assert!(
            matches!(result, Err(Error::ServiceInitialization { source, .. })
            if source.code() == "resource_task_interrupted")
        );
    }
    check(&observed);
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
#[test]
fn formal_scope_and_raw_foreign_future_poll_plus_drop_panics_are_isolated() -> Result<()> {
    const MARKER: &str = "OPENRUST_CT7_SCOPE_RAW_POLL_DROP_CHILD";
    if std::env::var_os(MARKER).is_some() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::internal)?
            .block_on(async {
                scope_and_raw().await?;
                scope_rejection_case().await?;
                extension_poll_drop_case(false).await?;
                extension_poll_drop_case(true).await
            })?;
        std::io::Write::write_all(
            &mut std::io::stdout().lock(),
            format!("{MARKER}:completed\n").as_bytes(),
        )
        .map_err(Error::internal)?;
        return Ok(());
    }
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, module)| module);
    let name =
        format!("{module}::formal_scope_and_raw_foreign_future_poll_plus_drop_panics_are_isolated");
    let output = std::process::Command::new(std::env::current_exe().map_err(Error::internal)?)
        .args(["--exact", &name, "--nocapture"])
        .env(MARKER, "1")
        .output()
        .map_err(Error::internal)?;
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("{MARKER}:completed")));
    assert!(
        output.status.success(),
        "scope/raw正式入口隔离进程必须正常结束：{}",
        output.status
    );
    Ok(())
}

struct ParamBad<const ID: usize>(Arc<AtomicUsize>);
impl<const ID: usize> Drop for ParamBad<ID> {
    fn drop(&mut self) {
        self.0.fetch_or(1 << ID, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("独立参数析构异常"));
    }
}
async fn parameter_case(rejected: bool) -> Result<()> {
    use super::parameters::{ParameterKey, ParameterOverrides, ParameterValue};
    let owner = Arc::new(ResourceOwner::new());
    let mut builder = ContainerBuilder::new();
    builder.resources = Some(owner.registrar());
    builder.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = builder.build();
    let resolver = target.resolve::<Resolver>().await?;
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
    let mask = Arc::new(AtomicUsize::new(0));
    let mut params = ParameterOverrides::new();
    params.insert(
        ParameterKey::Index(0),
        ParameterValue::new(ParamBad::<0>(Arc::clone(&mask))),
    );
    params.insert(
        ParameterKey::Index(1),
        ParameterValue::new(ParamBad::<1>(Arc::clone(&mask))),
    );
    if rejected {
        owner.seal();
    }
    let mut raw = handle.invoke_internal::<usize>((*resolver).clone(), params);
    if !rejected {
        assert!(futures_util::poll!(raw.as_mut()).is_pending());
        started.notified().await;
        target.seal_root();
    }
    let result = tokio::time::timeout(Duration::from_secs(3), raw)
        .await
        .map_err(|_| container_error("test_params_timeout", "独立参数清理不能漏fence"))?;
    assert!(matches!(result,Err(error)if error.code()=="resource_task_interrupted"));
    assert_eq!(mask.load(Ordering::Acquire), 3);
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
#[test]
fn two_owned_parameters_rejected_or_cancelled_are_isolated_in_child_process() -> Result<()> {
    const MARKER: &str = "OPENRUST_CT7_TWO_PARAMETERS_CHILD";
    if std::env::var_os(MARKER).is_some() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(Error::internal)?
            .block_on(async {
                parameter_case(true).await?;
                parameter_case(false).await
            })?;
        std::io::Write::write_all(
            &mut std::io::stdout().lock(),
            format!("{MARKER}:completed\n").as_bytes(),
        )
        .map_err(Error::internal)?;
        return Ok(());
    }
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, module)| module);
    let name = format!(
        "{module}::two_owned_parameters_rejected_or_cancelled_are_isolated_in_child_process"
    );
    let output = std::process::Command::new(std::env::current_exe().map_err(Error::internal)?)
        .args(["--exact", &name, "--nocapture"])
        .env(MARKER, "1")
        .output()
        .map_err(Error::internal)?;
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("{MARKER}:completed")));
    assert!(
        output.status.success(),
        "参数聚合隔离进程必须正常结束：{}",
        output.status
    );
    Ok(())
}
struct BusinessThenDrop;
impl Future for BusinessThenDrop {
    type Output = Result<Arc<usize>>;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::task::Poll::Ready(Err(Error::custom(
            "ct7_business_primary",
            "PRIVATE-BUSINESS-MESSAGE",
        )))
    }
}
impl Drop for BusinessThenDrop {
    fn drop(&mut self) {
        std::panic::resume_unwind(Box::new("独立Future Drop异常"));
    }
}
#[tokio::test]
async fn raw_business_error_is_retained_with_cleanup_unknown_and_sanitized_format() -> Result<()> {
    let owner = Arc::new(ResourceOwner::new());
    let mut builder = ContainerBuilder::new();
    builder.resources = Some(owner.registrar());
    builder.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = builder.build();
    let resolver = target.resolve::<Resolver>().await?;
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(|_| Box::pin(BusinessThenDrop));
    let source = source.build();
    let handle = source
        .bindings_snapshot()?
        .into_iter()
        .next()
        .map(|entry| entry.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))?;
    let error = handle
        .invoke_internal::<usize>(
            (*resolver).clone(),
            super::parameters::ParameterOverrides::new(),
        )
        .await
        .err()
        .ok_or_else(|| container_error("test_cleanup_missing", "不能把清理未知当作成功"))?;
    assert_eq!(error.code(), "resource_task_interrupted");
    assert!(!format!("{error:?} {error}").contains("PRIVATE-BUSINESS-MESSAGE"));
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    let mut found = false;
    for _ in 0..16 {
        let Some(current) = cause else {
            break;
        };
        if current
            .downcast_ref::<Error>()
            .is_some_and(|error| error.code() == "ct7_business_primary")
        {
            found = true;
            break;
        }
        cause = current.source();
    }
    assert!(found);
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(owner.shutdown(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
