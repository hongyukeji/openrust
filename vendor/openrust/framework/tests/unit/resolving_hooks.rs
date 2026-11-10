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
type Trace = Arc<Mutex<Vec<&'static str>>>;
fn record(trace: &Trace, item: &'static str) {
    trace.lock().unwrap().push(item);
}
fn missing() -> BindingResolutionError {
    BindingResolutionError::NotBound {
        service: "test-error".into(),
    }
}

#[test]
fn global_callbacks_precede_keyed_callbacks_in_each_stage_and_values_keep_identity() {
    let c = Container::default();
    let trace = Trace::default();
    let seen = trace.clone();
    c.bind::<u32, _>(move |_| {
        record(&seen, "factory");
        Ok(Arc::new(42))
    })
    .unwrap();
    for (selector, label) in [
        (Some(ServiceKey::typed::<u32>()), "before-key"),
        (None, "before-global"),
    ] {
        let seen = trace.clone();
        c.beforeResolving(selector, move |event, _| {
            assert!(event.value::<u32>().is_none());
            record(&seen, label);
            Ok(())
        })
        .unwrap();
    }
    for (selector, label) in [
        (Some(ServiceKey::typed::<u32>()), "resolve-key"),
        (None, "resolve-global"),
    ] {
        let seen = trace.clone();
        c.resolving(selector, move |event, app| {
            assert_eq!(*event.value::<u32>().unwrap(), 42);
            assert!(!app.resolved(event.key().clone())?);
            record(&seen, label);
            Ok(())
        })
        .unwrap();
    }
    for (selector, label) in [
        (Some(ServiceKey::typed::<u32>()), "after-key"),
        (None, "after-global"),
    ] {
        let seen = trace.clone();
        c.afterResolving(selector, move |_, _| {
            record(&seen, label);
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(*c.make::<u32>().unwrap(), 42);
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "before-global",
            "before-key",
            "factory",
            "resolve-global",
            "resolve-key",
            "after-global",
            "after-key"
        ]
    );
    assert!(c.resolved(ServiceKey::typed::<u32>()).unwrap());
}

#[test]
fn before_callbacks_can_supply_missing_services_and_use_the_original_canonical_alias() {
    let c = Container::default();
    c.alias("original", "alias").unwrap();
    c.beforeResolving(Some("alias".into()), |event, app| {
        assert_eq!(event.key(), &ServiceKey::named("original"));
        app.alias("elsewhere", "alias")?;
        app.instanceNamed("original", Arc::new(7_u32)).unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(*c.get::<u32>("alias").unwrap(), 7);
    assert_eq!(c.getAlias("alias").unwrap(), ServiceKey::named("elsewhere"));
}

#[test]
fn shared_cache_hits_run_before_callbacks_only_and_supplied_instances_skip_post_callbacks() {
    let c = Container::default();
    let before = Arc::new(AtomicUsize::new(0));
    let after = Arc::new(AtomicUsize::new(0));
    let observed = before.clone();
    c.beforeResolving(None, move |_, _| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    let observed = after.clone();
    c.afterResolving(None, move |_, _| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    c.singleton::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    let first = c.make::<u32>().unwrap();
    assert!(Arc::ptr_eq(&first, &c.make::<u32>().unwrap()));
    c.instance(Arc::new(9_u32)).unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 9);
    assert_eq!(before.load(Ordering::SeqCst), 3);
    assert_eq!(after.load(Ordering::SeqCst), 1);
}

#[test]
fn singleton_callbacks_can_reenter_the_published_cache_before_history_is_recorded() {
    let c = Container::default();
    c.singleton::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    c.resolving(Some(ServiceKey::typed::<u32>()), |event, app| {
        assert!(app.resolved(event.key().clone())?);
        assert!(Arc::ptr_eq(
            &event.value::<u32>().unwrap(),
            &app.make::<u32>()?
        ));
        Ok(())
    })
    .unwrap();
    assert_eq!(*c.make::<u32>().unwrap(), 7);
}

#[test]
fn failing_post_callbacks_preserve_published_singletons_but_not_transient_history() {
    let c = Container::default();
    c.singleton::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    c.bind::<u64, _>(|_| Ok(Arc::new(9))).unwrap();
    c.resolving(None, |_, _| Err(missing())).unwrap();
    assert!(c.make::<u32>().is_err());
    assert!(c.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert_eq!(*c.make::<u32>().unwrap(), 7);
    c.forgetInstance(ServiceKey::typed::<u32>());
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert!(c.make::<u64>().is_err());
    assert!(c.make::<u64>().is_err());
    assert!(!c.resolved(ServiceKey::typed::<u64>()).unwrap());
}

#[test]
fn callback_panics_release_waiters_and_leave_the_already_published_shared_value() {
    let c = Container::default();
    c.singleton::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    c.afterResolving(None, |_, _| panic!("post callback"))
        .unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| c.make::<u32>())).is_err());
    assert_eq!(*c.make::<u32>().unwrap(), 7);
    c.forgetInstance(ServiceKey::typed::<u32>());
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert!(catch_unwind(AssertUnwindSafe(|| c.make::<u32>())).is_err());
}

#[test]
fn before_callback_failures_retry_without_calling_the_constructor() {
    let c = Container::default();
    let attempts = AtomicUsize::new(0);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    c.bind::<u32, _>(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(7))
    })
    .unwrap();
    c.beforeResolving(None, move |_, _| {
        match attempts.fetch_add(1, Ordering::SeqCst) {
            0 => Err(missing()),
            1 => panic!("before callback"),
            _ => Ok(()),
        }
    })
    .unwrap();
    assert!(c.make::<u32>().is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| c.make::<u32>())).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(*c.make::<u32>().unwrap(), 7);
}

