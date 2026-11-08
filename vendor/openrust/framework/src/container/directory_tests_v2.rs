//! 针对独审五项反例的确定性交错；静态源码不代表运行通过。
use super::parameters::ParameterOverrides;
use super::*;
use std::cell::RefCell;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

std::thread_local! { static SELECTED: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) }; }
pub(super) fn observe_selected() {
    if let Some(probe) = SELECTED.take() {
        probe();
    }
}
#[derive(Default)]
struct SelectionPause {
    entered: AtomicBool,
    used: AtomicBool,
    resume: Notify,
}
tokio::task_local! { static PAUSE: Arc<SelectionPause>; }
pub(super) async fn after_binding_selection() {
    if let Ok(pause) = PAUSE.try_with(Arc::clone)
        && !pause.used.swap(true, Ordering::AcqRel)
    {
        pause.entered.store(true, Ordering::Release);
        pause.resume.notified().await;
    }
}
fn handle(container: &Container, key: &ServiceKey) -> Result<super::directory::FactoryHandle> {
    container
        .bindings_snapshot()?
        .into_iter()
        .find(|item| &item.key == key)
        .map(|item| item.concrete)
        .ok_or_else(|| container_error("test_handle_missing", "缺少原工厂"))
}

#[tokio::test]
async fn builder_follows_alias_for_extend_forget_and_rebinding_and_reports_cycles() -> Result<()> {
    for forget in [false, true] {
        let key = ServiceKey::named("target");
        let mut builder = ContainerBuilder::new();
        builder.register_key::<usize, _>(
            key.clone(),
            Lifetime::Singleton,
            |_| Box::pin(async { Ok(Arc::new(1)) }),
            false,
        );
        builder.alias_key(key.clone(), ServiceKey::typed::<usize>())?;
        builder
            .extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 10)) }))?;
        if forget {
            builder.forget_extenders::<usize>()?;
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        builder.rebinding::<usize, _>(move |_, value| {
            lock(&recorded).push(*value);
            Box::pin(async { Ok(()) })
        })?;
        let container = builder.build();
        assert_eq!(
            *container.resolve::<usize>().await?,
            if forget { 1 } else { 11 }
        );
        container
            .extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 1)) }))
            .await?;
        let extended = if forget { 2 } else { 12 };
        assert_eq!(*container.resolve::<usize>().await?, extended);
        container.instance_key(key, Arc::new(99_usize)).await?;
        assert_eq!(*lock(&seen), vec![extended, 99]);
    }
    let mut builder = ContainerBuilder::new();
    let target = ServiceKey::named("target");
    let typed = ServiceKey::typed::<usize>();
    builder.alias_key(target.clone(), typed.clone())?;
    builder.alias_key(typed, target)?;
    assert!(
        matches!(builder.extend::<usize,_>(|value,_|Box::pin(async move{Ok(value)})),Err(error)if error.code()=="service_alias_cycle")
    );
    assert!(
        matches!(builder.forget_extenders::<usize>(),Err(error)if error.code()=="service_alias_cycle")
    );
    assert!(
        matches!(builder.rebinding::<usize,_>(|_,_|Box::pin(async{Ok(())})),Err(error)if error.code()=="service_alias_cycle")
    );
    Ok(())
}

#[tokio::test]
async fn bindings_keep_first_factory_insertion_position_without_instance_only_or_alias_slots()
-> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance_key(ServiceKey::named("later"), Arc::new(0_usize));
    builder.instance_key(ServiceKey::named("instance-only"), Arc::new(0_usize));
    for name in ["first", "second", "third", "first"] {
        builder.register_key::<usize, _>(
            ServiceKey::named(name),
            Lifetime::Bind,
            |_| Box::pin(async { Ok(Arc::new(1)) }),
            false,
        );
    }
    builder.alias_key(ServiceKey::named("first"), ServiceKey::named("alias-only"))?;
    let container = builder.build();
    container
        .register_key::<usize, _>(
            ServiceKey::named("later"),
            Lifetime::Bind,
            |_| Box::pin(async { Ok(Arc::new(2)) }),
            false,
        )
        .await?;
    container
        .register_key::<usize, _>(
            ServiceKey::named("second"),
            Lifetime::Singleton,
            |_| Box::pin(async { Ok(Arc::new(3)) }),
            false,
        )
        .await?;
    assert_eq!(
        container
            .bindings_snapshot()?
            .iter()
            .map(|item| item.key.name().to_owned())
            .collect::<Vec<_>>(),
        vec!["first", "second", "third", "later"]
    );
    Ok(())
}

