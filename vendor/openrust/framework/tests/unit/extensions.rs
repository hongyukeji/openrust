use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
fn key<T: ?Sized + 'static>() -> ServiceKey {
    ServiceKey::typed::<T>()
}
fn failure() -> BindingResolutionError {
    BindingResolutionError::NotBound {
        service: "extension-failure".into(),
    }
}

#[test]
fn future_extenders_follow_aliases_and_registration_order_without_eager_construction() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    c.alias(key::<u32>(), "number").unwrap();
    c.extend::<u32, _>("number", |value, _| Ok(Arc::new(*value + 2)))
        .unwrap();
    c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value * 3)))
        .unwrap();
    let count = calls.clone();
    c.bind::<u32, _>(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(1))
    });
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(*c.make::<u32>().unwrap(), 9);
    assert_eq!(*c.get::<u32>("number").unwrap(), 9);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    c.forgetExtenders("number").unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 1);
}

#[test]
fn cached_singletons_are_updated_immediately_without_persisting_the_new_extender() {
    let c = Container::default();
    c.singleton::<u32, _>(|_| Ok(Arc::new(1)));
    c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 1)))
        .unwrap();
    let original = c.make::<u32>().unwrap();
    assert_eq!(*original, 2);
    c.extend::<u32, _>(key::<u32>(), |value, app| {
        assert!(Arc::ptr_eq(&value, &app.make::<u32>()?));
        Ok(Arc::new(*value * 10))
    })
    .unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 20);
    assert_eq!(*original, 2);
    c.forgetInstance(key::<u32>());
    assert_eq!(*c.make::<u32>().unwrap(), 2);
}

#[test]
fn supplied_instances_bypass_pending_extenders_and_keep_the_original_factory() {
    let c = Container::default();
    c.bind::<u32, _>(|_| Ok(Arc::new(1)));
    c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 10)))
        .unwrap();
    c.instance(Arc::new(99_u32));
    assert_eq!(*c.make::<u32>().unwrap(), 99);
    c.extend::<u32, _>(key::<u32>(), |value, app| {
        assert_eq!(*app.make::<u32>()?, 99);
        Ok(Arc::new(*value + 1))
    })
    .unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 100);
    c.forgetExtenders(key::<u32>()).unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 100);
    c.forgetInstance(key::<u32>());
    assert_eq!(*c.make::<u32>().unwrap(), 1);
}

#[test]
fn extender_contracts_are_checked_before_factories_or_mismatched_decorators_execute() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    c.extend::<u64, _>("number", |_, _| panic!("wrong decorator must not run"))
        .unwrap();
    let count = calls.clone();
    c.bindNamed::<u32, _>("number", move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(1))
    });
    assert!(matches!(
        c.get::<u32>("number"),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(matches!(
        c.extend::<u64, _>("number", |_, _| panic!("wrong binding")),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
}

#[test]
fn failing_or_panicking_future_extenders_retry_without_publishing_a_partial_singleton() {
    let c = Container::default();
    let attempts = AtomicUsize::new(0);
    c.singleton::<u32, _>(|_| Ok(Arc::new(1)));
    c.extend::<u32, _>(key::<u32>(), move |value, _| {
        match attempts.fetch_add(1, Ordering::SeqCst) {
            0 => Err(failure()),
            1 => panic!("extender panic"),
            _ => Ok(Arc::new(*value + 1)),
        }
    })
    .unwrap();
    assert!(c.make::<u32>().is_err());
    assert!(!c.resolved(key::<u32>()).unwrap());
    assert!(catch_unwind(AssertUnwindSafe(|| c.make::<u32>())).is_err());
    assert!(!c.resolved(key::<u32>()).unwrap());
    assert_eq!(*c.make::<u32>().unwrap(), 2);
    assert_eq!(*c.make::<u32>().unwrap(), 2);
}

#[test]
fn failing_cached_updates_restore_the_existing_pointer_and_allow_another_extension() {
    let c = Container::default();
    c.instance(Arc::new(1_u32));
    let original = c.make::<u32>().unwrap();
    assert!(
        c.extend::<u32, _>(key::<u32>(), |_, _| Err(failure()))
            .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(
            || c.extend::<u32, _>(key::<u32>(), |_, _| panic!("cached panic"))
        ))
        .is_err()
    );
    assert!(Arc::ptr_eq(&original, &c.make::<u32>().unwrap()));
    c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 1)))
        .unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 2);
}

