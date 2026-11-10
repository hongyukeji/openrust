use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
fn key<T: ?Sized + 'static>() -> ServiceKey {
    ServiceKey::typed::<T>()
}
fn failure() -> BindingResolutionError {
    BindingResolutionError::NotBound {
        service: "listener-failure".into(),
    }
}

#[test]
fn registration_resolves_existing_services_without_invoking_the_listener() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    assert!(
        c.rebinding::<u32, _>(key::<u32>(), move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap()
        .is_none()
    );
    c.bind::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(*c.make::<u32>().unwrap(), 1);
    c.bind::<u32, _>(|_| Ok(Arc::new(2))).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let seen = calls.clone();
    let current = c
        .rebinding::<u32, _>(key::<u32>(), move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(*current, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    c.bindIf::<u32, _>(|_| panic!("conditional replacement"))
        .unwrap();
    c.singletonIf::<u32, _>(|_| panic!("conditional replacement"))
        .unwrap();
    c.scopedIf::<u32, _>(|_| panic!("conditional replacement"))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn first_instances_do_not_notify_but_replacements_return_and_deliver_the_supplied_identity() {
    let c = Container::default();
    let values = Arc::new(Mutex::new(Vec::new()));
    let seen = values.clone();
    c.rebinding::<u32, _>("number", move |_, value| {
        seen.lock().unwrap().push(value);
        Ok(())
    })
    .unwrap();
    let first = Arc::new(1_u32);
    assert!(Arc::ptr_eq(
        &first,
        &c.instanceNamed("number", first.clone()).unwrap()
    ));
    assert!(values.lock().unwrap().is_empty());
    let second = Arc::new(2_u32);
    assert!(Arc::ptr_eq(
        &second,
        &c.instanceNamed("number", second.clone()).unwrap()
    ));
    assert!(Arc::ptr_eq(&second, &values.lock().unwrap()[0]));
}

#[test]
fn aliases_are_fixed_on_listener_registration_and_raw_alias_replacements_are_independent() {
    let c = Container::default();
    c.instanceNamed("original", Arc::new(1_u32)).unwrap();
    c.alias("original", "alias").unwrap();
    let values = Arc::new(Mutex::new(Vec::new()));
    let seen = values.clone();
    c.rebinding::<u32, _>("alias", move |_, value| {
        seen.lock().unwrap().push(*value);
        Ok(())
    })
    .unwrap();
    c.instanceNamed("alias", Arc::new(9_u32)).unwrap();
    assert!(values.lock().unwrap().is_empty());
    c.instanceNamed("original", Arc::new(2_u32)).unwrap();
    assert_eq!(*values.lock().unwrap(), [2]);
    assert_eq!(*c.get::<u32>("alias").unwrap(), 9);
}

#[test]
fn listeners_are_ordered_snapshots_and_nested_rebinding_errors_do_not_rollback_commits() {
    let c = Container::default();
    c.instance(Arc::new(1_u32)).unwrap();
    let values = Arc::new(Mutex::new(Vec::new()));
    let once = AtomicBool::new(true);
    let seen = values.clone();
    c.rebinding::<u32, _>(key::<u32>(), move |app, value| {
        seen.lock().unwrap().push(("first", *value));
        if once.swap(false, Ordering::SeqCst) {
            let late = seen.clone();
            app.rebinding::<u32, _>(key::<u32>(), move |_, value| {
                late.lock().unwrap().push(("late", *value));
                Ok(())
            })?;
            assert!(matches!(
                app.instance(Arc::new(99_u32)),
                Err(BindingResolutionError::CircularDependency { .. })
            ));
        }
        Ok(())
    })
    .unwrap();
    let seen = values.clone();
    c.rebinding::<u32, _>(key::<u32>(), move |_, value| {
        seen.lock().unwrap().push(("second", *value));
        Ok(())
    })
    .unwrap();
    c.instance(Arc::new(2_u32)).unwrap();
    assert_eq!(*values.lock().unwrap(), [("first", 2), ("second", 2)]);
    assert_eq!(*c.make::<u32>().unwrap(), 99);
    c.instance(Arc::new(3_u32)).unwrap();
    assert_eq!(
        &values.lock().unwrap()[2..],
        &[("first", 3), ("second", 3), ("late", 3)]
    );
}

#[test]
fn listener_errors_and_panics_propagate_after_the_binding_or_cache_is_committed() {
    let c = Container::default();
    c.instance(Arc::new(1_u32)).unwrap();
    c.rebinding::<u32, _>(key::<u32>(), |_, _| Err(failure()))
        .unwrap();
    assert!(c.instance(Arc::new(2_u32)).is_err());
    assert_eq!(*c.make::<u32>().unwrap(), 2);
    assert!(c.singleton::<u32, _>(|_| Ok(Arc::new(3))).is_err());
    assert_eq!(*c.make::<u32>().unwrap(), 3);
    assert!(
        c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 1)))
            .is_err()
    );
    assert_eq!(*c.make::<u32>().unwrap(), 4);
    let c = Container::default();
    c.instance(Arc::new(1_u32)).unwrap();
    let once = AtomicBool::new(true);
    c.rebinding::<u32, _>(key::<u32>(), move |_, _| {
        if once.swap(false, Ordering::SeqCst) {
            panic!("listener panic");
        }
        Ok(())
    })
    .unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| c.instance(Arc::new(2_u32)))).is_err());
    assert_eq!(*c.make::<u32>().unwrap(), 2);
    c.instance(Arc::new(3_u32)).unwrap();
}