#[tokio::test]
async fn alias_and_binding_are_selected_under_one_lock_and_retry_preserves_requested_key()
-> Result<()> {
    let old = Arc::new(AtomicUsize::new(0));
    let new = Arc::new(AtomicUsize::new(0));
    let other = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    for (name, value, calls) in [("A", 1, Arc::clone(&old)), ("B", 2, Arc::clone(&other))] {
        builder.register_key::<usize, _>(
            ServiceKey::named(name),
            Lifetime::Bind,
            move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move { Ok(Arc::new(value)) })
            },
            false,
        );
    }
    builder.alias_key(ServiceKey::named("A"), ServiceKey::named("x"))?;
    let container = builder.build();
    let resolver = container.factory_resolver_internal()?;
    let observed = Arc::new(AtomicBool::new(false));
    let probe = Arc::clone(&observed);
    let inner = Arc::clone(&container.inner);
    SELECTED.set(Some(Box::new(move || {
        probe.store(inner.state.try_lock().is_err(), Ordering::Release);
    })));
    let pause = Arc::new(SelectionPause::default());
    let mut pending = Box::pin(PAUSE.scope(
        Arc::clone(&pause),
        resolver.resolve_key::<usize>(ServiceKey::named("x")),
    ));
    assert!(futures_util::poll!(pending.as_mut()).is_pending());
    assert!(pause.entered.load(Ordering::Acquire));
    assert!(observed.load(Ordering::Acquire));
    container.alias_key(ServiceKey::named("B"), ServiceKey::named("x"))?;
    let count = Arc::clone(&new);
    container
        .register_key::<usize, _>(
            ServiceKey::named("A"),
            Lifetime::Bind,
            move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(Arc::new(3)) })
            },
            false,
        )
        .await?;
    pause.resume.notify_one();
    assert_eq!(*pending.await?, 2);
    assert_eq!(old.load(Ordering::SeqCst), 0);
    assert_eq!(new.load(Ordering::SeqCst), 0);
    assert_eq!(other.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn alias_only_retarget_reselects_before_transient_and_cached_admission() -> Result<()> {
    for lifetime in [Lifetime::Bind, Lifetime::Singleton, Lifetime::Scoped] {
        let left = ServiceKey::named("left");
        let right = ServiceKey::named("right");
        let alias = ServiceKey::named("selected");
        let old_calls = Arc::new(AtomicUsize::new(0));
        let mut builder = ContainerBuilder::new();
        builder.register_key(
            left.clone(),
            lifetime,
            {
                let old_calls = Arc::clone(&old_calls);
                move |_| {
                    old_calls.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async { Ok(Arc::new(1usize)) })
                }
            },
            false,
        );
        builder.register_key(
            right.clone(),
            lifetime,
            |_| Box::pin(async { Ok(Arc::new(2usize)) }),
            false,
        );
        builder.alias(left, alias.clone())?;
        let container = builder.build();
        let scope = container.scope()?;
        let pause = Arc::new(SelectionPause::default());
        let mut pending = Box::pin(PAUSE.scope(
            Arc::clone(&pause),
            scope.resolve_key::<usize>(alias.clone()),
        ));
        assert!(futures_util::poll!(pending.as_mut()).is_pending());
        assert!(pause.entered.load(Ordering::Acquire));
        container.alias(right, alias)?;
        pause.resume.notify_one();
        assert_eq!(*pending.await?, 2);
        assert_eq!(old_calls.load(Ordering::SeqCst), 0);
    }
    Ok(())
}

#[tokio::test]
async fn raw_entry_scope_must_match_inner_scope_id_and_scope_status_of_explicit_owner() -> Result<()>
{
    let target = ContainerBuilder::new().build();
    let first = Arc::new(target.scope()?);
    let second = target.scope()?;
    let inspect = target.clone();
    let mut source = ContainerBuilder::new();
    source.bind::<Option<u64>, _>(move |_| {
        let id = inspect.current_entry_scope().map(|scope| scope.id);
        Box::pin(async move { Ok(Arc::new(id)) })
    });
    let source = source.build();
    let factory = handle(&source, &ServiceKey::typed::<Option<u64>>())?;
    let another = ContainerBuilder::new().build();
    for (resolver, expected) in [
        (first.factory_resolver_internal()?, Some(first.id)),
        (second.factory_resolver_internal()?, None),
        (target.factory_resolver_internal()?, None),
        (another.factory_resolver_internal()?, None),
    ] {
        let value = first
            .run(factory.invoke_internal::<Option<u64>>(resolver, ParameterOverrides::new()))
            .await?;
        assert_eq!(*value, expected);
    }
    Ok(())
}

