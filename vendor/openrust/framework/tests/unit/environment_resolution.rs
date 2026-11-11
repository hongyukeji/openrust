use openrust::{
    container::{Container, EnvironmentQuery, EnvironmentResult, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicUsize, Ordering},
};
#[test]
fn absent_resolver_returns_false_for_every_shape_and_empty_values_are_not_reinterpreted() {
    let c = Container::default();
    assert_eq!(c.currentEnvironmentIs("local").unwrap(), false.into());
    assert_eq!(c.currentEnvironmentIs(&[]).unwrap(), false.into());
    assert_eq!(c.currentEnvironmentIs("").unwrap(), false.into());
    assert_eq!(c.currentEnvironmentIs("*").unwrap(), false.into());
}
#[test]
fn queries_preserve_exact_string_array_order_duplicates_and_borrowed_identity() {
    let c = Container::default();
    let captured = Arc::new(Mutex::new(vec![]));
    let observed = captured.clone();
    c.resolveEnvironmentUsing(Some(Arc::new(move |query| {
        observed.lock().unwrap().push(match query {
            EnvironmentQuery::Name(value) => format!("one:{value}"),
            EnvironmentQuery::Names(values) => format!("many:{}", values.join("|")),
        });
        Ok(true.into())
    })));
    let owned = "local".to_owned();
    let query = EnvironmentQuery::from(&owned);
    if let EnvironmentQuery::Name(value) = query {
        assert_eq!(value.as_ptr(), owned.as_ptr());
    } else {
        panic!("shape changed");
    }
    assert_eq!(c.currentEnvironmentIs(&owned).unwrap(), true.into());
    c.currentEnvironmentIs(&["", "*", "0", "local", "local"])
        .unwrap();
    c.currentEnvironmentIs(&[]).unwrap();
    assert_eq!(
        *captured.lock().unwrap(),
        ["one:local", "many:|*|0|local|local", "many:"]
    );
}
#[test]
fn return_values_are_preserved_and_bool_conversion_matches_php_for_strings() {
    for (value, expected) in [
        ("", false),
        ("0", false),
        ("false", true),
        ("00", true),
        ("testing", true),
    ] {
        let c = Container::default();
        c.resolveEnvironmentUsing(Some(Arc::new(move |_| Ok(value.into()))));
        let result = c.currentEnvironmentIs("uninterpreted").unwrap();
        assert_eq!(result, EnvironmentResult::from(value));
        assert_eq!(bool::from(&result), expected);
        assert_eq!(bool::from(result), expected);
    }
    assert!(!bool::from(EnvironmentResult::Boolean(false)));
    assert!(bool::from(EnvironmentResult::Boolean(true)));
    let current: Arc<str> = Arc::from("testing");
    let observed = current.clone();
    let c = Container::default();
    c.resolveEnvironmentUsing(Some(Arc::new(move |_| Ok(observed.clone().into()))));
    if let EnvironmentResult::String(value) = c.currentEnvironmentIs("anything").unwrap() {
        assert!(Arc::ptr_eq(&value, &current));
    } else {
        panic!("string result changed");
    }
}
#[test]
fn replacing_or_clearing_resolver_does_not_clear_binding_cache_or_emit_resolution_events() {
    let c = Container::default();
    let events = Arc::new(AtomicUsize::new(0));
    let seen = events.clone();
    c.beforeResolving(None, move |_, _| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    c.singleton::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    let original = c.make::<u32>().unwrap();
    c.resolveEnvironmentUsing(Some(Arc::new(|_| Ok(true.into()))));
    assert!(bool::from(c.currentEnvironmentIs("local").unwrap()));
    c.resolveEnvironmentUsing(None);
    assert!(!bool::from(c.currentEnvironmentIs("local").unwrap()));
    assert_eq!(events.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&original, &c.make::<u32>().unwrap()));
}
#[test]
fn resolver_is_retained_by_flush_and_errors_and_panics_do_not_poison_the_registry() {
    let c = Container::default();
    c.resolveEnvironmentUsing(Some(Arc::new(|_| {
        Err(BindingResolutionError::NotBound {
            service: "test environment failure".into(),
        })
    })));
    c.flush();
    assert!(matches!(
        c.currentEnvironmentIs("local"),
        Err(BindingResolutionError::NotBound { .. })
    ));
    c.resolveEnvironmentUsing(Some(Arc::new(|_| panic!("test resolver panic"))));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || c.currentEnvironmentIs("local")
        ))
        .is_err()
    );
    c.resolveEnvironmentUsing(Some(Arc::new(|_| Ok("ready".into()))));
    assert_eq!(c.currentEnvironmentIs(&["*"]).unwrap(), "ready".into());
}
#[test]
fn callbacks_run_without_registry_lock_and_current_invocation_keeps_its_original_callback() {
    let c = Arc::new(Container::default());
    let weak = Arc::downgrade(&c);
    c.resolveEnvironmentUsing(Some(Arc::new(move |_| {
        let c = weak.upgrade().unwrap();
        c.resolveEnvironmentUsing(Some(Arc::new(|_| Ok(false.into()))));
        c.instance(Arc::new(7u32))?;
        Ok("original".into())
    })));
    assert_eq!(c.currentEnvironmentIs("first").unwrap(), "original".into());
    assert_eq!(c.currentEnvironmentIs("second").unwrap(), false.into());
    assert_eq!(*c.make::<u32>().unwrap(), 7);
}
#[test]
fn retired_callback_captures_drop_after_replacement_and_can_reenter_the_container() {
    struct Reenter(Weak<Container>, Arc<AtomicUsize>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            let c = self.0.upgrade().unwrap();
            assert_eq!(c.currentEnvironmentIs("now").unwrap(), false.into());
            c.instance(Arc::new(3u32)).unwrap();
            self.1.fetch_add(1, Ordering::SeqCst);
        }
    }
    let c = Arc::new(Container::default());
    let n = Arc::new(AtomicUsize::new(0));
    let payload = Reenter(Arc::downgrade(&c), n.clone());
    c.resolveEnvironmentUsing(Some(Arc::new(move |_| {
        let _keep = &payload;
        Ok(true.into())
    })));
    c.resolveEnvironmentUsing(None);
    assert_eq!(n.load(Ordering::SeqCst), 1);
    assert_eq!(*c.make::<u32>().unwrap(), 3);
}
#[test]
fn different_containers_resolvers_and_service_histories_remain_independent() {
    let a = Container::default();
    let b = Container::default();
    a.resolveEnvironmentUsing(Some(Arc::new(|query| {
        Ok(matches!(query, EnvironmentQuery::Name("local")).into())
    })));
    b.resolveEnvironmentUsing(Some(Arc::new(|query| {
        Ok(matches!(query, EnvironmentQuery::Name("production")).into())
    })));
    assert!(bool::from(a.currentEnvironmentIs("local").unwrap()));
    assert!(!bool::from(b.currentEnvironmentIs("local").unwrap()));
    a.flush();
    assert!(bool::from(a.currentEnvironmentIs("local").unwrap()));
    assert!(!a.resolved(ServiceKey::typed::<u32>()).unwrap());
}

#[test]
fn concurrent_replacement_preserves_an_active_callback_until_it_returns() {
    struct Held(Arc<AtomicUsize>);
    impl Drop for Held {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let c = Arc::new(Container::default());
    let ready = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let dropped = Arc::new(AtomicUsize::new(0));
    let entered = ready.clone();
    let gate = release.clone();
    let held = Held(dropped.clone());
    c.resolveEnvironmentUsing(Some(Arc::new(move |_| {
        let _keep = &held;
        entered.wait();
        gate.wait();
        Ok("original".into())
    })));
    std::thread::scope(|scope| {
        let running = scope.spawn(|| c.currentEnvironmentIs("first"));
        ready.wait();
        c.resolveEnvironmentUsing(None);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        assert_eq!(c.currentEnvironmentIs("second").unwrap(), false.into());
        release.wait();
        assert_eq!(running.join().unwrap().unwrap(), "original".into());
    });
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}