#[test]
fn decorators_run_before_resolution_callbacks_and_contextual_values_do_not_replace_defaults() {
    struct Consumer(Arc<u32>);
    let c = Container::default();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let seen = trace.clone();
    c.singleton::<u32, _>(move |_| {
        seen.lock().unwrap().push("factory");
        Ok(Arc::new(1))
    });
    let seen = trace.clone();
    c.extend::<u32, _>(key::<u32>(), move |value, _| {
        seen.lock().unwrap().push("extend");
        Ok(Arc::new(*value + 1))
    })
    .unwrap();
    let seen = trace.clone();
    c.resolving(Some(key::<u32>()), move |event, _| {
        assert_eq!(*event.value::<u32>().unwrap(), 2);
        seen.lock().unwrap().push("resolving");
        Ok(())
    })
    .unwrap();
    let seen = trace.clone();
    c.afterResolving(Some(key::<u32>()), move |_, _| {
        seen.lock().unwrap().push("after");
        Ok(())
    })
    .unwrap();
    let original = c.make::<u32>().unwrap();
    assert_eq!(
        *trace.lock().unwrap(),
        ["factory", "extend", "resolving", "after"]
    );
    // Contextual values use the same persistent chain, without overwriting the default cache.
    c.bind::<Consumer, _>(|c| Ok(Arc::new(Consumer(c.make()?))));
    c.when::<Consumer>()
        .unwrap()
        .needs::<u32>()
        .give(|_| Ok(Arc::new(1)))
        .unwrap();
    let first = c.make::<Consumer>().unwrap();
    let second = c.make::<Consumer>().unwrap();
    assert_eq!(*first.0, 2);
    assert!(!Arc::ptr_eq(&first.0, &second.0));
    assert!(Arc::ptr_eq(&original, &c.make::<u32>().unwrap()));
}

#[test]
fn extending_from_a_post_callback_changes_the_cache_without_overwriting_it_at_completion() {
    let c = Container::default();
    c.singleton::<u32, _>(|_| Ok(Arc::new(1)));
    c.resolving(Some(key::<u32>()), |event, app| {
        app.extend::<u32, _>(event.key().clone(), |value, _| Ok(Arc::new(*value + 10)))?;
        assert_eq!(*app.make::<u32>()?, 11);
        Ok(())
    })
    .unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 1); // Laravel returns its original local object.
    assert_eq!(*c.make::<u32>().unwrap(), 11); // The cache contains the replacement.
}

#[test]
fn an_extender_registered_during_construction_is_included_once_before_publication() {
    use std::sync::mpsc::channel;
    let c = Arc::new(Container::default());
    let (entered, enter_rx) = channel();
    let (release, release_rx) = channel();
    let receiver = Mutex::new(release_rx);
    c.singleton::<u32, _>(move |_| {
        entered.send(()).unwrap();
        receiver.lock().unwrap().recv().unwrap();
        Ok(Arc::new(1))
    });
    let calls = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        let pending = scope.spawn(|| c.make::<u32>().unwrap());
        enter_rx.recv().unwrap();
        let count = calls.clone();
        c.extend::<u32, _>(key::<u32>(), move |value, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(*value + 2))
        })
        .unwrap();
        release.send(()).unwrap();
        assert_eq!(*pending.join().unwrap(), 3);
    });
    assert_eq!(*c.make::<u32>().unwrap(), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_cached_extensions_are_serialized_and_other_readers_wait() {
    use std::sync::mpsc::channel;
    let c = Arc::new(Container::default());
    c.instance(Arc::new(1_u32));
    let (entered, enter_rx) = channel();
    let (release, release_rx) = channel();
    let receiver = Mutex::new(release_rx);
    let (reading, read_rx) = channel();
    c.beforeResolving(Some(key::<u32>()), move |_, _| {
        reading.send(()).unwrap();
        Ok(())
    })
    .unwrap();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            c.extend::<u32, _>(key::<u32>(), move |value, _| {
                entered.send(()).unwrap();
                receiver.lock().unwrap().recv().unwrap();
                Ok(Arc::new(*value + 1))
            })
            .unwrap()
        });
        enter_rx.recv().unwrap();
        let (done, done_rx) = channel();
        let app = c.clone();
        let reader = scope.spawn(move || {
            let value = app.make::<u32>().unwrap();
            done.send(()).unwrap();
            value
        });
        read_rx.recv().unwrap();
        assert!(done_rx.try_recv().is_err());
        let second = scope.spawn(|| {
            c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value * 10)))
                .unwrap()
        });
        release.send(()).unwrap();
        first.join().unwrap();
        second.join().unwrap();
        assert!([2, 20].contains(&*reader.join().unwrap()));
    });
    assert_eq!(*c.make::<u32>().unwrap(), 20);
}

