use openrust::{
    container::{Container, ParameterKey, ParameterOverrides, ParameterValue, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn parameters(value: u32) -> ParameterOverrides {
    let mut result = ParameterOverrides::new();
    result.insert(ParameterKey::name("number"), ParameterValue::new(value));
    result
}
fn number(c: &Container) -> Result<u32, BindingResolutionError> {
    c.getLastParameterOverride()
        .get(&ParameterKey::name("number"))
        .map(|value| value.downcast::<u32>().map(|value| *value))
        .unwrap_or(Ok(0))
}
#[test]
fn values_are_typed_redacted_ordered_and_copy_on_write() {
    let mut a = parameters(3);
    a.insert(
        ParameterKey::Index(0),
        ParameterValue::new("private secret"),
    );
    a.insert(ParameterKey::name("0"), ParameterValue::null());
    let b = a.clone();
    assert_eq!(
        *a.insert(ParameterKey::name("number"), ParameterValue::new(4u32))
            .unwrap()
            .downcast::<u32>()
            .unwrap(),
        3
    );
    assert_eq!(
        *b.get(&ParameterKey::name("number"))
            .unwrap()
            .downcast::<u32>()
            .unwrap(),
        3
    );
    assert_eq!(a.len(), 3);
    assert_eq!(a.iter().next().unwrap().0, &ParameterKey::name("number"));
    assert!(!format!("{a:?}").contains("private secret"));
    assert!(matches!(
        a.get(&ParameterKey::name("0")).unwrap().downcast::<u32>(),
        Err(BindingResolutionError::ParameterTypeMismatch { actual: None, .. })
    ));
    assert!(matches!(
        a.get(&ParameterKey::Index(0)).unwrap().downcast::<u32>(),
        Err(BindingResolutionError::ParameterTypeMismatch {
            actual: Some(_),
            ..
        })
    ));
    let value = Arc::new(Mutex::new(5));
    let shared = ParameterValue::from_arc(value.clone());
    assert!(Arc::ptr_eq(&value, &shared.downcast().unwrap()));
}
#[test]
fn nonempty_parameters_bypass_cold_and_hot_shared_cache_and_history() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let n = calls.clone();
    c.singleton::<u32, _>(move |c| {
        n.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(number(c)?))
    })
    .unwrap();
    assert_eq!(*c.makeWith::<u32>(parameters(7)).unwrap(), 7);
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
    let original = c.make::<u32>().unwrap();
    assert_eq!(*original, 0);
    assert_eq!(*c.makeWith::<u32>(parameters(9)).unwrap(), 9);
    assert!(Arc::ptr_eq(
        &original,
        &c.makeWith::<u32>(ParameterOverrides::new()).unwrap()
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
#[test]
fn supplied_instances_are_bypassed_without_removing_their_registered_factory() {
    let c = Container::default();
    c.singletonNamed::<u32, _>("base", |c| Ok(Arc::new(number(c)?)))
        .unwrap();
    c.alias("base", "alias").unwrap();
    let provided = c.instanceNamed("base", Arc::new(99u32)).unwrap();
    assert_eq!(*c.makeWithNamed::<u32>("alias", parameters(8)).unwrap(), 8);
    assert!(Arc::ptr_eq(&provided, &c.get::<u32>("alias").unwrap()));
    c.instance(Arc::new(1u16)).unwrap();
    assert!(matches!(
        c.makeWith::<u16>(parameters(1)),
        Err(BindingResolutionError::NotBound { .. })
    ));
    assert_eq!(*c.make::<u16>().unwrap(), 1);
}
#[test]
fn nested_lookups_mask_parameters_and_explicit_forwarding_remains_available() {
    let c = Container::default();
    c.bind::<u16, _>(|c| Ok(Arc::new(number(c)? as u16)))
        .unwrap();
    c.bind::<u32, _>(|c| {
        assert_eq!(*c.make::<u16>()?, 0);
        assert_eq!(*c.makeWith::<u16>(c.getLastParameterOverride())?, 6);
        Ok(Arc::new(number(c)?))
    })
    .unwrap();
    assert_eq!(*c.makeWith::<u32>(parameters(6)).unwrap(), 6);
    assert!(c.getLastParameterOverride().is_empty());
}
#[test]
fn before_callbacks_receive_overrides_and_post_callbacks_keep_the_factory_frame() {
    let c = Container::default();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    c.beforeResolving(Some(ServiceKey::typed::<u32>()), move |event, _| {
        log.lock().unwrap().push(
            event
                .parameters()
                .get(&ParameterKey::name("number"))
                .unwrap()
                .downcast::<u32>()
                .map(|n| *n)?,
        );
        Ok(())
    })
    .unwrap();
    c.bind::<u32, _>(|c| Ok(Arc::new(number(c)?))).unwrap();
    let log = seen.clone();
    c.resolving(Some(ServiceKey::typed::<u32>()), move |_, c| {
        log.lock().unwrap().push(number(c)?);
        Ok(())
    })
    .unwrap();
    assert_eq!(*c.makeWith::<u32>(parameters(4)).unwrap(), 4);
    assert_eq!(*seen.lock().unwrap(), [4, 4]);
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
}
#[test]
fn parameter_frames_are_restored_after_errors_panics_and_other_containers() {
    let c = Container::default();
    let other = Arc::new(Container::default());
    let b = other.clone();
    c.bind::<u32, _>(move |c| {
        assert!(b.getLastParameterOverride().is_empty());
        if number(c)? == 1 {
            panic!("construction panic");
        }
        Err(BindingResolutionError::NotBound {
            service: "failure".into(),
        })
    })
    .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || c.makeWith::<u32>(parameters(1))
        ))
        .is_err()
    );
    assert!(c.getLastParameterOverride().is_empty());
    assert!(c.makeWith::<u32>(parameters(2)).is_err());
    assert!(c.getLastParameterOverride().is_empty());
}
#[tokio::test]
async fn asynchronous_parameters_do_not_leak_across_joined_futures_or_nested_dependencies() {
    let c = Container::default();
    c.bindAsync::<u16, _>(|c| {
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(Arc::new(number(c)? as u16))
        })
    })
    .await
    .unwrap();
    c.singletonAsync::<u32, _>(|c| {
        Box::pin(async move {
            let before = number(c)?;
            tokio::task::yield_now().await;
            assert_eq!(number(c)?, before);
            assert_eq!(*c.makeAsync::<u16>().await?, 0);
            assert_eq!(
                *c.makeWithAsync::<u16>(c.getLastParameterOverride()).await?,
                before as u16
            );
            Ok(Arc::new(before))
        })
    })
    .await
    .unwrap();
    let (a, b, plain) = tokio::join!(
        c.makeWithAsync::<u32>(parameters(2)),
        c.makeWithAsync::<u32>(parameters(3)),
        c.makeAsync::<u16>()
    );
    assert_eq!((*a.unwrap(), *b.unwrap(), *plain.unwrap()), (2, 3, 0));
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert!(c.getLastParameterOverride().is_empty());
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 0);
}
#[tokio::test]
async fn scoped_overrides_never_replace_the_normal_lifecycle_cache() {
    let c = Container::default();
    c.scopedAsyncNamed::<u32, _>("scoped", |c| {
        Box::pin(async move { Ok(Arc::new(number(c)?)) })
    })
    .await
    .unwrap();
    c.scope(async {
        c.instanceNamed("scoped", Arc::new(99u32)).unwrap();
        assert_eq!(
            *c.makeWithAsyncNamed::<u32>("scoped", parameters(5))
                .await
                .unwrap(),
            5
        );
        assert_eq!(*c.getAsync::<u32>("scoped").await.unwrap(), 99);
    })
    .await;
    c.scope(async {
        assert_eq!(*c.getAsync::<u32>("scoped").await.unwrap(), 0);
    })
    .await;
}
#[tokio::test]
async fn cleanup_reads_its_own_parameters_and_resets_the_next_resolution() {
    struct Probe {
        container: Arc<Container>,
        seen: Arc<Mutex<Vec<u32>>>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            self.seen
                .lock()
                .unwrap()
                .push(number(&self.container).unwrap());
        }
    }
    let c = Arc::new(Container::default());
    let weak = Arc::downgrade(&c);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let entered = Arc::new(tokio::sync::Notify::new());
    let started = entered.clone();
    c.bindAsync::<u32, _>(move |_| {
        let c = weak.upgrade().unwrap();
        let seen = log.clone();
        let entered = started.clone();
        Box::pin(async move {
            let _probe = Probe { container: c, seen };
            entered.notify_one();
            std::future::pending().await
        })
    })
    .await
    .unwrap();
    let mut pending = Box::pin(c.makeWithAsync::<u32>(parameters(8)));
    tokio::select! {value=&mut pending=>panic!("unexpected {value:?}"),_=entered.notified()=>{}}
    drop(pending);
    assert_eq!(*seen.lock().unwrap(), [8]);
    assert!(c.getLastParameterOverride().is_empty());
}
#[test]
fn explicit_recursive_calls_can_use_different_parameters_but_same_snapshot_cycles_fail() {
    let c = Container::default();
    c.bind::<u32, _>(|c| {
        let n = number(c)?;
        if n > 0 {
            return c.makeWith::<u32>(parameters(n - 1));
        }
        Ok(Arc::new(0))
    })
    .unwrap();
    assert_eq!(*c.makeWith::<u32>(parameters(3)).unwrap(), 0);
    c.bind::<u16, _>(|c| c.makeWith::<u16>(c.getLastParameterOverride()))
        .unwrap();
    assert!(matches!(
        c.makeWith::<u16>(parameters(1)),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
}

#[tokio::test]
async fn an_ordinary_pending_lookup_stays_empty_when_resumed_from_a_parameterized_factory() {
    let c = Arc::new(Container::default());
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let (a, b) = (entered.clone(), release.clone());
    c.bindAsync::<u16, _>(move |c| {
        let (a, b) = (a.clone(), b.clone());
        Box::pin(async move {
            a.notify_one();
            b.notified().await;
            Ok(Arc::new(number(c)? as u16))
        })
    })
    .await
    .unwrap();
    let owned = c.clone();
    let mut pending: openrust::container::FactoryFuture<'static, u16> =
        Box::pin(async move { owned.makeAsync::<u16>().await });
    tokio::select! {value=&mut pending=>panic!("unexpected {value:?}"),_=entered.notified()=>{}}
    let saved = Arc::new(Mutex::new(Some(pending)));
    let stash = saved.clone();
    c.bindAsync::<u32, _>(move |c| {
        let saved = stash.clone();
        let release = release.clone();
        Box::pin(async move {
            let pending = saved.lock().unwrap().take().unwrap();
            release.notify_one();
            assert_eq!(*pending.await?, 0);
            Ok(Arc::new(number(c)?))
        })
    })
    .await
    .unwrap();
    assert_eq!(*c.makeWithAsync::<u32>(parameters(13)).await.unwrap(), 13);
    assert!(saved.lock().unwrap().is_none());
}
#[test]
fn contextual_recipes_and_decorators_apply_without_caching_explicit_constructions() {
    struct Consumer(u16);
    let c = Container::default();
    c.bind::<Consumer, _>(|c| {
        Ok(Arc::new(Consumer(
            *c.makeWith::<u16>(c.getLastParameterOverride())?,
        )))
    })
    .unwrap();
    c.when::<Consumer>()
        .unwrap()
        .needs::<u16>()
        .give(|c| Ok(Arc::new(number(c)? as u16 + 10)))
        .unwrap();
    c.extend::<u16, _>(ServiceKey::typed::<u16>(), |value, _| {
        Ok(Arc::new(*value + 1))
    })
    .unwrap();
    assert_eq!(c.makeWith::<Consumer>(parameters(5)).unwrap().0, 16);
    assert!(!c.resolved(ServiceKey::typed::<Consumer>()).unwrap());
    assert!(!c.resolved(ServiceKey::typed::<u16>()).unwrap());
}

#[tokio::test]
async fn retired_argument_payloads_do_not_inherit_an_outer_calls_parameters() {
    struct Payload(std::sync::Weak<Container>, Arc<Mutex<Vec<u32>>>);
    impl Drop for Payload {
        fn drop(&mut self) {
            self.1
                .lock()
                .unwrap()
                .push(number(&self.0.upgrade().unwrap()).unwrap());
        }
    }
    let c = Arc::new(Container::default());
    let weak = Arc::downgrade(&c);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    c.bind::<u16, _>(|_| Ok(Arc::new(0))).unwrap();
    c.bindAsync::<u32, _>(move |c| {
        let (weak, log) = (weak.clone(), log.clone());
        Box::pin(async move {
            for asynchronous in [false, true] {
                let mut args = ParameterOverrides::new();
                args.insert(
                    ParameterKey::name("payload"),
                    ParameterValue::new(Payload(weak.clone(), log.clone())),
                );
                if asynchronous {
                    c.makeWithAsync::<u16>(args).await?;
                } else {
                    c.makeWith::<u16>(args)?;
                }
            }
            Ok(Arc::new(number(c)?))
        })
    })
    .await
    .unwrap();
    assert_eq!(*c.makeWithAsync::<u32>(parameters(9)).await.unwrap(), 9);
    assert_eq!(*seen.lock().unwrap(), [0, 0]);
}
