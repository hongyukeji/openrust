//! 检出别名被统一归一、条件登记 TOCTOU、原工厂被覆盖与 raw 调用泄漏状态/owner 的缺陷。

use super::directory::{FactoryHandle, RegistrationOutcome};
use super::parameters::{ParameterKey, ParameterValue};
use super::*;
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

fn root(container: &Container) -> Resolver {
    let state = lock(&container.inner.state);
    container.resolver(
        state.root_scope,
        Arc::clone(&state.root_closed),
        state.root_resources.registrar(),
    )
}

fn handle(container: &Container, key: &ServiceKey) -> Result<FactoryHandle> {
    container
        .bindings_snapshot()?
        .into_iter()
        .find(|entry| &entry.key == key)
        .map(|entry| entry.concrete)
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂快照"))
}

#[tokio::test]
async fn dangling_and_cyclic_aliases_are_bound_without_eager_resolution() -> Result<()> {
    let container = ContainerBuilder::new().build();
    let target = ServiceKey::named("missing");
    let alias = ServiceKey::named("alias");
    container.alias_key(target.clone(), alias.clone())?;
    {
        let state = lock(&container.inner.state);
        assert!(state.bound_key(&alias));
        assert!(!state.bound_key(&target));
        assert_eq!(state.aliases.resolve(&alias)?, target);
        assert!(!state.shared_key(&alias));
        assert!(!state.resolved_key(&alias)?);
    }
    assert!(
        matches!(root(&container).resolve_key::<usize>(alias.clone()).await, Err(error) if error.code()=="service_not_bound")
    );
    assert!(
        matches!(container.alias_key(alias.clone(), alias.clone()), Err(error) if error.code()=="service_alias_self")
    );
    container.alias_key(alias.clone(), target.clone())?;
    let state = lock(&container.inner.state);
    assert!(state.bound_key(&target));
    assert!(state.bound_key(&alias));
    assert!(
        matches!(state.aliases.resolve(&alias), Err(error) if error.code()=="service_alias_cycle")
    );
    assert!(
        matches!(state.resolved_key(&alias), Err(error) if error.code()=="service_alias_cycle")
    );
    Ok(())
}