#[tokio::test]
async fn singleton_raw_rechecks_actual_owner_sealed_during_last_user_poll_and_drops_candidate_outside_locks()
-> Result<()> {
    struct Candidate {
        registrar: ResourceRegistrar,
        inner: Weak<Inner>,
        dropped: Arc<AtomicBool>,
    }
    impl Drop for Candidate {
        fn drop(&mut self) {
            let _ = self.registrar.on_shutdown(|| Box::pin(async { Ok(()) }));
            if let Some(inner) = self.inner.upgrade() {
                self.dropped
                    .store(inner.state.try_lock().is_ok(), Ordering::Release);
            }
        }
    }
    let owner = Arc::new(ResourceOwner::new());
    let mut destination = ContainerBuilder::new();
    destination.resources = Some(owner.registrar());
    // 保存真正 singleton factory 收到的 Resolver；其 ScopeStatus 与真实应用 owner 不同。
    destination
        .singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let destination = destination.build();
    let resolver = destination.resolve::<Resolver>().await?;
    let actual = Arc::clone(&owner);
    let dropped = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&dropped);
    let inner = Arc::downgrade(&destination.inner);
    let mut source = ContainerBuilder::new();
    source.bind::<Candidate, _>(move |_| {
        let actual = Arc::clone(&actual);
        let dropped = Arc::clone(&observed);
        let inner = inner.clone();
        Box::pin(async move {
            actual.seal();
            Ok(Arc::new(Candidate {
                registrar: actual.registrar(),
                inner,
                dropped,
            }))
        })
    });
    let source = source.build();
    let factory = handle(&source, &ServiceKey::typed::<Candidate>())?;
    assert!(
        matches!(factory.invoke_internal::<Candidate>((*resolver).clone(),ParameterOverrides::new()).await,Err(error)if error.code()=="resources_closed")
    );
    assert!(dropped.load(Ordering::Acquire));
    assert!(lock(&destination.inner.state).raw_attempts.is_empty());
    owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn root_shutdown_waits_for_nonroot_owner_future_and_final_factory_capture_drop() -> Result<()>
{
    use std::sync::Condvar;
    struct FinalCapture {
        entered: Arc<Notify>,
        gate: Arc<(Mutex<bool>, Condvar)>,
        dropped: Arc<AtomicBool>,
    }
    impl Drop for FinalCapture {
        fn drop(&mut self) {
            self.entered.notify_one();
            let (mutex, changed) = &*self.gate;
            let mut released = lock(mutex);
            while !*released {
                released = changed
                    .wait(released)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            self.dropped.store(true, Ordering::Release);
        }
    }
    struct Release(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            let (mutex, changed) = &*self.0;
            *lock(mutex) = true;
            changed.notify_all();
        }
    }
    let owner = ResourceOwner::new();
    let mut target = ContainerBuilder::new();
    target.resources = Some(owner.registrar());
    target.singleton::<Resolver, _>(|resolver| Box::pin(async move { Ok(Arc::new(resolver)) }));
    let target = target.build();
    let resolver = target.resolve::<Resolver>().await?;
    let entered = Arc::new(Notify::new());
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let dropped = Arc::new(AtomicBool::new(false));
    let release = Release(Arc::clone(&gate));
    let capture = FinalCapture {
        entered: Arc::clone(&entered),
        gate,
        dropped: Arc::clone(&dropped),
    };
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |_| {
        let _ = &capture;
        Box::pin(async { Ok(Arc::new(1)) })
    });
    let source = source.build();
    let factory = handle(&source, &ServiceKey::typed::<usize>())?;
    let mut raw = factory.invoke_internal::<usize>((*resolver).clone(), ParameterOverrides::new());
    drop(factory);
    drop(source);
    tokio::select! {result=&mut raw=>{result?;return Err(container_error("test_join_early","raw 不得早于最终捕获析构完成"));},()=entered.notified()=>{}}
    assert!(!dropped.load(Ordering::Acquire));
    assert!(futures_util::poll!(raw.as_mut()).is_pending());
    let mut shutdown =
        Box::pin(target.shutdown_root(tokio::time::Instant::now() + Duration::from_secs(3)));
    assert!(futures_util::poll!(shutdown.as_mut()).is_pending());
    drop(release);
    shutdown.await?;
    // 封根先于析构放行，尚未交付的值须在最终校验时被拒绝。
    assert!(matches!(raw.await, Err(error) if error.code() == "container_closed"));
    assert!(dropped.load(Ordering::Acquire));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    Ok(())
}

