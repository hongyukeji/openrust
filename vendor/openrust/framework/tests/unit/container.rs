use std::sync::{
    Arc, Barrier,
    atomic::{AtomicUsize, Ordering},
};

use openrust::{
    container::Container,
    contracts::container::{BindingResolutionError, Container as _},
};

#[test]
fn singleton_retries_after_failure_and_shares_the_first_success() {
    let container = Container::default();
    let calls = AtomicUsize::new(0);
    container.singleton::<usize, _>(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(BindingResolutionError::NotBound {
                service: "probe".into(),
            });
        }
        Ok(Arc::new(42))
    });
    assert!(container.make::<usize>().is_err());

    let gate = Barrier::new(4);
    let instances = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    gate.wait();
                    container.make::<usize>().unwrap()
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(
        instances
            .iter()
            .all(|instance| Arc::ptr_eq(instance, &instances[0]))
    );
    assert_eq!(*instances[0], 42);
    let replacement = Arc::new(7);
    container.instance(replacement.clone());
    assert!(Arc::ptr_eq(
        &container.make::<usize>().unwrap(),
        &replacement
    ));
}

#[test]
fn transient_factory_and_cycles() {
    let container = Container::default();
    container.bind::<usize, _>(|_| Ok(Arc::new(7)));
    assert!(!Arc::ptr_eq(
        &container.make::<usize>().unwrap(),
        &container.make::<usize>().unwrap()
    ));
    container.singleton::<usize, _>(|container| container.make::<usize>());
    assert!(matches!(
        container.make::<usize>(),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
}

#[test]
fn cross_thread_singleton_cycles_report_an_error() {
    let container = Arc::new(Container::default());
    let gate = Arc::new(Barrier::new(2));
    for binding in [false, true] {
        let gate = Arc::clone(&gate);
        let attempts = AtomicUsize::new(0);
        if binding {
            container.singleton::<u32, _>(move |container| {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    gate.wait();
                }
                container.make::<u64>().map(|_| Arc::new(0))
            });
        } else {
            container.singleton::<u64, _>(move |container| {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    gate.wait();
                }
                container.make::<u32>().map(|_| Arc::new(0))
            });
        }
    }
    let (send, receive) = std::sync::mpsc::channel();
    let other_container = Arc::clone(&container);
    let other_send = send.clone();
    let first = std::thread::spawn(move || {
        other_send
            .send(other_container.make::<u32>().map(|_| ()))
            .unwrap()
    });
    let second =
        std::thread::spawn(move || send.send(container.make::<u64>().map(|_| ())).unwrap());
    for _ in 0..2 {
        assert!(matches!(
            receive
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            Err(BindingResolutionError::CircularDependency { .. })
        ));
    }
    first.join().unwrap();
    second.join().unwrap();
}

#[test]
fn named_bindings_alias_chains_and_rebinding_keep_service_identity() {
    use openrust::container::ServiceKey;
    let container = Container::default();
    container.singletonNamed::<String, _>("mail", |_| Ok(Arc::new("first".into())));
    container.alias("mail", "mailer").unwrap();
    container.alias("mailer", "outgoing").unwrap();
    assert!(container.has("outgoing"));
    assert!(container.isAlias("mailer"));
    assert_eq!(
        container.getAlias("outgoing").unwrap(),
        ServiceKey::named("mail")
    );
    let first = container.get::<String>("outgoing").unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &container.get::<String>("mail").unwrap()
    ));
    container.instanceNamed("mail", Arc::new(String::from("replacement")));
    assert_eq!(
        &*container.get::<String>("outgoing").unwrap(),
        "replacement"
    );
    assert_eq!(&*first, "first");
    container.instanceNamed("outgoing", Arc::new(19_u64));
    assert!(!container.isAlias("outgoing"));
    assert_eq!(*container.get::<u64>("outgoing").unwrap(), 19);
    assert_eq!(&*container.get::<String>("mailer").unwrap(), "replacement");
    assert!(matches!(
        container.alias("mail", "mail"),
        Err(BindingResolutionError::AliasToSelf { .. })
    ));
    assert!(!container.isAlias("mail"));
}

