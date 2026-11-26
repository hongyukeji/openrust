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
        assert!(Arc::ptr_eq(&Container::get_instance(), &self.0));
        self.1.store(true, Ordering::Release);
    }
}

#[test]
fn process_instance_is_lazy_shared_and_replaced_before_retiring_services() {
    assert!(Container::set_instance(None).is_none());
    let barrier = Arc::new(Barrier::new(2));
    let worker = std::thread::spawn({
        let barrier = barrier.clone();
        move || {
            barrier.wait();
            Container::get_instance()
        }
    });
    barrier.wait();
    let original = Container::get_instance();
    assert!(Arc::ptr_eq(&original, &worker.join().unwrap()));
    let replacement = Arc::new(Container::default());
    let retired = Arc::new(AtomicBool::new(false));
    original
        .instance(Arc::new(OnRetire(replacement.clone(), retired.clone())))
        .unwrap();
    drop(original);
    assert!(Arc::ptr_eq(
        &replacement,
        &Container::set_instance(Some(replacement.clone())).unwrap()
    ));
    assert!(retired.load(Ordering::Acquire));
    assert!(Container::set_instance(None).is_none());
    assert!(!Arc::ptr_eq(&replacement, &Container::get_instance()));
}

#[test]
fn offsets_resolve_values_and_factories_and_unset_only_the_raw_key() {
    let container = Container::default();
    let value: Arc<str> = Arc::from("hello");
    assert!(!container.offset_exists("message"));
    container.offset_set("message", value.clone()).unwrap();
    assert!(!container.is_shared("message"));
    container.alias("message", "alias").unwrap();
    assert!(container.offset_exists("alias"));
    assert!(Arc::ptr_eq(
        &container.offset_get::<str>("alias").unwrap(),
        &value
    ));
    container.offset_unset("alias");
    assert!(container.is_alias("alias"));
    assert!(container.resolved("message").unwrap());

    container.offset_unset("message");
    assert!(!container.offset_exists("message"));
    assert!(!container.resolved("message").unwrap());
    assert!(container.get_bindings().is_empty());
    assert!(container.offset_exists("alias"));
    assert!(matches!(
        container.offset_get::<str>("alias"),
        Err(BindingResolutionError::NotBound { .. })
    ));

    let key = ServiceKey::named("message");
    let count = AtomicUsize::new(0);
    container
        .offset_set_factory(key, move |_| {
            Ok(Arc::new(count.fetch_add(1, Ordering::Relaxed)))
        })
        .unwrap();
    assert_eq!(*container.offset_get::<usize>("alias").unwrap(), 0);
    assert_eq!(*container.offset_get::<usize>("alias").unwrap(), 1);

    container
        .instance_named("message", Arc::new(12_usize))
        .unwrap();
    assert_eq!(*container.offset_get::<usize>("message").unwrap(), 12);
    container.offset_unset("message");
    assert!(!container.offset_exists("message"));
    assert!(!container.resolved("message").unwrap());
}

#[tokio::test]
async fn unset_evicts_other_live_scopes_and_prevents_in_flight_history() {
    let container = Container::default();
    let ready = Notify::new();
    let removed = Notify::new();
    container
        .scoped_named("scoped", |_| Ok(Arc::new(1_u32)))
        .unwrap();
    container.offset_set("keep", Arc::new(9_u32)).unwrap();
    let use_scope = container.scope(async {
        assert_eq!(*container.offset_get::<u32>("scoped").unwrap(), 1);
        container.instance_named("scoped", Arc::new(2_u32)).unwrap();
        ready.notify_one();
        removed.notified().await;
        assert!(!container.offset_exists("scoped"));
        assert!(!container.resolved("scoped").unwrap());
        assert_eq!(*container.offset_get::<u32>("keep").unwrap(), 9);
    });
    let erase = async {
        ready.notified().await;
        container.offset_unset("scoped");
        removed.notify_one();
    };
    tokio::join!(use_scope, erase);

    let ready = Arc::new(Notify::new());
    let removed = Arc::new(Notify::new());
    container
        .offset_set_factory_async("async", {
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
    let (value, ()) = tokio::join!(container.offset_get_async::<u32>("async"), async {
        ready.notified().await;
        container.offset_unset("async");
        removed.notify_one();
    });
    assert_eq!(*value.unwrap(), 4);
    assert!(!container.offset_exists("async"));
    assert!(!container.resolved("async").unwrap());
}

#[test]
fn unset_in_a_resolution_callback_does_not_restore_history() {
    let container = Container::default();
    container.offset_set("one", Arc::new(1_u32)).unwrap();
    container
        .resolving(Some(ServiceKey::named("one")), |_, c| {
            c.offset_unset("one");
            Ok(())
        })
        .unwrap();
    assert_eq!(*container.offset_get::<u32>("one").unwrap(), 1);
    assert!(!container.resolved("one").unwrap());
}