#[tokio::test]
async fn raw_shared_and_forget_do_not_follow_alias_but_make_and_history_do() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(17)) }));
    let key = ServiceKey::typed::<usize>();
    let alias = ServiceKey::named("service");
    builder.alias_key(key.clone(), alias.clone())?;
    let container = builder.build();
    let first = root(&container).resolve_key::<usize>(alias.clone()).await?;
    {
        let state = lock(&container.inner.state);
        assert!(state.resolved_key(&alias)?);
        assert!(!state.shared_key(&alias));
        assert!(state.shared_key(&key));
    }
    container.forget_instance_key(alias.clone()).await?;
    assert!(Arc::ptr_eq(&first, &container.resolve::<usize>().await?));
    container
        .instance_key(alias.clone(), Arc::new(8_usize))
        .await?;
    assert!(!lock(&container.inner.state).aliases.contains(&alias));
    assert_eq!(
        *root(&container).resolve_key::<usize>(alias.clone()).await?,
        8
    );
    assert_eq!(*container.resolve::<usize>().await?, 17);
    container.forget_instance_key(alias.clone()).await?;
    assert!(!lock(&container.inner.state).bound_key(&alias));
    assert_eq!(container.bindings_snapshot()?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn conditional_registration_noop_preserves_epoch_cache_callbacks_and_drops_outside_state()
-> Result<()> {
    struct Capture {
        inner: Weak<Inner>,
        outside: Arc<AtomicBool>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            if let Some(inner) = self.inner.upgrade() {
                self.outside
                    .store(inner.state.try_lock().is_ok(), Ordering::Release);
            }
        }
    }
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(3)) }));
    let container = builder.build();
    let original = container.resolve::<usize>().await?;
    let key = ServiceKey::typed::<usize>();
    let alias = ServiceKey::named("default");
    container.alias_key(key.clone(), alias.clone())?;
    let before = lock(&container.inner.state).next_generation;
    for target in [key, alias] {
        let outside = Arc::new(AtomicBool::new(false));
        let capture = Capture {
            inner: Arc::downgrade(&container.inner),
            outside: Arc::clone(&outside),
        };
        let result = container
            .register_key::<usize, _>(
                target,
                Lifetime::Bind,
                move |_| {
                    let _ = &capture;
                    Box::pin(async {
                        Err(container_error("test_factory_called", "no-op 不得运行工厂"))
                    })
                },
                true,
            )
            .await?;
        assert_eq!(result, RegistrationOutcome::AlreadyBound);
        assert!(outside.load(Ordering::Acquire));
        assert_eq!(lock(&container.inner.state).next_generation, before);
        assert!(Arc::ptr_eq(&original, &container.resolve::<usize>().await?));
    }
    assert!(
        lock(&container.inner.state)
            .aliases
            .contains(&ServiceKey::named("default"))
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_conditionals_install_exactly_one_named_factory() -> Result<()> {
    let container = ContainerBuilder::new().build();
    let gate = Arc::new(tokio::sync::Barrier::new(3));
    let spawn = |value: usize| {
        let container = container.clone();
        let gate = Arc::clone(&gate);
        tokio::spawn(async move {
            gate.wait().await;
            container
                .register_key::<usize, _>(
                    ServiceKey::named("same"),
                    Lifetime::Singleton,
                    move |_| Box::pin(async move { Ok(Arc::new(value)) }),
                    true,
                )
                .await
        })
    };
    let one = spawn(1);
    let two = spawn(2);
    gate.wait().await;
    let a = one.await.map_err(Error::internal)??;
    let b = two.await.map_err(Error::internal)??;
    assert!(matches!(
        (a, b),
        (
            RegistrationOutcome::Registered,
            RegistrationOutcome::AlreadyBound
        ) | (
            RegistrationOutcome::AlreadyBound,
            RegistrationOutcome::Registered
        )
    ));
    assert_eq!(lock(&container.inner.state).next_generation, 1);
    assert_eq!(container.bindings_snapshot()?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn named_expected_type_never_changes_identity_or_runs_wrong_factory() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let mut builder = ContainerBuilder::new();
    let key = ServiceKey::named("mail");
    builder.register_key::<usize, _>(
        key.clone(),
        Lifetime::Singleton,
        move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Arc::new(1)) })
        },
        false,
    );
    let container = builder.build();
    assert!(
        matches!(root(&container).resolve_key::<String>(key.clone()).await, Err(error) if error.code()=="service_type_mismatch")
    );
    assert!(
        matches!(handle(&container, &key)?.invoke_internal::<String>(root(&container), ParameterOverrides::new()).await, Err(error) if error.code()=="service_type_mismatch")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(container.bindings_snapshot()?.len(), 1);
    assert_eq!(*root(&container).resolve_key::<usize>(key).await?, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn raw_snapshot_preserves_original_factory_shared_flag_and_skips_outer_state() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(move |_| {
        let number = count.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(async move { Ok(Arc::new(number)) })
    });
    builder.extend::<usize, _>(|v, _| Box::pin(async move { Ok(Arc::new(*v + 100)) }))?;
    let container = builder.build();
    let key = ServiceKey::typed::<usize>();
    let entry = container.bindings_snapshot()?.remove(0);
    assert!(entry.shared);
    let original = entry.concrete;
    assert!(original.same_factory(&handle(&container, &key)?));
    for expected in [1, 2] {
        assert_eq!(
            *original
                .invoke_internal::<usize>(root(&container), ParameterOverrides::new())
                .await?,
            expected
        );
        assert!(!lock(&container.inner.state).resolved_key(&key)?);
        assert!(container.get_ready::<usize>().is_none());
    }
    assert_eq!(*container.resolve::<usize>().await?, 103);
    container
        .instance(Arc::new(String::from("override")))
        .await?;
    container.instance(Arc::new(99_usize)).await?;
    assert!(original.same_factory(&handle(&container, &key)?));
    assert_eq!(
        *original
            .invoke_internal::<usize>(root(&container), ParameterOverrides::new())
            .await?,
        4
    );
    assert_eq!(*container.resolve::<usize>().await?, 99);
    container.forget_instance::<usize>().await?;
    assert!(original.same_factory(&handle(&container, &key)?));
    container
        .bind::<usize, _>(|_| Box::pin(async { Ok(Arc::new(40)) }))
        .await?;
    assert!(!original.same_factory(&handle(&container, &key)?));
    assert_eq!(
        *original
            .invoke_internal::<usize>(root(&container), ParameterOverrides::new())
            .await?,
        5
    );
    assert_eq!(*container.resolve::<usize>().await?, 140);
    Ok(())
}

#[tokio::test]
async fn raw_uses_supplied_container_and_nested_make_has_fresh_parameters() -> Result<()> {
    let key = ServiceKey::named("outer");
    let mut builder = ContainerBuilder::new();
    builder.register_key::<String, _>(
        key.clone(),
        Lifetime::Bind,
        |r| {
            Box::pin(async move {
                let label = r
                    .factory_parameters()
                    .get(&ParameterKey::name("label"))
                    .ok_or_else(|| container_error("test_parameter_missing", "缺少 label"))?
                    .downcast::<String>()?;
                assert!(
                    r.factory_parameters()
                        .get(&ParameterKey::Index(0))
                        .is_some_and(ParameterValue::is_null)
                );
                let dependency = r.resolve::<String>().await?;
                Ok(Arc::new(format!("{label}:{dependency}")))
            })
        },
        false,
    );
    let source = builder.build();
    let factory = handle(&source, &key)?;
    let mut other = ContainerBuilder::new();
    other.bind::<String, _>(|r| {
        Box::pin(async move {
            assert!(r.factory_parameters().is_empty());
            Ok(Arc::new(String::from("B")))
        })
    });
    let other = other.build();
    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        ParameterKey::name("label"),
        ParameterValue::new(String::from("input")),
    );
    parameters.insert(ParameterKey::Index(0), ParameterValue::null());
    assert_eq!(
        &*factory
            .invoke_internal::<String>(root(&other), parameters)
            .await?,
        "input:B"
    );
    assert!(!lock(&other.inner.state).resolved_key(&key)?);
    assert!(lock(&other.inner.state).resolved_key(&ServiceKey::typed::<String>())?);
    assert!(!lock(&source.inner.state).resolved_key(&key)?);
    Ok(())
}