#[test]
fn unbounded_before_and_transient_post_recursion_returns_a_cycle_error() {
    let c = Container::default();
    c.beforeResolving(Some(ServiceKey::typed::<u32>()), |_, c| {
        c.make::<u32>()?;
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        c.make::<u32>(),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
    c.bind::<u64, _>(|_| Ok(Arc::new(9))).unwrap();
    c.afterResolving(Some(ServiceKey::typed::<u64>()), |_, c| {
        c.make::<u64>()?;
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        c.make::<u64>(),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
}

#[test]
fn snapshots_allow_global_callbacks_to_register_keyed_callbacks_for_the_current_stage() {
    let c = Container::default();
    let trace = Trace::default();
    c.bind::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    let before = trace.clone();
    c.beforeResolving(None, move |_, app| {
        let seen = before.clone();
        app.beforeResolving(Some(ServiceKey::typed::<u32>()), move |_, _| {
            record(&seen, "new-keyed");
            Ok(())
        })?;
        let seen = before.clone();
        app.beforeResolving(None, move |_, _| {
            record(&seen, "next-global");
            Ok(())
        })?;
        Ok(())
    })
    .unwrap();
    let after = trace.clone();
    c.resolving(None, move |_, app| {
        let seen = after.clone();
        app.afterResolving(None, move |_, _| {
            record(&seen, "new-after");
            Ok(())
        })?;
        Ok(())
    })
    .unwrap();
    c.make::<u32>().unwrap();
    assert_eq!(*trace.lock().unwrap(), ["new-keyed", "new-after"]);

    let c = Container::default();
    let trace = Trace::default();
    c.bindNamed::<u32, _>("number", |_| Ok(Arc::new(7)))
        .unwrap();
    let seen = trace.clone();
    c.resolving(None, move |_, app| {
        record(&seen, "global");
        let late = seen.clone();
        app.resolving(Some("number".into()), move |_, _| {
            record(&late, "named-late");
            Ok(())
        })
    })
    .unwrap();
    for (key, label) in [
        (ServiceKey::typed::<u32>(), "typed-first"),
        (ServiceKey::named("number"), "named"),
        (ServiceKey::typed::<u32>(), "typed-second"),
    ] {
        let seen = trace.clone();
        c.resolving(Some(key), move |_, _| {
            record(&seen, label);
            Ok(())
        })
        .unwrap();
    }
    c.get::<u32>("number").unwrap();
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "global",
            "typed-first",
            "typed-second",
            "named",
            "named-late"
        ]
    );
}

#[test]
fn named_services_also_match_their_declared_output_type_without_duplicate_dispatch() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    c.bindNamed::<u32, _>("number", |_| Ok(Arc::new(7)))
        .unwrap();
    c.bind::<u32, _>(|_| Ok(Arc::new(9))).unwrap();
    let count = calls.clone();
    c.resolving(Some(ServiceKey::typed::<u32>()), move |event, _| {
        assert!(event.value::<u32>().is_some());
        assert!(event.value::<u64>().is_none());
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    c.get::<u32>("number").unwrap();
    c.make::<u32>().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn concurrent_shared_readers_wait_for_post_callbacks_and_callbacks_run_once() {
    use std::sync::mpsc::channel;
    let c = Arc::new(Container::default());
    let (entered, entered_rx) = channel();
    let (release, release_rx) = channel();
    let release_rx = Mutex::new(release_rx);
    let stage = Arc::new(AtomicUsize::new(0));
    let active = stage.clone();
    c.singleton::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    c.resolving(None, move |_, _| {
        active.store(1, Ordering::SeqCst);
        entered.send(()).unwrap();
        release_rx.lock().unwrap().recv().unwrap();
        active.store(2, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    let (reading, reading_rx) = channel();
    let active = stage.clone();
    c.beforeResolving(None, move |_, _| {
        if active.load(Ordering::SeqCst) == 1 {
            reading.send(()).unwrap();
        }
        Ok(())
    })
    .unwrap();
    std::thread::scope(|scope| {
        let a = scope.spawn(|| c.make::<u32>().unwrap());
        entered_rx.recv().unwrap();
        let (done, done_rx) = channel();
        let second = c.clone();
        let b = scope.spawn(move || {
            let value = second.make::<u32>().unwrap();
            done.send(()).unwrap();
            value
        });
        reading_rx.recv().unwrap();
        assert!(done_rx.try_recv().is_err());
        release.send(()).unwrap();
        assert!(Arc::ptr_eq(&a.join().unwrap(), &b.join().unwrap()));
    });
    assert_eq!(stage.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn scoped_callbacks_run_once_per_lifecycle_and_failed_callbacks_do_not_escape_the_scope() {
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    c.scoped::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
    let count = calls.clone();
    c.afterResolving(None, move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        Err(missing())
    })
    .unwrap();
    for _ in 0..2 {
        c.scope(async {
            assert!(c.make::<u32>().is_err());
            assert_eq!(*c.make::<u32>().unwrap(), 7);
        })
        .await;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!c.resolved(ServiceKey::typed::<u32>()).unwrap());
}
