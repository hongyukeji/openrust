use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
async fn nested_scopes_share_instances_and_concurrent_lifecycles_release_them() {
    let container = Container::default();
    let calls = AtomicUsize::new(0);
    container
        .scoped::<usize, _>(move |_| Ok(Arc::new(calls.fetch_add(1, Ordering::SeqCst))))
        .unwrap();
    let outside = container.make::<usize>().unwrap();
    let run = || {
        container.scope(async {
            let first = container.make::<usize>().unwrap();
            tokio::task::yield_now().await;
            container
                .scope(async {
                    assert!(Arc::ptr_eq(&first, &container.make::<usize>().unwrap()));
                })
                .await;
            Arc::downgrade(&first)
        })
    };
    let (first, second) = tokio::join!(run(), run());
    assert!(!first.ptr_eq(&second));
    assert!(first.upgrade().is_none());
    assert!(second.upgrade().is_none());
    assert!(Arc::ptr_eq(&outside, &container.make::<usize>().unwrap()));
}

#[tokio::test]
async fn clearing_one_scope_keeps_other_requests_and_singletons_intact() {
    let container = Container::default();
    container.scoped::<usize, _>(|_| Ok(Arc::new(1))).unwrap();
    container.singleton::<u64, _>(|_| Ok(Arc::new(2))).unwrap();
    let singleton = container.make::<u64>().unwrap();
    let gate = tokio::sync::Barrier::new(2);
    let first = container.scope(async {
        let previous = container.make::<usize>().unwrap();
        gate.wait().await;
        container.forgetScopedInstances();
        let current = container.make::<usize>().unwrap();
        assert!(!Arc::ptr_eq(&previous, &current));
        assert!(Arc::ptr_eq(&singleton, &container.make::<u64>().unwrap()));
        gate.wait().await;
    });
    let second = container.scope(async {
        let instance = container.make::<usize>().unwrap();
        gate.wait().await;
        gate.wait().await;
        assert!(Arc::ptr_eq(&instance, &container.make::<usize>().unwrap()));
    });
    tokio::join!(first, second);
}

#[tokio::test]
async fn scoped_instance_overrides_and_removed_aliases_stay_in_the_current_lifecycle() {
    let container = Container::default();
    container
        .scopedNamed::<u32, _>("service", |_| Ok(Arc::new(7)))
        .unwrap();
    container.alias("service", "alias").unwrap();
    let gate = tokio::sync::Barrier::new(2);
    tokio::join!(
        container.scope(async {
            container.instanceNamed("alias", Arc::new(99_u32)).unwrap();
            assert_eq!(*container.get::<u32>("alias").unwrap(), 99);
            assert!(!container.isAlias("alias"));
            gate.wait().await;
            container.forgetInstance("alias");
            assert!(!container.has("alias"));
            assert!(container.get::<u32>("alias").is_err());
            container.forgetScopedInstances();
            assert!(!container.isAlias("alias"));
        }),
        container.scope(async {
            assert_eq!(*container.get::<u32>("service").unwrap(), 7);
            assert!(container.isAlias("alias"));
            gate.wait().await;
        })
    );
    assert!(container.isAlias("alias"));
    assert_eq!(*container.get::<u32>("alias").unwrap(), 7);
    container
        .scope(async {
            container.instanceNamed("alias", Arc::new(99_u32)).unwrap();
            container.forgetInstance("alias");
            container
                .scopedIfNamed::<u32, _>("alias", |_| Ok(Arc::new(23)))
                .unwrap();
            assert_eq!(*container.get::<u32>("alias").unwrap(), 23);
        })
        .await;
}

#[test]
fn scoped_if_does_not_register_existing_singletons_for_lifecycle_eviction() {
    let container = Container::default();
    container.singleton::<u64, _>(|_| Ok(Arc::new(5))).unwrap();
    let shared = container.make::<u64>().unwrap();
    container
        .scopedIf::<u64, _>(|_| panic!("existing singleton replaced"))
        .unwrap();
    container
        .scopedIfNamed::<u32, _>("fresh", |_| Ok(Arc::new(8)))
        .unwrap();
    let first = container.get::<u32>("fresh").unwrap();
    container.forgetScopedInstances();
    assert!(!Arc::ptr_eq(
        &first,
        &container.get::<u32>("fresh").unwrap()
    ));
    assert!(Arc::ptr_eq(&shared, &container.make::<u64>().unwrap()));
    assert!(container.isShared("fresh"));
}