#[tokio::test]
async fn raw_call_is_not_cancelled_by_rebind_but_root_shutdown_cancels_and_joins() -> Result<()> {
    let started = Arc::new(Notify::new());
    let gate = Arc::new(Notify::new());
    let entered = Arc::clone(&started);
    let release = Arc::clone(&gate);
    let mut builder = ContainerBuilder::new();
    builder.bind::<usize, _>(move |_| {
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        Box::pin(async move {
            entered.notify_one();
            release.notified().await;
            Ok(Arc::new(1))
        })
    });
    let container = builder.build();
    let factory = handle(&container, &ServiceKey::typed::<usize>())?;
    let mut raw = factory.invoke_internal::<usize>(root(&container), ParameterOverrides::new());
    tokio::select! { result=&mut raw=>{result?;return Err(container_error("test_not_pending","原工厂应挂起"));}, ()=started.notified()=>{} }
    container
        .bind::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
        .await?;
    assert!(futures_util::poll!(raw.as_mut()).is_pending());
    gate.notify_one();
    assert_eq!(*raw.await?, 1);
    assert!(!lock(&container.inner.state).resolved_key(&ServiceKey::typed::<usize>())?);

    let mut pending = factory.invoke_internal::<usize>(root(&container), ParameterOverrides::new());
    tokio::select! { result=&mut pending=>{result?;return Err(container_error("test_not_pending","第二次原工厂应挂起"));}, ()=started.notified()=>{} }
    container
        .shutdown_root(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(matches!(pending.await, Err(error) if error.code()=="container_closed"));
    assert!(lock(&container.inner.state).raw_attempts.is_empty());
    Ok(())
}

#[tokio::test]
async fn closed_and_expired_raw_calls_never_run_user_factory() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let mut builder = ContainerBuilder::new();
    builder.bind::<usize, _>(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(Arc::new(1)) })
    });
    let container = builder.build();
    let factory = handle(&container, &ServiceKey::typed::<usize>())?;
    let scope = container.scope()?;
    let resolver = container.resolver(
        scope.id,
        Arc::clone(&scope.closed),
        scope.resources.registrar(),
    );
    scope.close();
    assert!(
        matches!(factory.invoke_internal::<String>(resolver, ParameterOverrides::new()).await, Err(error) if error.code()=="scope_closed")
    );
    let expired = crate::core::ExecutionContext::new(
        crate::core::ExecutionMetadata::default(),
        Some(tokio::time::Instant::now()),
        crate::core::Cancellation::new(),
    );
    let result = crate::core::with_execution_context(
        expired,
        factory.invoke_internal::<usize>(root(&container), ParameterOverrides::new()),
    )
    .await;
    assert!(matches!(result, Err(error) if error.code()=="execution_deadline_exceeded"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(lock(&container.inner.state).raw_attempts.is_empty());
    Ok(())
}

#[tokio::test]
async fn declared_class_symbol_uses_same_raw_key_before_alias_policy() -> Result<()> {
    use super::class_identity::{ClassIdentities, ClassIdentity};
    let classes = ClassIdentities::from_entries([ClassIdentity::of::<usize>("Acme\\Counter")])?;
    let mut builder = ContainerBuilder::with_class_identities(classes);
    let symbol = ServiceKey::named("Acme\\Counter");
    let typed = ServiceKey::typed::<usize>();
    assert!(!builder.bound_key(&symbol));
    assert!(!builder.contains::<usize>());
    assert!(
        matches!(builder.alias_key(typed.clone(), symbol.clone()), Err(error) if error.code()=="service_alias_self")
    );
    builder.register_key::<usize, _>(
        symbol.clone(),
        Lifetime::Singleton,
        |_| Box::pin(async { Ok(Arc::new(5)) }),
        false,
    );
    let container = builder.build();
    let a = root(&container)
        .resolve_key::<usize>(symbol.clone())
        .await?;
    assert!(Arc::ptr_eq(&a, &container.resolve::<usize>().await?));
    assert_eq!(container.bindings_snapshot()?.len(), 1);
    assert_eq!(container.bindings_snapshot()?.remove(0).key, typed);
    assert!(lock(&container.inner.state).resolved_key(&symbol)?);
    assert_eq!(
        container
            .register_key::<usize, _>(
                symbol.clone(),
                Lifetime::Bind,
                |_| Box::pin(async { Ok(Arc::new(99)) }),
                true
            )
            .await?,
        RegistrationOutcome::AlreadyBound
    );
    container
        .instance_key(symbol.clone(), Arc::new(7_usize))
        .await?;
    assert_eq!(*container.resolve::<usize>().await?, 7);
    container.forget_instance_key(symbol.clone()).await?;
    assert_eq!(*container.resolve::<usize>().await?, 5);
    container.alias_key(symbol.clone(), ServiceKey::named("counter"))?;
    assert!(!lock(&container.inner.state).shared_key(&ServiceKey::named("counter")));
    assert!(lock(&container.inner.state).shared_key(&symbol));
    assert!(lock(&container.inner.state).resolved_key(&ServiceKey::named("counter"))?);
    Ok(())
}

#[tokio::test]
async fn raw_closure_sync_entry_observes_current_execution_context() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.bind::<usize, _>(|_| {
        let present = crate::core::ExecutionContext::current().is_some();
        Box::pin(async move { Ok(Arc::new(usize::from(present))) })
    });
    let container = builder.build();
    let factory = handle(&container, &ServiceKey::typed::<usize>())?;
    let context = crate::core::ExecutionContext::new(
        crate::core::ExecutionMetadata::default(),
        None,
        crate::core::Cancellation::new(),
    );
    let result = crate::core::with_execution_context(
        context,
        factory.invoke_internal::<usize>(root(&container), ParameterOverrides::new()),
    )
    .await?;
    assert_eq!(*result, 1);
    Ok(())
}

