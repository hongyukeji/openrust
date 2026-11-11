use openrust::{
    container::{
        ClassDescriptor, Container, MethodCall, ParameterKey, ParameterOverrides, ParameterValue,
        ServiceKey,
    },
    contracts::container::{BindingResolutionError, Container as _},
};
use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};
fn key<T: ?Sized + 'static>() -> ServiceKey {
    ServiceKey::typed::<T>()
}
#[test]
fn snapshots_preserve_insertion_order_original_shared_flags_and_exclude_other_directories() {
    struct Automatic;
    let c = Container::default();
    c.bindNamed::<u32, _>("first", |_| Ok(Arc::new(1))).unwrap();
    c.scopedNamed::<u32, _>("second", |_| Ok(Arc::new(2)))
        .unwrap();
    c.bindIfNamed::<u32, _>("first", |_| panic!("conditional must not replace"))
        .unwrap();
    c.bindNamed::<u32, _>("first", |_| Ok(Arc::new(3))).unwrap();
    c.instanceNamed("first", Arc::new(9u32)).unwrap();
    c.instanceNamed("instance-only", Arc::new(9u32)).unwrap();
    c.alias("second", "alias").unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Automatic, _>(vec![], |_| Ok(Arc::new(Automatic))).unwrap(),
    )
    .unwrap();
    let mut snapshot = c.getBindings();
    assert_eq!(
        snapshot.keys().map(ServiceKey::name).collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(snapshot.len(), 2);
    assert!(!snapshot.is_empty());
    assert!(!snapshot.get(&"first".into()).unwrap().shared);
    assert!(snapshot.get(&"second".into()).unwrap().shared);
    assert!(snapshot.get(&"alias".into()).is_none());
    assert!(snapshot.get(&key::<Automatic>()).is_none());
    snapshot.get_mut(&"first".into()).unwrap().shared = true;
    assert!(!c.getBindings().get(&"first".into()).unwrap().shared);
    assert_eq!(snapshot.iter().count(), 2);
    assert_eq!(snapshot.into_iter().count(), 2);
}
#[test]
fn raw_factory_bypasses_target_cache_extenders_events_and_history_and_keeps_source_identity() {
    let c = Container::default();
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    c.singleton::<u32, _>(move |_| Ok(Arc::new(seen.fetch_add(1, Ordering::SeqCst) as u32 + 1)))
        .unwrap();
    c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 100)))
        .unwrap();
    let events = Arc::new(AtomicUsize::new(0));
    let seen = events.clone();
    c.beforeResolving(Some(key::<u32>()), move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    let raw = c.getBindings().get(&key::<u32>()).unwrap().concrete.clone();
    for number in [1, 2] {
        assert_eq!(
            *raw.call::<u32>(&c, ParameterOverrides::new()).unwrap(),
            number
        );
        assert!(!c.resolved(key::<u32>()).unwrap());
    }
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert_eq!(*c.make::<u32>().unwrap(), 103);
    c.instance(Arc::new(99u32)).unwrap();
    assert!(raw.sameFactory(&c.getBindings().get(&key::<u32>()).unwrap().concrete));
    assert_eq!(*raw.call::<u32>(&c, ParameterOverrides::new()).unwrap(), 4);
    assert_eq!(*c.make::<u32>().unwrap(), 99);
    c.forgetInstance(key::<u32>());
    assert!(raw.sameFactory(&c.getBindings().get(&key::<u32>()).unwrap().concrete));
    c.bind::<u32, _>(|_| Ok(Arc::new(40))).unwrap();
    assert!(!raw.sameFactory(&c.getBindings().get(&key::<u32>()).unwrap().concrete));
    assert_eq!(*raw.call::<u32>(&c, ParameterOverrides::new()).unwrap(), 5);
    assert_eq!(*c.make::<u32>().unwrap(), 140);
    assert!(matches!(
        raw.call::<String>(&c, ParameterOverrides::new()),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    assert_eq!(n.load(Ordering::SeqCst), 5);
}
#[test]
fn snapshot_does_not_retain_a_warmed_singleton_instance_and_remains_callable_after_flush() {
    struct Held;
    let c = Container::default();
    c.singleton::<Held, _>(|_| Ok(Arc::new(Held))).unwrap();
    let value = c.make::<Held>().unwrap();
    let weak = Arc::downgrade(&value);
    drop(value);
    let raw = c
        .getBindings()
        .get(&key::<Held>())
        .unwrap()
        .concrete
        .clone();
    c.flush();
    assert!(weak.upgrade().is_none());
    assert!(c.getBindings().is_empty());
    raw.call::<Held>(&c, ParameterOverrides::new()).unwrap();
    assert!(!c.bound::<Held>());
}
#[tokio::test]
async fn original_factory_uses_supplied_container_and_parameters_without_inventing_consumer_context()
 {
    struct Dep(&'static str);
    let a = Container::default();
    let b = Container::default();
    a.bind::<String, _>(|c| {
        let parameters = c.getLastParameterOverride();
        let input = parameters
            .get(&ParameterKey::name("label"))
            .unwrap()
            .downcast::<String>()?;
        assert!(parameters.get(&ParameterKey::Index(0)).unwrap().is_null());
        Ok(Arc::new(format!("{input}:{}", c.make::<Dep>()?.0)))
    })
    .unwrap();
    b.bind::<Dep, _>(|c| {
        assert!(c.getLastParameterOverride().is_empty());
        Ok(Arc::new(Dep("b")))
    })
    .unwrap();
    b.when::<String>()
        .unwrap()
        .needs::<Dep>()
        .give(|_| panic!("raw target must not add a class frame"))
        .unwrap();
    let raw = a
        .getBindings()
        .get(&key::<String>())
        .unwrap()
        .concrete
        .clone();
    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        ParameterKey::name("label"),
        ParameterValue::new("input".to_owned()),
    );
    parameters.insert(ParameterKey::Index(0), ParameterValue::null());
    assert_eq!(
        &*raw.callAsync::<String>(&b, parameters).await.unwrap(),
        "input:b"
    );
    assert!(!a.resolved(key::<String>()).unwrap());
    assert!(!b.resolved(key::<String>()).unwrap());
    assert!(b.resolved(key::<Dep>()).unwrap());
    assert!(b.getLastParameterOverride().is_empty());
}
#[tokio::test]
async fn async_raw_calls_do_not_cache_even_when_source_is_shared_and_wrong_types_never_execute() {
    let c = Container::default();
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    c.singletonAsync::<u32, _>(move |_| {
        let n = seen.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(Arc::new(n.fetch_add(1, Ordering::SeqCst) as u32))
        })
    })
    .await
    .unwrap();
    let raw = c.getBindings().get(&key::<u32>()).unwrap().concrete.clone();
    assert!(matches!(
        raw.call::<u32>(&c, ParameterOverrides::new()),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    assert!(
        raw.callAsync::<String>(&c, ParameterOverrides::new())
            .await
            .is_err()
    );
    assert_eq!(n.load(Ordering::SeqCst), 0);
    let (a, b) = tokio::join!(
        raw.callAsync::<u32>(&c, ParameterOverrides::new()),
        raw.callAsync::<u32>(&c, ParameterOverrides::new())
    );
    let mut values = [*a.unwrap(), *b.unwrap()];
    values.sort();
    assert_eq!(values, [0, 1]);
    assert!(!c.resolved(key::<u32>()).unwrap());
    assert_eq!(*c.makeAsync::<u32>().await.unwrap(), 2);
    assert!(raw.call::<u32>(&c, ParameterOverrides::new()).is_err());
}
#[tokio::test]
async fn flush_retains_contextual_tags_extenders_hooks_method_bindings_and_compiled_metadata() {
    struct Automatic;
    struct Consumer;
    let c = Container::default();
    let events = Arc::new(AtomicUsize::new(0));
    let seen = events.clone();
    c.beforeResolving(Some(key::<u32>()), move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 10)))
        .unwrap();
    c.when::<Consumer>()
        .unwrap()
        .needs::<u32>()
        .give(|_| Ok(Arc::new(7)))
        .unwrap();
    c.tag([key::<u32>()], ["numbers"]);
    c.registerClass(
        ClassDescriptor::concrete::<Automatic, _>(vec![], |_| Ok(Arc::new(Automatic))).unwrap(),
    )
    .unwrap();
    c.registerMethod::<Consumer, u32, _>("run", vec![], |_, _, c| {
        Box::pin(async move { Ok(*c.makeAsync::<u32>().await?) })
    })
    .unwrap();
    c.bindMethod::<Consumer, u32, _>("bound", |_, _| Box::pin(async { Ok(99) }))
        .unwrap();
    c.bind::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    c.alias(key::<u32>(), "alias").unwrap();
    c.make::<u32>().unwrap();
    c.flush();
    assert!(!c.has("alias"));
    assert!(!c.isAlias("alias"));
    assert!(!c.bound::<u32>());
    assert!(!c.resolved(key::<u32>()).unwrap());
    c.make::<Automatic>().unwrap();
    c.bind::<u32, _>(|_| Ok(Arc::new(2))).unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 12);
    assert_eq!(
        *c.tagged::<u32>("numbers")
            .into_iter()
            .next()
            .unwrap()
            .unwrap(),
        12
    );
    assert_eq!(
        c.call::<u32, _>(
            MethodCall::on(Arc::new(Consumer), "run"),
            ParameterOverrides::new()
        )
        .await
        .unwrap(),
        17
    );
    assert_eq!(
        c.callMethodBinding::<Consumer, u32>("bound", Arc::new(Consumer))
            .await
            .unwrap(),
        99
    );
    assert_eq!(events.load(Ordering::SeqCst), 4);
}
#[test]
fn flush_drops_user_values_after_committing_and_outside_registry_lock() {
    struct Reenter(Weak<Container>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            let c = self.0.upgrade().unwrap();
            assert!(c.getBindings().is_empty());
            c.instance(Arc::new(true)).unwrap();
        }
    }
    let c = Arc::new(Container::default());
    c.instance(Arc::new(Reenter(Arc::downgrade(&c)))).unwrap();
    c.flush();
    assert!(*c.make::<bool>().unwrap());
}
#[test]
fn destructor_panic_does_not_roll_back_flush_or_poison_the_registry() {
    struct Panics;
    impl Drop for Panics {
        fn drop(&mut self) {
            panic!("test destructor panic");
        }
    }
    let c = Container::default();
    c.instance(Arc::new(Panics)).unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.flush())).is_err());
    assert!(!c.bound::<Panics>());
    c.instance(Arc::new(5u32)).unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 5);
}
#[tokio::test]
async fn flush_detaches_every_active_scope_cache_without_closing_their_lifecycles() {
    let c = Container::default();
    c.scoped::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    c.alias(key::<u32>(), "alias").unwrap();
    let first = Mutex::new(None);
    let second = Mutex::new(None);
    async fn parked(c: &Container, save: &Mutex<Option<Weak<u32>>>, gate: &tokio::sync::Notify) {
        c.scope(async {
            c.make::<u32>().unwrap();
            let value = c.instance(Arc::new(9u32)).unwrap();
            *save.lock().unwrap() = Some(Arc::downgrade(&value));
            drop(value);
            gate.notified().await;
            assert!(!c.isAlias("alias"));
            assert_eq!(*c.make::<u32>().unwrap(), 2);
        })
        .await;
    }
    let gate = tokio::sync::Notify::new();
    let mut a = Box::pin(parked(&c, &first, &gate));
    let mut b = Box::pin(parked(&c, &second, &gate));
    std::future::poll_fn(|cx| {
        assert!(a.as_mut().poll(cx).is_pending());
        assert!(b.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    c.flush();
    assert!(first.lock().unwrap().as_ref().unwrap().upgrade().is_none());
    assert!(second.lock().unwrap().as_ref().unwrap().upgrade().is_none());
    assert!(!c.has("alias"));
    c.bind::<u32, _>(|_| Ok(Arc::new(2))).unwrap();
    // Both scopes stay open and a fresh non-scoped binding has no stale request override.
    let raw = c.getBindings().get(&key::<u32>()).unwrap().concrete.clone();
    assert_eq!(*raw.call::<u32>(&c, ParameterOverrides::new()).unwrap(), 2);
    gate.notify_waiters();
    tokio::join!(a, b);
}
#[tokio::test]
async fn in_flight_singleton_cannot_repopulate_directory_or_history_after_flush() {
    let c = Container::default();
    let gate = Arc::new(tokio::sync::Notify::new());
    let wait = gate.clone();
    c.singletonAsync::<u32, _>(move |_| {
        let gate = wait.clone();
        Box::pin(async move {
            gate.notified().await;
            Ok(Arc::new(1))
        })
    })
    .await
    .unwrap();
    let mut old = Box::pin(c.makeAsync::<u32>());
    std::future::poll_fn(|cx| {
        assert!(old.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    c.flush();
    c.singleton::<u32, _>(|_| Ok(Arc::new(2))).unwrap();
    gate.notify_one();
    assert_eq!(*old.await.unwrap(), 1);
    assert!(!c.resolved(key::<u32>()).unwrap());
    assert_eq!(*c.make::<u32>().unwrap(), 2);
}
#[tokio::test]
async fn raw_calls_survive_binding_replacement_but_refuse_closed_scopes() {
    let c = Container::default();
    let gate = Arc::new(tokio::sync::Notify::new());
    let wait = gate.clone();
    c.bindAsync::<u32, _>(move |_| {
        let gate = wait.clone();
        Box::pin(async move {
            gate.notified().await;
            Ok(Arc::new(1))
        })
    })
    .await
    .unwrap();
    let raw = c.getBindings().get(&key::<u32>()).unwrap().concrete.clone();
    let mut old = Box::pin(raw.callAsync::<u32>(&c, ParameterOverrides::new()));
    std::future::poll_fn(|cx| {
        assert!(old.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    c.flush();
    c.bind::<u32, _>(|_| Ok(Arc::new(2))).unwrap();
    gate.notify_one();
    assert_eq!(*old.await.unwrap(), 1);
    let escaped = Mutex::new(None);
    c.scope(async {
        let mut future = Box::pin(raw.callAsync::<u32>(&c, ParameterOverrides::new()));
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        *escaped.lock().unwrap() = Some(future);
    })
    .await;
    let escaped = escaped.lock().unwrap().take().unwrap();
    assert!(matches!(
        escaped.await,
        Err(BindingResolutionError::ScopeClosed)
    ));
}

type CleanupObservations = Arc<Mutex<Vec<(bool, &'static str)>>>;
struct CleanupDep(&'static str);
struct CleanupTrigger;
struct CleanupHeld {
    container: Weak<Container>,
    seen: CleanupObservations,
}
impl Drop for CleanupHeld {
    fn drop(&mut self) {
        let c = self.container.upgrade().unwrap();
        self.seen.lock().unwrap().push((
            c.getLastParameterOverride().is_empty(),
            c.make::<CleanupDep>().unwrap().0,
        ));
    }
}
fn cleanup_fixture() -> (Arc<Container>, CleanupObservations) {
    let c = Arc::new(Container::default());
    let seen = Arc::new(Mutex::new(vec![]));
    c.registerClass(
        ClassDescriptor::concrete::<CleanupDep, _>(vec![], |_| Ok(Arc::new(CleanupDep("root"))))
            .unwrap(),
    )
    .unwrap();
    c.when::<CleanupTrigger>()
        .unwrap()
        .needs::<CleanupDep>()
        .give(|_| Ok(Arc::new(CleanupDep("caller"))))
        .unwrap();
    let owner = Arc::downgrade(&c);
    let observed = seen.clone();
    c.scoped::<CleanupHeld, _>(move |_| {
        Ok(Arc::new(CleanupHeld {
            container: owner.clone(),
            seen: observed.clone(),
        }))
    })
    .unwrap();
    (c, seen)
}
fn cleanup_parameters() -> ParameterOverrides {
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("caller"), ParameterValue::new(7u32));
    parameters
}
#[tokio::test]
async fn flush_retires_scoped_values_without_the_callers_parameters_or_class_context() {
    let (c, seen) = cleanup_fixture();
    c.bind::<CleanupTrigger, _>(|c| {
        assert!(!c.getLastParameterOverride().is_empty());
        c.flush();
        Ok(Arc::new(CleanupTrigger))
    })
    .unwrap();
    c.scope(async {
        c.make::<CleanupHeld>().unwrap();
        c.makeWith::<CleanupTrigger>(cleanup_parameters()).unwrap();
    })
    .await;
    assert_eq!(*seen.lock().unwrap(), [(true, "root")]);
}
#[tokio::test]
async fn normal_scope_end_also_retires_cache_without_an_outer_constructors_inputs() {
    let (c, seen) = cleanup_fixture();
    c.bindAsync::<CleanupTrigger, _>(|c| {
        Box::pin(async move {
            c.scope(async {
                c.make::<CleanupHeld>().unwrap();
            })
            .await;
            assert!(!c.getLastParameterOverride().is_empty());
            Ok(Arc::new(CleanupTrigger))
        })
    })
    .await
    .unwrap();
    c.makeWithAsync::<CleanupTrigger>(cleanup_parameters())
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), [(true, "root")]);
}
