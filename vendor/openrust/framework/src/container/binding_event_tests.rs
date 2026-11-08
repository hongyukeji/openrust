//! 验证关闭、重绑和作用域装饰的可观察顺序；一次性观察点只在当前测试线程生效。

use super::*;
use std::cell::RefCell;

type BoundaryProbe = RefCell<Option<Box<dyn FnOnce()>>>;
std::thread_local! {
    static PUBLICATION: BoundaryProbe = const { RefCell::new(None) };
    static INSTANCE_READ: BoundaryProbe = const { RefCell::new(None) };
}

pub(super) fn observe_publication() {
    if let Some(probe) = PUBLICATION.take() {
        probe();
    }
}
pub(super) fn observe_instance_read() {
    if let Some(probe) = INSTANCE_READ.take() {
        probe();
    }
}

fn history(container: &Container) -> bool {
    lock(&container.inner.state)
        .resolved_history
        .contains(&ServiceKey::typed::<usize>())
}

fn required_event(event: Option<Rebound>) -> Result<Rebound> {
    event.ok_or_else(|| container_error("test_event_missing", "提交应产生通知"))
}

async fn observe_values(container: &Container) -> Result<Arc<Mutex<Vec<usize>>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&seen);
    container
        .rebinding::<usize, _>(move |_, value| {
            lock(&recorded).push(*value);
            Box::pin(async { Ok(()) })
        })
        .await?;
    Ok(seen)
}

fn new_binding<F>(factory: F) -> Result<Binding>
where
    F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<usize>>> + Send + Sync + 'static,
{
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(factory);
    builder
        .bindings
        .remove(&ServiceKey::typed::<usize>())
        .ok_or_else(|| container_error("test_binding_missing", "缺少测试绑定"))
}

#[tokio::test]
async fn cached_outcome_and_history_commit_before_close_can_observe_either() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    let release = Arc::new(Notify::new());
    let gate = Arc::clone(&release);
    builder.scoped::<usize, _>(move |_| {
        let gate = Arc::clone(&gate);
        Box::pin(async move {
            gate.notified().await;
            Ok(Arc::new(19))
        })
    });
    let container = builder.build();
    let scope = container.scope()?;
    let mut pending = Box::pin(scope.resolve::<usize>());
    assert!(futures_util::poll!(pending.as_mut()).is_pending());
    let attempt = lock(&container.inner.state)
        .slots
        .values()
        .next()
        .cloned()
        .ok_or_else(|| container_error("test_slot_missing", "缺少待发布的 scoped 槽"))?;
    let inner = Arc::clone(&container.inner);
    let observed = Arc::new(AtomicBool::new(false));
    let probe = Arc::clone(&observed);
    PUBLICATION.set(Some(Box::new(move || {
        // 若两次提交间提前释放 State，关闭可抢先将同一槽发布为失败。
        probe.store(
            matches!(
                inner.state.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ) && matches!(
                attempt.outcome.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ),
            Ordering::Release,
        );
    })));
    release.notify_one();
    assert_eq!(*pending.await?, 19);
    assert!(observed.load(Ordering::Acquire));
    scope.close();
    assert!(history(&container));

    let mut builder = ContainerBuilder::new();
    builder.scoped::<usize, _>(|_| Box::pin(std::future::pending()));
    let container = builder.build();
    let scope = container.scope()?;
    let mut request = Box::pin(scope.resolve::<usize>());
    assert!(futures_util::poll!(request.as_mut()).is_pending());
    scope.close();
    assert!(matches!(request.await, Err(error) if error.code() == "scope_closed"));
    assert!(!history(&container));
    Ok(())
}

#[tokio::test]
async fn global_extend_after_forget_rebuilds_and_notifies_with_decorated_value() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    builder.singleton::<usize, _>(move |_| {
        let n = count.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(Arc::new(n + 1)) })
    });
    let container = builder.build();
    let seen = observe_values(&container).await?;
    container.forget_instance::<usize>().await?;
    container
        .extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 10)) }))
        .await?;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(*lock(&seen), vec![12]);
    assert_eq!(container.get_ready::<usize>().as_deref(), Some(&12));
    Ok(())
}

#[tokio::test]
async fn explicit_read_keeps_its_state_snapshot_until_it_has_selected_the_value() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(71)) }));
    let container = builder.build();
    let explicit = Arc::new(31_usize);
    container.instance(Arc::clone(&explicit)).await?;
    let inner = Arc::clone(&container.inner);
    let observed = Arc::new(AtomicBool::new(false));
    let probe = Arc::clone(&observed);
    INSTANCE_READ.set(Some(Box::new(move || {
        probe.store(
            matches!(
                inner.state.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ),
            Ordering::Release,
        );
    })));
    let result = container.get_instance::<usize>();
    assert!(
        result
            .as_ref()
            .is_some_and(|value| Arc::ptr_eq(value, &explicit))
    );
    assert!(observed.load(Ordering::Acquire));
    container.forget_instance::<usize>().await?;
    assert_eq!(*container.resolve::<usize>().await?, 71);
    assert!(container.get_instance::<usize>().is_none());
    assert_eq!(container.get_ready::<usize>().as_deref(), Some(&71));
    Ok(())
}