#[test]
fn aliases_allow_late_bindings_and_detect_cycles_without_changing_types() {
    use openrust::container::ServiceKey;
    let container = Container::default();
    container.alias(ServiceKey::typed::<str>(), "name").unwrap();
    assert!(container.has("name"));
    assert!(!container.bound::<str>());
    assert!(matches!(
        container.get::<str>("name"),
        Err(BindingResolutionError::NotBound { .. })
    ));
    container.instance::<str>(Arc::from("Taylor"));
    assert_eq!(&*container.get::<str>("name").unwrap(), "Taylor");
    container.alias("other", "name").unwrap();
    container.alias("name", "other").unwrap();
    assert!(matches!(
        container.getAlias("name"),
        Err(BindingResolutionError::CircularAlias { .. })
    ));
    container
        .alias(ServiceKey::typed::<str>(), "other")
        .unwrap();
    assert_eq!(&*container.get::<str>("name").unwrap(), "Taylor");
    assert_eq!(
        container.getAlias("unknown").unwrap(),
        ServiceKey::named("unknown")
    );
    assert!(!container.has("Name"));
}

#[test]
fn named_output_type_is_checked_before_the_factory_runs() {
    let container = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    container.bindNamed::<u32, _>("value", move |_| {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(7))
    });
    assert!(matches!(
        container.get::<String>("value"),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(*container.get::<u32>("value").unwrap(), 7);
    assert!(!Arc::ptr_eq(
        &container.get::<u32>("value").unwrap(),
        &container.get::<u32>("value").unwrap()
    ));
    container.bindNamed::<u32, _>("recursive", |container| container.get("recursive"));
    assert!(matches!(
        container.get::<u32>("recursive"),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
}

#[test]
fn trait_contracts_and_type_names_are_distinct_from_named_keys() {
    use openrust::container::ServiceKey;
    trait Transport: Send + Sync {
        fn send(&self) -> &'static str;
    }
    struct Mail;
    impl Transport for Mail {
        fn send(&self) -> &'static str {
            "sent"
        }
    }
    let container = Container::default();
    container.singleton::<dyn Transport, _>(|_| Ok(Arc::new(Mail)));
    let typed = ServiceKey::typed::<dyn Transport>();
    container.instanceNamed(typed.name(), Arc::new(13_u32));
    assert_eq!(container.make::<dyn Transport>().unwrap().send(), "sent");
    assert_eq!(*container.get::<u32>(typed.name()).unwrap(), 13);
    container.alias(typed.clone(), "transport").unwrap();
    assert!(Arc::ptr_eq(
        &container.make::<dyn Transport>().unwrap(),
        &container.get::<dyn Transport>("transport").unwrap()
    ));
    assert_eq!(typed, typed.clone());
    assert_ne!(typed, ServiceKey::named(typed.name()));
}

#[test]
fn singleton_panic_does_not_poison_resolution_or_prevent_retry() {
    let container = Container::default();
    let calls = AtomicUsize::new(0);
    container.singleton::<u16, _>(move |_| {
        assert_ne!(calls.fetch_add(1, Ordering::SeqCst), 0, "constructor panic");
        Ok(Arc::new(8))
    });
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| container.make::<u16>())).is_err()
    );
    assert_eq!(*container.make::<u16>().unwrap(), 8);
}

#[test]
fn instance_overrides_preserve_the_original_factory_and_raw_forget_identity() {
    let container = Container::default();
    let calls = AtomicUsize::new(0);
    container.singletonNamed::<usize, _>("service", move |_| {
        Ok(Arc::new(calls.fetch_add(1, Ordering::SeqCst)))
    });
    container.alias("service", "alias").unwrap();
    let original = container.get::<usize>("service").unwrap();
    container.instanceNamed("service", Arc::new(99_usize));
    assert_eq!(*container.get::<usize>("alias").unwrap(), 99);
    container.forgetInstance("alias");
    assert_eq!(*container.get::<usize>("service").unwrap(), 99);
    container.forgetInstance("service");
    let next = container.get::<usize>("alias").unwrap();
    assert_eq!(*next, 1);
    assert_eq!(*original, 0);
    assert!(!Arc::ptr_eq(&original, &next));
    container.forgetInstances();
    assert_eq!(*container.get::<usize>("service").unwrap(), 2);
}