#[tokio::test]
async fn rebinds_evict_local_overrides_and_nested_applications_restore_their_scopes() {
    let first = Container::default();
    let second = Container::default();
    first.scoped::<usize, _>(|_| Ok(Arc::new(1))).unwrap();
    second.scoped::<usize, _>(|_| Ok(Arc::new(2))).unwrap();
    first
        .scope(async {
            let original = first.make::<usize>().unwrap();
            second
                .scope(async {
                    assert!(Arc::ptr_eq(&original, &first.make::<usize>().unwrap()));
                    assert_eq!(*second.make::<usize>().unwrap(), 2);
                })
                .await;
            first.instance(Arc::new(99_usize)).unwrap();
            assert_eq!(*first.make::<usize>().unwrap(), 99);
            first.scoped::<usize, _>(|_| Ok(Arc::new(3))).unwrap();
            assert_eq!(*first.make::<usize>().unwrap(), 3);
            let key = ServiceKey::typed::<usize>();
            first.forgetInstance(key);
            assert_eq!(*first.make::<usize>().unwrap(), 3);
        })
        .await;
}

#[tokio::test]
async fn failed_constructors_retry_and_cancelled_futures_release_scoped_instances() {
    let container = Container::default();
    let attempts = AtomicUsize::new(0);
    container
        .scoped::<usize, _>(move |_| {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(BindingResolutionError::NotBound {
                    service: "probe".into(),
                });
            }
            Ok(Arc::new(11))
        })
        .unwrap();
    container
        .scope(async {
            assert!(container.make::<usize>().is_err());
            assert_eq!(*container.make::<usize>().unwrap(), 11);
        })
        .await;
    let mut weak = None;
    let mut future = Box::pin(container.scope(async {
        weak = Some(Arc::downgrade(&container.make::<usize>().unwrap()));
        std::future::pending::<()>().await;
    }));
    use std::future::Future as _;
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    drop(future);
    assert!(weak.unwrap().upgrade().is_none());
}

#[tokio::test]
async fn explicitly_supplied_instances_can_break_a_factory_cycle() {
    let container = Container::default();
    container
        .scoped::<usize, _>(|container| {
            container.instance(Arc::new(17_usize)).unwrap();
            container.make::<usize>()
        })
        .unwrap();
    container
        .scope(async {
            assert_eq!(*container.make::<usize>().unwrap(), 17);
        })
        .await;
}

#[test]
fn scoped_cache_eviction_drops_values_outside_its_lock() {
    struct Reenter(std::sync::Weak<Container>, Arc<AtomicUsize>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            assert_eq!(*self.0.upgrade().unwrap().make::<u32>().unwrap(), 7);
            self.1.fetch_add(1, Ordering::SeqCst);
        }
    }
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let container = Arc::new(Container::default());
        container.scoped::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
        let weak = Arc::downgrade(&container);
        let dropped = Arc::new(AtomicUsize::new(0));
        let drops = dropped.clone();
        container
            .scoped::<Reenter, _>(move |_| Ok(Arc::new(Reenter(weak.clone(), drops.clone()))))
            .unwrap();
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(container.scope(async {
                drop(container.make::<Reenter>().unwrap());
                container.forgetScopedInstances();
                assert_eq!(dropped.load(Ordering::SeqCst), 1);
            }));
        send.send(()).unwrap();
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    worker.join().unwrap();
}

#[tokio::test]
async fn panicking_constructors_can_retry_without_poisoning_the_scope_cache() {
    let container = Container::default();
    let attempts = AtomicUsize::new(0);
    container
        .scoped::<u8, _>(move |_| {
            assert_ne!(
                attempts.fetch_add(1, Ordering::SeqCst),
                0,
                "constructor panic"
            );
            Ok(Arc::new(4))
        })
        .unwrap();
    container
        .scope(async {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| container.make::<u8>()))
                    .is_err()
            );
            assert_eq!(*container.make::<u8>().unwrap(), 4);
        })
        .await;
}

