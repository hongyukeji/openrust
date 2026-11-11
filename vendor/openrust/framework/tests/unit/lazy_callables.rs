use openrust::{
    container::{
        Callback, ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue,
        ServiceKey,
    },
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn factories_are_lazy_and_follow_live_aliases_instances_and_factories() {
    let c = Container::default();
    let factory = c.factoryNamed::<u32>("alias".into());
    assert!(matches!(
        factory(),
        Err(BindingResolutionError::NotBound { .. })
    ));
    c.bindNamed::<u32, _>("first", |_| Ok(Arc::new(1))).unwrap();
    c.bindNamed::<u32, _>("second", |_| Ok(Arc::new(2)))
        .unwrap();
    c.alias("first", "alias").unwrap();
    assert_eq!(*factory().unwrap(), 1);
    c.alias("second", "alias").unwrap();
    assert_eq!(*factory().unwrap(), 2);
    c.instanceNamed("second", Arc::new(3u32)).unwrap();
    assert_eq!(*factory().unwrap(), 3);
    c.forgetInstance("second");
    assert_eq!(*factory().unwrap(), 2);
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    c.bind::<u32, _>(move |_| Ok(Arc::new(seen.fetch_add(1, Ordering::SeqCst) as u32)))
        .unwrap();
    let factory = c.factory::<u32>();
    assert_eq!(n.load(Ordering::SeqCst), 0);
    assert_eq!((*factory().unwrap(), *factory().unwrap()), (0, 1));
}
#[tokio::test]
async fn factory_uses_invocation_scope_and_same_shared_cache_as_make() {
    let c = Container::default();
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    c.scoped::<u32, _>(move |_| Ok(Arc::new(seen.fetch_add(1, Ordering::SeqCst) as u32)))
        .unwrap();
    let factory = c.factoryAsync::<u32>();
    let root = factory().await.unwrap();
    c.scope(async {
        let a = factory().await.unwrap();
        let b = factory().await.unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&root, &a));
    })
    .await;
    assert!(Arc::ptr_eq(&root, &factory().await.unwrap()));
    assert_eq!(n.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn async_factory_is_lazy_retries_errors_and_sync_factory_requires_async_resolution() {
    let c = Container::default();
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    c.singletonAsyncNamed::<u32, _>("number", move |_| {
        let n = seen.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            if n.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(BindingResolutionError::FactoryFailed {
                    service: "number",
                    source: std::io::Error::other("private").into(),
                })
            } else {
                Ok(Arc::new(7))
            }
        })
    })
    .await
    .unwrap();
    let factory = c.factoryAsyncNamed::<u32>("number".into());
    assert_eq!(n.load(Ordering::SeqCst), 0);
    assert!(c.factoryNamed::<u32>("number".into())().is_err());
    assert!(factory().await.is_err());
    let ready = factory().await.unwrap();
    assert_eq!(*ready, 7);
    assert!(Arc::ptr_eq(&ready, &factory().await.unwrap()));
    c.instanceNamed("number", Arc::new(9u32)).unwrap();
    assert_eq!(*factory().await.unwrap(), 9);
    assert_eq!(n.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn wrap_retains_parameter_snapshot_but_resolves_live_dependencies_each_time() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let signature = [
        ConstructorParam::class::<u32>("dep"),
        ConstructorParam::scalar::<u32>("number"),
    ];
    let mut params = ParameterOverrides::new();
    params.insert(ParameterKey::name("number"), ParameterValue::new(5u32));
    let wrapped = c.wrap(
        Callback::reusable(&signature, move |args| {
            let calls = seen.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok((
                    *args.resolved.service::<u32>(0)?,
                    *args.resolved.get::<u32>(1)?,
                ))
            }
        }),
        params.clone(),
    );
    params.insert(ParameterKey::name("number"), ParameterValue::new(8u32));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(wrapped().await.is_err());
    c.instance(Arc::new(1u32)).unwrap();
    assert_eq!(wrapped().await.unwrap(), (1, 5));
    c.instance(Arc::new(2u32)).unwrap();
    assert_eq!(wrapped().await.unwrap(), (2, 5));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn repeated_wrap_shares_callback_state_and_fresh_defaults_without_requiring_clone() {
    struct NotClone(AtomicUsize);
    let state = NotClone(AtomicUsize::new(0));
    let n = Arc::new(AtomicUsize::new(0));
    let defaults = n.clone();
    let c = Container::default();
    let signature = [ConstructorParam::scalar::<usize>("number")
        .withDefault(move || Ok(ParameterValue::new(defaults.fetch_add(1, Ordering::SeqCst))))];
    let wrapped = c.wrap(
        Callback::reusable(&signature, move |args| {
            let count = state.0.fetch_add(1, Ordering::SeqCst);
            async move {
                tokio::task::yield_now().await;
                Ok((count, *args.resolved.get::<usize>(0)?))
            }
        }),
        ParameterOverrides::new(),
    );
    let (a, b) = tokio::join!(wrapped(), wrapped());
    assert_eq!((a.unwrap(), b.unwrap()), ((0, 0), (1, 1)));
    assert_eq!(n.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn wrap_borrows_external_data_and_respects_constructor_context_and_empty_parameter_mask() {
    struct Consumer(u32);
    let c = Container::default();
    let text = String::from("borrowed");
    let wrapped = c.wrap(
        Callback::reusable(&[], |_| async { Ok(text.as_str()) }),
        ParameterOverrides::new(),
    );
    assert_eq!(wrapped().await.unwrap(), "borrowed");
    c.when::<Consumer>()
        .unwrap()
        .needs::<u32>()
        .give(|_| Ok(Arc::new(42)))
        .unwrap();
    c.bindAsync::<Consumer, _>(|c| {
        Box::pin(async move {
            let signature = [ConstructorParam::class::<u32>("dep")];
            let wrapped = c.wrap(
                Callback::reusable(&signature, |args| async move {
                    Ok(*args.resolved.service::<u32>(0)?)
                }),
                ParameterOverrides::new(),
            );
            tokio::task::yield_now().await;
            Ok(Arc::new(Consumer(wrapped().await?)))
        })
    })
    .await
    .unwrap();
    assert_eq!(c.makeAsync::<Consumer>().await.unwrap().0, 42);
    assert!(c.factory::<u32>()().is_err());
}
#[tokio::test]
async fn cancelling_wrapped_future_preserves_other_calls_and_releases_own_arguments() {
    use std::task::Poll;
    struct Held(Arc<AtomicUsize>);
    impl Drop for Held {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let n = drops.clone();
    let c = Container::default();
    let signature = [ConstructorParam::scalar::<Held>("owned")
        .withDefault(move || Ok(ParameterValue::new(Held(n.clone()))))];
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let wrapped = c.wrap(
        Callback::reusable(&signature, move |args| {
            let attempt = seen.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    std::future::pending::<()>().await;
                }
                drop(args);
                Ok(())
            }
        }),
        ParameterOverrides::new(),
    );
    let mut pending = wrapped();
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(pending);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    wrapped().await.unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn wrapped_and_factory_futures_polled_in_closed_scope_are_rejected() {
    use std::task::Poll;
    let c = Container::default();
    c.bindAsync::<u32, _>(|_| Box::pin(std::future::pending()))
        .await
        .unwrap();
    let factory = c.factoryAsync::<u32>();
    let signature = [ConstructorParam::class::<u32>("number")];
    let wrapped = c.wrap(
        Callback::reusable(&signature, |_| async { Ok(()) }),
        ParameterOverrides::new(),
    );
    let escaped = std::sync::Mutex::new(None);
    c.scope(async {
        let mut a = factory();
        let mut b = wrapped();
        std::future::poll_fn(|cx| {
            assert!(a.as_mut().poll(cx).is_pending());
            assert!(b.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        *escaped.lock().unwrap() = Some((a, b));
    })
    .await;
    let (a, b) = escaped.lock().unwrap().take().unwrap();
    assert!(matches!(a.await, Err(BindingResolutionError::ScopeClosed)));
    assert!(matches!(b.await, Err(BindingResolutionError::ScopeClosed)));
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
}