#[test]
fn failed_registration_keeps_the_listener_and_wrong_contracts_fail_without_invoking_it() {
    let c = Container::default();
    c.bind::<u32, _>(|_| Err(failure())).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    assert!(
        c.rebinding::<u32, _>(key::<u32>(), move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .is_err()
    );
    c.instance(Arc::new(7_u32)).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let c = Container::default();
    c.rebinding::<u64, _>("number", |_, _| panic!("wrong listener"))
        .unwrap();
    c.bindNamed::<u32, _>("number", |_| Ok(Arc::new(1)))
        .unwrap();
    c.get::<u32>("number").unwrap();
    assert!(matches!(
        c.bindNamed::<u32, _>("number", |_| Ok(Arc::new(2))),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    assert_eq!(*c.get::<u32>("number").unwrap(), 2);
}

#[test]
fn refresh_registers_an_explicit_target_setter_without_calling_it_initially() {
    let c = Container::default();
    c.instance(Arc::new(1_u32)).unwrap();
    let target = Arc::new(Mutex::new(0_u32));
    let current = c
        .refresh::<u32, _, _>(key::<u32>(), target.clone(), |target, value| {
            *target.lock().unwrap() = *value;
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(*current, 1);
    assert_eq!(*target.lock().unwrap(), 0);
    c.instance(Arc::new(2_u32)).unwrap();
    assert_eq!(*target.lock().unwrap(), 2);
}

#[test]
fn before_and_construction_hooks_precede_listeners_and_instance_updates_skip_post_hooks() {
    let c = Container::default();
    let trace = Arc::new(Mutex::new(Vec::new()));
    c.bind::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    for stage in ["before", "resolving", "after"] {
        let seen = trace.clone();
        let callback = move |_: &openrust::container::ResolutionEvent, _: &Container| {
            seen.lock().unwrap().push(stage);
            Ok(())
        };
        match stage {
            "before" => c.beforeResolving(None, callback),
            "resolving" => c.resolving(None, callback),
            _ => c.afterResolving(None, callback),
        }
        .unwrap();
    }
    let seen = trace.clone();
    c.rebinding::<u32, _>(key::<u32>(), move |_, _| {
        seen.lock().unwrap().push("listener");
        Ok(())
    })
    .unwrap();
    trace.lock().unwrap().clear();
    let seen = trace.clone();
    c.bind::<u32, _>(move |_| {
        seen.lock().unwrap().push("factory");
        Ok(Arc::new(2))
    })
    .unwrap();
    assert_eq!(
        *trace.lock().unwrap(),
        ["before", "factory", "resolving", "after", "listener"]
    );
    trace.lock().unwrap().clear();
    c.instance(Arc::new(3_u32)).unwrap();
    assert_eq!(*trace.lock().unwrap(), ["before", "listener"]);
}

#[test]
fn concurrent_instance_notifications_keep_their_committed_values() {
    use std::sync::mpsc::channel;
    let c = Arc::new(Container::default());
    c.instance(Arc::new(1_u32)).unwrap();
    let values = Arc::new(Mutex::new(Vec::new()));
    let (entered, entered_rx) = channel();
    let (release, release_rx) = channel();
    let release_rx = Mutex::new(release_rx);
    let seen = values.clone();
    c.rebinding::<u32, _>(key::<u32>(), move |_, value| {
        if *value == 2 {
            entered.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
        }
        seen.lock().unwrap().push(*value);
        Ok(())
    })
    .unwrap();
    std::thread::scope(|scope| {
        let older = scope.spawn(|| c.instance(Arc::new(2_u32)).unwrap());
        entered_rx.recv().unwrap();
        c.instance(Arc::new(3_u32)).unwrap();
        release.send(()).unwrap();
        older.join().unwrap();
    });
    assert_eq!(*values.lock().unwrap(), [3, 2]);
    assert_eq!(*c.make::<u32>().unwrap(), 3);
}

#[test]
fn concurrent_recipe_notifications_never_resolve_a_later_replacement_for_an_old_event() {
    use std::sync::mpsc::channel;
    let c = Arc::new(Container::default());
    c.singleton::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    let values = Arc::new(Mutex::new(Vec::new()));
    let seen = values.clone();
    c.rebinding::<u32, _>(key::<u32>(), move |_, value| {
        seen.lock().unwrap().push(*value);
        Ok(())
    })
    .unwrap();
    let (entered, entered_rx) = channel();
    let (release, release_rx) = channel();
    let release_rx = Mutex::new(release_rx);
    std::thread::scope(|scope| {
        let older = scope.spawn(|| {
            c.singleton::<u32, _>(move |_| {
                entered.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                Ok(Arc::new(2))
            })
            .unwrap()
        });
        entered_rx.recv().unwrap();
        c.singleton::<u32, _>(|_| Ok(Arc::new(3))).unwrap();
        release.send(()).unwrap();
        older.join().unwrap();
    });
    assert_eq!(*values.lock().unwrap(), [3, 2]);
    assert_eq!(*c.make::<u32>().unwrap(), 3);
}

#[tokio::test]
async fn scoped_instance_notifications_do_not_replace_other_lifecycles() {
    let c = Container::default();
    c.scoped::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    let values = Arc::new(Mutex::new(Vec::new()));
    let seen = values.clone();
    c.rebinding::<u32, _>(key::<u32>(), move |_, value| {
        seen.lock().unwrap().push(*value);
        Ok(())
    })
    .unwrap();
    for value in [2, 3] {
        c.scope(async {
            assert_eq!(*c.make::<u32>().unwrap(), 1);
            c.instance(Arc::new(value)).unwrap();
            assert_eq!(*c.make::<u32>().unwrap(), value);
        })
        .await;
    }
    assert_eq!(*values.lock().unwrap(), [2, 3]);
    assert_eq!(*c.make::<u32>().unwrap(), 1);
}

#[test]
fn scoped_registration_can_replace_a_raw_circular_alias_without_following_it_first() {
    let c = Container::default();
    c.alias("second", "first").unwrap();
    c.alias("first", "second").unwrap();
    c.scopedNamed::<u32, _>("first", |_| Ok(Arc::new(7)))
        .unwrap();
    assert_eq!(*c.get::<u32>("first").unwrap(), 7);
}

#[test]
fn extending_resolved_transients_and_evicted_singletons_rebuilds_for_the_listener() {
    for shared in [false, true] {
        let c = Container::default();
        if shared {
            c.singleton::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
        } else {
            c.bind::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
        }
        let values = Arc::new(Mutex::new(Vec::new()));
        let seen = values.clone();
        c.rebinding::<u32, _>(key::<u32>(), move |_, value| {
            seen.lock().unwrap().push(*value);
            Ok(())
        })
        .unwrap();
        if shared {
            c.forgetInstance(key::<u32>());
        }
        c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 10)))
            .unwrap();
        assert_eq!(*values.lock().unwrap(), [11]);
        assert_eq!(*c.make::<u32>().unwrap(), 11);
    }
}

#[test]
fn extending_after_an_explicit_only_read_does_not_invent_history_or_notify() {
    let c = Container::default();
    c.singleton::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    c.instance(Arc::new(2_u32)).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    c.rebinding::<u32, _>(key::<u32>(), move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    c.forgetInstance(key::<u32>());
    assert!(!c.resolved(key::<u32>()).unwrap());
    c.extend::<u32, _>(key::<u32>(), |value, _| Ok(Arc::new(*value + 1)))
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!c.resolved(key::<u32>()).unwrap());
    assert_eq!(*c.make::<u32>().unwrap(), 8);
}

#[tokio::test]
async fn a_rebound_scope_retires_stale_cache_values_after_unlocking_the_registry() {
    struct Probe {
        container: std::sync::Weak<Container>,
        original: bool,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            if self.original
                && let Some(container) = self.container.upgrade()
            {
                container.instance(Arc::new(42_u16)).unwrap();
            }
        }
    }
    let c = Arc::new(Container::default());
    let weak = Arc::downgrade(&c);
    c.scoped::<Probe, _>(move |_| {
        Ok(Arc::new(Probe {
            container: weak.clone(),
            original: true,
        }))
    })
    .unwrap();
    c.rebinding::<Probe, _>(key::<Probe>(), |_, _| Ok(()))
        .unwrap();
    c.scope(async {
        c.make::<Probe>().unwrap();
        let other = c.clone();
        std::thread::spawn(move || {
            let weak = Arc::downgrade(&other);
            other
                .scoped::<Probe, _>(move |_| {
                    Ok(Arc::new(Probe {
                        container: weak.clone(),
                        original: false,
                    }))
                })
                .unwrap();
        })
        .join()
        .unwrap();
        // The replacement happened outside this lifecycle, so its old cache still exists.
        c.extend::<Probe, _>(key::<Probe>(), |value, _| Ok(value))
            .unwrap();
        assert!(!c.make::<Probe>().unwrap().original);
        assert_eq!(*c.make::<u16>().unwrap(), 42);
    })
    .await;
}