#[tokio::test]
async fn instance_and_global_eviction_do_not_flush_another_active_scope() {
    let container = Container::default();
    container
        .scopedNamed::<u32, _>("value", |_| Ok(Arc::new(1)))
        .unwrap();
    let gate = tokio::sync::Barrier::new(2);
    tokio::join!(
        container.scope(async {
            container.instanceNamed("value", Arc::new(99_u32)).unwrap();
            assert_eq!(*container.get::<u32>("value").unwrap(), 99);
            gate.wait().await;
            container.forgetInstances();
            assert_eq!(*container.get::<u32>("value").unwrap(), 1);
            gate.wait().await;
        }),
        container.scope(async {
            let original = container.get::<u32>("value").unwrap();
            gate.wait().await;
            gate.wait().await;
            assert!(Arc::ptr_eq(
                &original,
                &container.get::<u32>("value").unwrap()
            ));
        })
    );
}

#[tokio::test]
async fn forgetting_a_scoped_alias_override_does_not_resurrect_hidden_root_instances() {
    let container = Container::default();
    container.instanceNamed("alias", Arc::new(99_u32)).unwrap();
    container
        .scopedNamed::<u32, _>("value", |_| Ok(Arc::new(7)))
        .unwrap();
    container.alias("value", "alias").unwrap();
    container
        .scope(async {
            container.instanceNamed("alias", Arc::new(12_u32)).unwrap();
            container.forgetInstance("alias");
            assert!(!container.has("alias"));
            assert!(!container.isShared("alias"));
            assert!(!container.resolved("alias").unwrap());
            assert!(container.get::<u32>("alias").is_err());
        })
        .await;
    assert!(container.isAlias("alias"));
    assert_eq!(*container.get::<u32>("alias").unwrap(), 7);
}

#[tokio::test]
async fn root_scoped_reset_preserves_a_child_lifecycle_on_another_execution() {
    let container = Container::default();
    container.scoped::<usize, _>(|_| Ok(Arc::new(1))).unwrap();
    let root = container.make::<usize>().unwrap();
    container
        .scope(async {
            let child = container.make::<usize>().unwrap();
            let next_root = std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        container.forgetScopedInstances();
                        container.make::<usize>().unwrap()
                    })
                    .join()
                    .unwrap()
            });
            assert!(!Arc::ptr_eq(&root, &next_root));
            assert!(Arc::ptr_eq(&child, &container.make::<usize>().unwrap()));
        })
        .await;
}

#[tokio::test]
async fn replacing_a_scoped_instance_releases_the_previous_cached_reference() {
    let container = Container::default();
    container.scoped::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    container
        .scope(async {
            let first = container.make::<u32>().unwrap();
            let weak = Arc::downgrade(&first);
            drop(first);
            container.instance(Arc::new(99_u32)).unwrap();
            assert!(weak.upgrade().is_none());
            assert_eq!(*container.make::<u32>().unwrap(), 99);
        })
        .await;
}

#[tokio::test]
#[allow(
    clippy::async_yields_async,
    reason = "Exercises a retained nested future after its parent lifecycle ends"
)]
async fn retained_nested_futures_cannot_confuse_a_replaced_container_at_the_same_address() {
    use std::future::Future as _;
    let owner = Arc::new(std::sync::Mutex::new(Box::new(Container::default())));
    owner
        .lock()
        .unwrap()
        .scoped::<u32, _>(|_| Ok(Arc::new(7)))
        .unwrap();
    let other = Container::default();
    let (send, receive) = tokio::sync::oneshot::channel::<()>();
    let captured = owner.clone();
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let nested = {
        let original = owner.lock().unwrap();
        let mut outer = Box::pin(original.scope(async {
            original.make::<u32>().unwrap();
            let mut inner = Box::pin(other.scope(async move {
                receive.await.unwrap();
                captured.lock().unwrap().make::<u32>().unwrap()
            }));
            assert!(inner.as_mut().poll(&mut context).is_pending());
            inner
        }));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        match outer.as_mut().poll(&mut context) {
            std::task::Poll::Ready(nested) => nested,
            _ => panic!("outer scope unexpectedly suspended"),
        }
    };
    let expected = {
        let mut current = owner.lock().unwrap();
        **current = Container::default();
        current.scoped::<u32, _>(|_| Ok(Arc::new(99))).unwrap();
        current.make::<u32>().unwrap()
    };
    send.send(()).unwrap();
    let result = nested.await;
    assert!(Arc::ptr_eq(&expected, &result));
}