#[test]
fn shared_and_resolved_states_distinguish_raw_aliases_instances_and_history() {
    let container = Container::default();
    container.singletonNamed::<u32, _>("shared", |_| Ok(Arc::new(1)));
    container.bindNamed::<u32, _>("transient", |_| Ok(Arc::new(2)));
    container.alias("shared", "alias").unwrap();
    assert!(container.isShared("shared"));
    assert!(!container.isShared("alias"));
    assert!(!container.resolved("alias").unwrap());
    container.get::<u32>("alias").unwrap();
    assert!(container.resolved("shared").unwrap());
    assert!(container.resolved("alias").unwrap());
    container.forgetInstance("shared");
    assert!(container.resolved("alias").unwrap());
    assert!(!container.isShared("transient"));
    assert!(!container.resolved("transient").unwrap());
    container.get::<u32>("transient").unwrap();
    assert!(container.resolved("transient").unwrap());
    container.instanceNamed("provided", Arc::new(3_u32));
    assert!(container.resolved("provided").unwrap());
    assert!(container.isShared("provided"));
    container.get::<u32>("provided").unwrap();
    container.forgetInstances();
    assert!(!container.has("provided"));
    assert!(!container.resolved("provided").unwrap());
    assert!(container.resolved("transient").unwrap());
}

#[test]
fn conditional_bindings_preserve_dangling_aliases_instances_and_cached_singletons() {
    use openrust::container::ServiceKey;
    let container = Container::default();
    container.singletonIf::<u32, _>(|_| Ok(Arc::new(5)));
    let first = container.make::<u32>().unwrap();
    container.bindIf::<u32, _>(|_| panic!("existing binding replaced"));
    assert!(Arc::ptr_eq(&first, &container.make::<u32>().unwrap()));
    container.instance::<u64>(Arc::new(7));
    container.singletonIf::<u64, _>(|_| panic!("existing instance replaced"));
    assert_eq!(*container.make::<u64>().unwrap(), 7);
    container.alias("missing", "alias").unwrap();
    container.bindIfNamed::<u32, _>("alias", |_| panic!("alias replaced"));
    assert_eq!(
        container.getAlias("alias").unwrap(),
        ServiceKey::named("missing")
    );
    assert!(container.get::<u32>("alias").is_err());
    container.bindIf::<u16, _>(|_| Ok(Arc::new(11)));
    assert_eq!(*container.make::<u16>().unwrap(), 11);
    assert!(!Arc::ptr_eq(
        &container.make::<u16>().unwrap(),
        &container.make::<u16>().unwrap()
    ));
}

#[test]
fn concurrent_conditional_registration_selects_one_factory_without_eager_execution() {
    let container = Arc::new(Container::default());
    let gate = Arc::new(Barrier::new(8));
    let executions = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        for value in 0..8 {
            let container = container.clone();
            let gate = gate.clone();
            let executions = executions.clone();
            scope.spawn(move || {
                gate.wait();
                container.singletonIfNamed::<usize, _>("winner", move |_| {
                    executions.fetch_add(1, Ordering::SeqCst);
                    Ok(Arc::new(value))
                });
            });
        }
    });
    assert_eq!(executions.load(Ordering::SeqCst), 0);
    let first = container.get::<usize>("winner").unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &container.get::<usize>("winner").unwrap()
    ));
    assert_eq!(executions.load(Ordering::SeqCst), 1);
}

#[test]
fn rejected_conditional_factories_are_dropped_after_unlocking_the_registry() {
    struct Reenter(std::sync::Weak<Container>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            assert!(self.0.upgrade().unwrap().has("existing"));
        }
    }
    let container = Arc::new(Container::default());
    container.instanceNamed("existing", Arc::new(1_usize));
    let weak = Arc::downgrade(&container);
    let (send, receive) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let owned = Reenter(weak);
        container.bindIfNamed::<usize, _>("existing", move |_| {
            let _ = &owned;
            Ok(Arc::new(2))
        });
        send.send(()).unwrap();
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    worker.join().unwrap();
}