#[test]
fn recursive_extensions_are_rejected_without_poisoning_the_cache() {
    let c = Container::default();
    c.instance(Arc::new(1_u32));
    assert!(matches!(
        c.extend::<u32, _>(key::<u32>(), |value, app| {
            app.extend::<u32, _>(key::<u32>(), |inner, _| Ok(inner))?;
            Ok(value)
        }),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
    assert_eq!(*c.make::<u32>().unwrap(), 1);
    let c = Container::default();
    c.singleton::<u32, _>(|_| Ok(Arc::new(1)));
    c.extend::<u32, _>(key::<u32>(), |_, app| app.make::<u32>())
        .unwrap();
    assert!(matches!(
        c.make::<u32>(),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
}

#[tokio::test]
async fn cached_scoped_extensions_stay_local_and_future_chains_survive_scope_eviction() {
    let c = Container::default();
    c.scoped::<u32, _>(|_| Ok(Arc::new(1)));
    c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 1)))
        .unwrap();
    c.scope(async {
        assert_eq!(*c.make::<u32>().unwrap(), 2);
        c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value * 10)))
            .unwrap();
        assert_eq!(*c.make::<u32>().unwrap(), 20);
        c.forgetScopedInstances();
        assert_eq!(*c.make::<u32>().unwrap(), 2);
    })
    .await;
    c.scope(async {
        assert_eq!(*c.make::<u32>().unwrap(), 2);
    })
    .await;
}

#[test]
fn forgetting_extenders_drops_captures_outside_the_registry_lock() {
    struct Reenter(Arc<Container>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            self.0.instance(Arc::new(42_u16));
        }
    }
    let c = Arc::new(Container::default());
    let capture = Reenter(c.clone());
    c.extend::<u32, _>(key::<u32>(), move |value, _| {
        let _ = &capture;
        Ok(value)
    })
    .unwrap();
    c.forgetExtenders(key::<u32>()).unwrap();
    assert_eq!(*c.make::<u16>().unwrap(), 42);
}

#[test]
fn retired_extender_captures_are_dropped_before_publishing_the_constructed_value() {
    struct PanicDrop;
    impl Drop for PanicDrop {
        fn drop(&mut self) {
            panic!("capture destructor");
        }
    }
    let c = Container::default();
    c.singleton::<u32, _>(|_| Ok(Arc::new(1)));
    let capture = PanicDrop;
    c.extend::<u32, _>(key::<u32>(), move |value, app| {
        let _ = &capture;
        app.forgetExtenders(key::<u32>())?;
        Ok(value)
    })
    .unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| c.make::<u32>())).is_err());
    assert!(!c.resolved(key::<u32>()).unwrap());
    assert_eq!(*c.make::<u32>().unwrap(), 1);
}

#[test]
fn interface_services_can_be_wrapped_without_changing_the_public_contract() {
    trait Named: Send + Sync {
        fn name(&self) -> String;
    }
    struct Base;
    impl Named for Base {
        fn name(&self) -> String {
            "base".into()
        }
    }
    struct Decorated(Arc<dyn Named>);
    impl Named for Decorated {
        fn name(&self) -> String {
            format!("decorated:{}", self.0.name())
        }
    }
    let c = Container::default();
    c.bind::<dyn Named, _>(|_| Ok(Arc::new(Base)));
    c.extend::<dyn Named, _>(key::<dyn Named>(), |value, _| {
        Ok(Arc::new(Decorated(value)))
    })
    .unwrap();
    assert_eq!(c.make::<dyn Named>().unwrap().name(), "decorated:base");
}

#[test]
fn cross_thread_extension_cycles_use_the_existing_wait_graph_instead_of_deadlocking() {
    let c = Arc::new(Container::default());
    c.instance(Arc::new(1_u32));
    c.instance(Arc::new(2_u64));
    let gate = Arc::new(std::sync::Barrier::new(2));
    let outcomes = std::thread::scope(|scope| {
        let first_gate = gate.clone();
        let first = scope.spawn(|| {
            c.extend::<u32, _>(key::<u32>(), move |value, app| {
                first_gate.wait();
                app.extend::<u64, _>(key::<u64>(), |other, _| Ok(Arc::new(*other + 1)))?;
                Ok(value)
            })
        });
        let second = scope.spawn(|| {
            c.extend::<u64, _>(key::<u64>(), move |value, app| {
                gate.wait();
                app.extend::<u32, _>(key::<u32>(), |other, _| Ok(Arc::new(*other + 1)))?;
                Ok(value)
            })
        });
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert!(outcomes.iter().any(|result| matches!(
        result,
        Err(BindingResolutionError::CircularDependency { .. })
    )));
    assert!(c.make::<u32>().is_ok());
    assert!(c.make::<u64>().is_ok());
}

#[test]
fn an_inflight_extension_cannot_overwrite_a_newly_rebound_instance() {
    use std::sync::mpsc::channel;
    let c = Arc::new(Container::default());
    c.instance(Arc::new(1_u32));
    let (entered, entered_rx) = channel();
    let (release, release_rx) = channel();
    let receiver = Mutex::new(release_rx);
    std::thread::scope(|scope| {
        let pending = scope.spawn(|| {
            c.extend::<u32, _>(key::<u32>(), move |value, _| {
                entered.send(()).unwrap();
                receiver.lock().unwrap().recv().unwrap();
                Ok(Arc::new(*value + 1))
            })
            .unwrap()
        });
        entered_rx.recv().unwrap();
        c.instance(Arc::new(99_u32));
        release.send(()).unwrap();
        pending.join().unwrap();
    });
    assert_eq!(*c.make::<u32>().unwrap(), 99);
}