#[tokio::test]
async fn delayed_factory_event_keeps_original_key_when_new_alias_points_elsewhere() -> Result<()> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let values = Arc::clone(&seen);
    let mut builder = ContainerBuilder::new();
    builder.bind::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    builder.register_key::<usize, _>(
        ServiceKey::named("other"),
        Lifetime::Bind,
        |_| Box::pin(async { Ok(Arc::new(2)) }),
        false,
    );
    builder.rebinding::<usize, _>(move |_, value| {
        lock(&values).push(*value);
        Box::pin(async { Ok(()) })
    })?;
    let container = builder.build();
    let resolver = root(&container);
    let key = ServiceKey::typed::<usize>();
    let event = {
        let state = lock(&container.inner.state);
        let binding = state
            .bindings
            .get(&key)
            .ok_or_else(|| container_error("test_binding_missing", "缺少绑定"))?;
        Rebound::capture(&state, &container.inner, &resolver, &key, binding, None)
            .ok_or_else(|| container_error("test_event_missing", "缺少事件"))?
    };
    container.alias_key(ServiceKey::named("other"), key)?;
    event.dispatch().await?;
    assert_eq!(*lock(&seen), vec![1]);
    assert_eq!(*container.resolve::<usize>().await?, 2);
    Ok(())
}

