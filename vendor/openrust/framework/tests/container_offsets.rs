use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

struct OnRetire(Arc<Container>, Arc<AtomicBool>);
impl Drop for OnRetire {
    fn drop(&mut self) {
        assert!(Arc::ptr_eq(&Container::getInstance(), &self.0));
        self.1.store(true, Ordering::Release);
    }
}

#[test]
fn process_instance_is_lazy_shared_and_replaced_before_retiring_services() {
    assert!(Container::setInstance(None).is_none());
    let barrier = Arc::new(Barrier::new(2));
    let worker = std::thread::spawn({
        let barrier = barrier.clone();
        move || {
            barrier.wait();
            Container::getInstance()
        }
    });
    barrier.wait();
    let original = Container::getInstance();
    assert!(Arc::ptr_eq(&original, &worker.join().unwrap()));
    let replacement = Arc::new(Container::default());
    let retired = Arc::new(AtomicBool::new(false));
    original
        .instance(Arc::new(OnRetire(replacement.clone(), retired.clone())))
        .unwrap();
    drop(original);
    assert!(Arc::ptr_eq(
        &replacement,
        &Container::setInstance(Some(replacement.clone())).unwrap()
    ));
    assert!(retired.load(Ordering::Acquire));
    assert!(Container::setInstance(None).is_none());
    assert!(!Arc::ptr_eq(&replacement, &Container::getInstance()));
}

#[test]
fn offsets_resolve_values_and_factories_and_unset_only_the_raw_key() {
    let container = Container::default();
    let value: Arc<str> = Arc::from("hello");
    assert!(!container.offsetExists("message"));
    container.offsetSet("message", value.clone()).unwrap();
    assert!(!container.isShared("message"));
    container.alias("message", "alias").unwrap();
    assert!(container.offsetExists("alias"));
    assert!(Arc::ptr_eq(
        &container.offsetGet::<str>("alias").unwrap(),
        &value
    ));
    container.offsetUnset("alias");
    assert!(container.isAlias("alias"));
    assert!(container.resolved("message").unwrap());

    container.offsetUnset("message");
    assert!(!container.offsetExists("message"));
    assert!(!container.resolved("message").unwrap());
    assert!(container.getBindings().is_empty());
    assert!(container.offsetExists("alias"));
    assert!(matches!(
        container.offsetGet::<str>("alias"),
        Err(BindingResolutionError::NotBound { .. })
    ));

    let key = ServiceKey::named("message");
    let count = AtomicUsize::new(0);
    container
        .offsetSetFactory(key, move |_| {
            Ok(Arc::new(count.fetch_add(1, Ordering::Relaxed)))
        })
        .unwrap();
    assert_eq!(*container.offsetGet::<usize>("alias").unwrap(), 0);
    assert_eq!(*container.offsetGet::<usize>("alias").unwrap(), 1);

    container
        .instanceNamed("message", Arc::new(12_usize))
        .unwrap();
    assert_eq!(*container.offsetGet::<usize>("message").unwrap(), 12);
    container.offsetUnset("message");
    assert!(!container.offsetExists("message"));
    assert!(!container.resolved("message").unwrap());
}

#[tokio::test]
async fn unset_evicts_other_live_scopes_and_prevents_in_flight_history() {
    let container = Container::default();
    let ready = Notify::new();
    let removed = Notify::new();
    container
        .scopedNamed("scoped", |_| Ok(Arc::new(1_u32)))
        .unwrap();
    container.offsetSet("keep", Arc::new(9_u32)).unwrap();
    let use_scope = container.scope(async {
        assert_eq!(*container.offsetGet::<u32>("scoped").unwrap(), 1);
        container.instanceNamed("scoped", Arc::new(2_u32)).unwrap();
        ready.notify_one();
        removed.notified().await;
        assert!(!container.offsetExists("scoped"));
        assert!(!container.resolved("scoped").unwrap());
        assert_eq!(*container.offsetGet::<u32>("keep").unwrap(), 9);
    });
    let erase = async {
        ready.notified().await;
        container.offsetUnset("scoped");
        removed.notify_one();
    };
    tokio::join!(use_scope, erase);

    let ready = Arc::new(Notify::new());
    let removed = Arc::new(Notify::new());
    container
        .offsetSetFactoryAsync("async", {
            let ready = ready.clone();
            let removed = removed.clone();
            move |_| {
                let ready = ready.clone();
                let removed = removed.clone();
                Box::pin(async move {
                    ready.notify_one();
                    removed.notified().await;
                    Ok(Arc::new(4_u32))
                })
            }
        })
        .await
        .unwrap();
    let (value, ()) = tokio::join!(container.offsetGetAsync::<u32>("async"), async {
        ready.notified().await;
        container.offsetUnset("async");
        removed.notify_one();
    });
    assert_eq!(*value.unwrap(), 4);
    assert!(!container.offsetExists("async"));
    assert!(!container.resolved("async").unwrap());
}

#[test]
fn unset_in_a_resolution_callback_does_not_restore_history() {
    let container = Container::default();
    container.offsetSet("one", Arc::new(1_u32)).unwrap();
    container
        .resolving(Some(ServiceKey::named("one")), |_, c| {
            c.offsetUnset("one");
            Ok(())
        })
        .unwrap();
    assert_eq!(*container.offsetGet::<u32>("one").unwrap(), 1);
    assert!(!container.resolved("one").unwrap());
}