#[tokio::test]
async fn delayed_instance_events_use_their_committed_value_and_callback_list() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(1_usize));
    let container = builder.build();
    let first_listener = observe_values(&container).await?;
    let key = ServiceKey::typed::<usize>();
    let event_b = required_event(container.commit_binding_change(
        key.clone(),
        BindingChange::Instance(super::erase(Arc::new(2_usize))),
    )?)?;
    let second_listener = observe_values(&container).await?;
    let event_c = required_event(container.commit_binding_change(
        key,
        BindingChange::Instance(super::erase(Arc::new(3_usize))),
    )?)?;
    container.forget_instance::<usize>().await?;
    event_c.dispatch().await?;
    event_b.dispatch().await?;
    assert_eq!(*lock(&first_listener), vec![3, 2]);
    assert_eq!(*lock(&second_listener), vec![3]);
    assert!(!container.contains::<usize>());
    Ok(())
}

#[tokio::test]
async fn superseded_factory_event_only_runs_an_already_admitted_factory() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    let container = builder.build();
    let seen = observe_values(&container).await?;
    let key = ServiceKey::typed::<usize>();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let obsolete = new_binding(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Arc::new(2)) })
    })?;
    let skipped = required_event(
        container.commit_binding_change(key.clone(), BindingChange::Factory(obsolete))?,
    )?;
    let release = Arc::new(Notify::new());
    let gate = Arc::clone(&release);
    let slow = new_binding(move |_| {
        let gate = Arc::clone(&gate);
        Box::pin(async move {
            gate.notified().await;
            Ok(Arc::new(3))
        })
    })?;
    let event = required_event(
        container.commit_binding_change(key.clone(), BindingChange::Factory(slow))?,
    )?;
    assert!(matches!(skipped.dispatch().await, Err(error) if error.code() == "binding_replaced"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&seen).is_empty());
    let mut admitted = Box::pin(event.dispatch());
    assert!(futures_util::poll!(admitted.as_mut()).is_pending());
    let current = new_binding(|_| Box::pin(async { Ok(Arc::new(4)) }))?;
    let latest =
        required_event(container.commit_binding_change(key, BindingChange::Factory(current))?)?;
    release.notify_one();
    admitted.await?;
    assert_eq!(*lock(&seen), vec![3]);
    assert!(container.get_ready::<usize>().is_none());
    latest.dispatch().await?;
    assert_eq!(*lock(&seen), vec![3, 4]);
    assert_eq!(container.get_ready::<usize>().as_deref(), Some(&4));
    Ok(())
}

#[tokio::test]
async fn queued_scoped_extension_is_readmitted_from_its_original_scope_after_forget() -> Result<()>
{
    let mut builder = ContainerBuilder::new();
    builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(5)) }));
    builder.instance(Arc::new(40_usize));
    let container = builder.build();
    let a = container.scope()?;
    let b = container.scope()?;
    let mut holding =
        Box::pin(container.extend::<usize, _>(|_, _| Box::pin(std::future::pending())));
    assert!(futures_util::poll!(holding.as_mut()).is_pending());
    let mut queued = Box::pin(
        a.extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 1)) })),
    );
    assert!(futures_util::poll!(queued.as_mut()).is_pending());
    container.forget_instance::<usize>().await?;
    queued.await?;
    drop(holding);
    assert_eq!(*a.resolve::<usize>().await?, 6);
    assert_eq!(*b.resolve::<usize>().await?, 5);
    Ok(())
}

#[tokio::test]
async fn superseded_scoped_operations_neither_call_extender_nor_remove_new_chain() -> Result<()> {
    for forgetting in [false, true] {
        let first = Arc::new(AtomicBool::new(true));
        let mut builder = ContainerBuilder::new();
        builder.scoped::<usize, _>(move |_| {
            let wait = first.swap(false, Ordering::AcqRel);
            Box::pin(async move {
                if wait {
                    std::future::pending::<()>().await;
                }
                Ok(Arc::new(5))
            })
        });
        builder
            .extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 100)) }))?;
        let container = builder.build();
        let a = container.scope()?;
        let b = container.scope()?;
        a.extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 1)) }))
            .await?;
        let mut old = Box::pin(a.resolve::<usize>());
        assert!(futures_util::poll!(old.as_mut()).is_pending());
        let called = Arc::new(AtomicBool::new(false));
        let probe = Arc::clone(&called);
        let mut stale: BoxFuture<'_, Result<()>> = if forgetting {
            Box::pin(a.forget_extenders::<usize>())
        } else {
            Box::pin(a.extend::<usize, _>(move |value, _| {
                probe.store(true, Ordering::Release);
                Box::pin(async move { Ok(value) })
            }))
        };
        assert!(futures_util::poll!(stale.as_mut()).is_pending());
        container.instance(Arc::new(90_usize)).await?;
        container.forget_instance::<usize>().await?;
        // 明确在旧构造还挂起时观察被撤销的装饰操作，不能永远等旧任务。
        let status = futures_util::poll!(stale.as_mut());
        assert!(
            matches!(status, std::task::Poll::Ready(Err(error)) if error.code() == "binding_replaced")
        );
        assert!(!called.load(Ordering::Acquire));
        drop(stale);
        drop(old);
        assert_eq!(*a.resolve::<usize>().await?, 106);
        assert_eq!(*b.resolve::<usize>().await?, 105);
    }
    Ok(())
}

#[tokio::test]
async fn global_extend_after_explicit_only_read_does_not_invent_history() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(7)) }));
    builder.instance(Arc::new(2_usize));
    let container = builder.build();
    let seen = observe_values(&container).await?;
    container.forget_instance::<usize>().await?;
    container
        .extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 1)) }))
        .await?;
    assert!(lock(&seen).is_empty());
    assert!(container.get_ready::<usize>().is_none());
    assert!(!history(&container));
    assert_eq!(*container.resolve::<usize>().await?, 8);
    Ok(())
}