#[tokio::test]
async fn raw_does_not_bypass_singleton_captive_scope_protection() -> Result<()> {
    let mut source = ContainerBuilder::new();
    source.bind::<String, _>(|r| {
        Box::pin(async move {
            let value = r.resolve::<usize>().await?;
            Ok(Arc::new(value.to_string()))
        })
    });
    let source = source.build();
    let factory = handle(&source, &ServiceKey::typed::<String>())?;
    let owner = ResourceOwner::new();
    let mut destination = ContainerBuilder::new();
    destination.resources = Some(owner.registrar());
    destination.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(5)) }));
    destination.singleton::<String, _>(move |r| {
        factory.invoke_internal::<String>(r, ParameterOverrides::new())
    });
    let destination = destination.build();
    assert!(
        matches!(destination.resolve::<String>().await, Err(error) if error.code()=="captive_scope_dependency")
    );
    assert!(!lock(&destination.inner.state).resolved_key(&ServiceKey::typed::<usize>())?);
    owner
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    Ok(())
}

#[tokio::test]
async fn managed_raw_call_preserves_entry_scope_and_caller_drop_cancels_its_future() -> Result<()> {
    struct MarkDrop(Arc<AtomicBool>);
    impl Drop for MarkDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let target = ContainerBuilder::new().build();
    let scope = Arc::new(target.scope()?);
    let observed = target.clone();
    let started = Arc::new(Notify::new());
    let entered = Arc::clone(&started);
    let dropped = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&dropped);
    let mut source = ContainerBuilder::new();
    source.bind::<usize, _>(move |_| {
        let present = observed.current_entry_scope().is_some();
        let entered = Arc::clone(&entered);
        let finished = Arc::clone(&finished);
        Box::pin(async move {
            assert!(present);
            let _drop = MarkDrop(finished);
            entered.notify_one();
            std::future::pending::<Result<Arc<usize>>>().await
        })
    });
    let source = source.build();
    let factory = handle(&source, &ServiceKey::typed::<usize>())?;
    let raw = factory.invoke_internal::<usize>(
        scope.factory_resolver_internal()?,
        ParameterOverrides::new(),
    );
    let mut raw = Box::pin(scope.run(raw));
    tokio::select! { result=&mut raw=>{result?;return Err(container_error("test_not_pending", "raw 应挂起"));}, ()=started.notified()=>{} }
    drop(raw);
    scope
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(dropped.load(Ordering::Acquire));
    assert!(lock(&target.inner.state).raw_attempts.is_empty());
    Ok(())
}

#[tokio::test]
async fn alias_lookup_is_independent_of_typed_output_preflight() -> Result<()> {
    trait View: Send + Sync {
        fn number(&self) -> usize;
    }
    impl View for usize {
        fn number(&self) -> usize {
            *self
        }
    }

    let key = ServiceKey::named("view-source");
    let alias = ServiceKey::named("view-alias");
    let calls = Arc::new(AtomicUsize::new(0));
    let captured = Arc::clone(&calls);
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(17_usize));
    assert!(matches!(
        builder.register_key::<dyn View, _>(
            key.clone(),
            Lifetime::Singleton,
            move |_| {
                captured.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok(Arc::new(42_usize) as Arc<dyn View>) })
            },
            false,
        ),
        RegistrationOutcome::Registered
    ));
    builder.alias_key(key.clone(), alias.clone())?;
    let container = builder.build();
    let resolver = root(&container);

    assert!(matches!(
        resolver.resolve_key::<usize>(alias.clone()).await,
        Err(error) if error.code() == "service_type_mismatch"
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(*container.resolve::<usize>().await?, 17);

    let view = resolver.resolve_key::<dyn View>(alias).await?;
    let named = resolver.resolve_key::<dyn View>(key).await?;
    assert_eq!(view.number(), 42);
    assert!(Arc::ptr_eq(&view, &named));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}