#[tokio::test]
async fn cancellation_on_last_user_poll_is_not_returned_as_success() -> Result<()> {
    let cancelled = crate::core::Cancellation::new();
    let trigger = cancelled.clone();
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |_| {
        let trigger = trigger.clone();
        Box::pin(async move {
            trigger.cancel();
            Ok(Arc::new(1))
        })
    });
    let source = source.build();
    let factory = handle(&source, &ServiceKey::typed::<usize>())?;
    let context = crate::core::ExecutionContext::new(
        crate::core::ExecutionMetadata::default(),
        None,
        cancelled,
    );
    let result = crate::core::with_execution_context(
        context,
        factory.invoke_internal::<usize>(
            source.factory_resolver_internal()?,
            ParameterOverrides::new(),
        ),
    )
    .await;
    assert!(matches!(result,Err(error)if error.code()=="execution_cancelled"));
    assert!(lock(&source.inner.state).raw_attempts.is_empty());
    Ok(())
}

#[tokio::test]
async fn independent_singletons_and_raw_factories_detect_wait_graph_cycle_and_clear_edges()
-> Result<()> {
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(|resolver| Box::pin(async move { resolver.resolve::<usize>().await }));
    source
        .bind::<String, _>(|resolver| Box::pin(async move { resolver.resolve::<String>().await }));
    let source = source.build();
    let to_a = handle(&source, &ServiceKey::typed::<usize>())?;
    let to_b = handle(&source, &ServiceKey::typed::<String>())?;
    let owner = ResourceOwner::new();
    let mut target = ContainerBuilder::new();
    target.resources = Some(owner.registrar());
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let a_barrier = Arc::clone(&barrier);
    target.singleton::<usize, _>(move |resolver| {
        let barrier = Arc::clone(&a_barrier);
        let raw = to_b.invoke_internal::<String>(resolver, ParameterOverrides::new());
        Box::pin(async move {
            barrier.wait().await;
            raw.await?;
            Ok(Arc::new(1))
        })
    });
    target.singleton::<String, _>(move |resolver| {
        let barrier = Arc::clone(&barrier);
        let raw = to_a.invoke_internal::<usize>(resolver, ParameterOverrides::new());
        Box::pin(async move {
            barrier.wait().await;
            raw.await?;
            Ok(Arc::new(String::from("resolved")))
        })
    });
    let target = target.build();
    let (a, b) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(target.resolve::<usize>(), target.resolve::<String>())
    })
    .await
    .map_err(Error::internal)?;
    assert!(matches!(a,Err(error)if error.code()=="service_cycle"));
    assert!(matches!(b,Err(error)if error.code()=="service_cycle"));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(lock(&target.inner.state).edges.is_empty());
    owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    Ok(())
}

#[tokio::test]
async fn raw_candidate_success_is_not_delivered_when_final_capture_drop_is_unknown() -> Result<()> {
    struct BrokenCapture;
    impl Drop for BrokenCapture {
        fn drop(&mut self) {
            std::panic::resume_unwind(Box::new("受控工厂捕获析构失败"));
        }
    }
    let capture = BrokenCapture;
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |_| {
        let _ = &capture;
        Box::pin(async { Ok(Arc::new(1)) })
    });
    let source = source.build();
    let factory = handle(&source, &ServiceKey::typed::<usize>())?;
    let target = ContainerBuilder::new().build();
    let raw = factory.invoke_internal::<usize>(
        target.factory_resolver_internal()?,
        ParameterOverrides::new(),
    );
    drop(factory);
    drop(source);
    assert!(matches!(raw.await,Err(error)if error.code()=="resource_task_interrupted"));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    assert!(
        matches!(target.shutdown_root(tokio::time::Instant::now()+Duration::from_secs(3)).await,Err(error)if error.code()=="resource_shutdown_failed")
    );
    Ok(())
}
