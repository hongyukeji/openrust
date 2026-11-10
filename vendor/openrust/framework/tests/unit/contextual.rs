use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Consumer(Arc<String>);
struct Other(Arc<String>);
fn container() -> Container {
    let app = Container::default();
    app.bind::<Consumer, _>(|c| Ok(Arc::new(Consumer(c.make::<String>()?))));
    app.bind::<Other, _>(|c| Ok(Arc::new(Other(c.make::<String>()?))));
    app
}

#[test]
fn contextual_dependencies_bypass_existing_instances_and_do_not_populate_default_state() {
    let app = container();
    app.singleton::<String, _>(|_| Ok(Arc::new("default".into())));
    app.when::<Consumer>()
        .unwrap()
        .needs::<String>()
        .give(|_| Ok(Arc::new("custom".into())))
        .unwrap();
    assert_eq!(&*app.make::<Consumer>().unwrap().0, "custom");
    assert!(!app.resolved(ServiceKey::typed::<String>()).unwrap());
    let default = app.make::<String>().unwrap();
    let one = app.make::<Consumer>().unwrap();
    let two = app.make::<Consumer>().unwrap();
    assert!(!Arc::ptr_eq(&one.0, &two.0));
    assert!(Arc::ptr_eq(&default, &app.make::<Other>().unwrap().0));
    app.instance(Arc::new(String::from("instance")));
    assert_eq!(&*app.make::<Consumer>().unwrap().0, "custom");
    assert_eq!(&*app.make::<String>().unwrap(), "instance");
}

#[test]
fn contextual_factory_can_delegate_to_the_default_binding_of_the_same_dependency() {
    let app = container();
    app.singleton::<String, _>(|_| Ok(Arc::new("default".into())));
    app.when::<Consumer>()
        .unwrap()
        .needs::<String>()
        .give(|c| c.make::<String>())
        .unwrap();
    let consumer = app.make::<Consumer>().unwrap();
    assert!(Arc::ptr_eq(&consumer.0, &app.make::<String>().unwrap()));
    app.bind::<String, _>(|c| c.make::<String>());
    assert!(matches!(
        app.make::<Consumer>(),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
}

#[test]
fn nearest_consumer_wins_without_inheriting_an_ancestors_rule() {
    struct Outer(Arc<Other>, Arc<String>);
    let app = container();
    app.bind::<String, _>(|_| Ok(Arc::new("default".into())));
    app.bind::<Outer, _>(|c| Ok(Arc::new(Outer(c.make()?, c.make()?))));
    app.when::<Outer>()
        .unwrap()
        .needs::<String>()
        .give(|_| Ok(Arc::new("outer".into())))
        .unwrap();
    let result = app.make::<Outer>().unwrap();
    assert_eq!(&*result.0.0, "default");
    assert_eq!(&*result.1, "outer");
    app.when::<Other>()
        .unwrap()
        .needs::<String>()
        .give(|_| Ok(Arc::new("inner".into())))
        .unwrap();
    assert_eq!(&*app.make::<Outer>().unwrap().0.0, "inner");
}

#[test]
fn multiple_consumers_and_aliased_service_implementations_preserve_identity() {
    let app = container();
    app.alias(ServiceKey::typed::<Consumer>(), "consumer")
        .unwrap();
    app.instanceNamed("implementation", Arc::new(String::from("shared")));
    app.alias("implementation", "implementation-alias").unwrap();
    app.whenMany([ServiceKey::named("consumer"), ServiceKey::typed::<Other>()])
        .unwrap()
        .needs::<String>()
        .giveService("implementation-alias")
        .unwrap();
    let one = app.get::<Consumer>("consumer").unwrap();
    let two = app.make::<Other>().unwrap();
    assert!(Arc::ptr_eq(&one.0, &two.0));
    assert!(!app.bound::<String>());
    assert!(!app.resolved(ServiceKey::typed::<String>()).unwrap());
    assert!(app.make::<String>().is_err());
}

#[test]
fn late_aliases_are_followed_and_retargeted_aliases_do_not_leave_stale_contexts() {
    let app = Container::default();
    app.bindNamed::<String, _>("consumer", |c| c.get("dependency"));
    app.whenNamed("consumer")
        .unwrap()
        .needsNamed::<String>("stale")
        .give(|_| Ok(Arc::new("stale".into())))
        .unwrap();
    app.whenNamed("consumer")
        .unwrap()
        .needsNamed::<String>("live")
        .give(|_| Ok(Arc::new("live".into())))
        .unwrap();
    app.alias("dependency", "stale").unwrap();
    app.alias("unrelated", "stale").unwrap();
    app.alias("dependency", "live").unwrap();
    assert_eq!(&*app.get::<String>("consumer").unwrap(), "live");
    app.bindNamed::<String, _>("live", |_| Ok(Arc::new("raw".into())));
    assert!(app.get::<String>("consumer").is_err());

    app.whenNamed("consumer")
        .unwrap()
        .needsNamed::<String>("dependency")
        .give(|_| Ok(Arc::new("canonical".into())))
        .unwrap();
    app.alias("target", "dependency").unwrap();
    assert_eq!(&*app.get::<String>("consumer").unwrap(), "canonical");
}

#[test]
fn wrong_contextual_output_type_is_rejected_before_executing_the_factory() {
    let app = container();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    app.addContextualBinding(
        ServiceKey::typed::<Consumer>(),
        ServiceKey::typed::<String>(),
        move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(42_u32))
        },
    )
    .unwrap();
    assert!(matches!(
        app.make::<Consumer>(),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn error_and_panic_unwind_the_consumer_stack_and_allow_retries() {
    let app = container();
    let calls = AtomicUsize::new(0);
    app.bind::<String, _>(|_| Ok(Arc::new("default".into())));
    app.when::<Consumer>()
        .unwrap()
        .needs::<String>()
        .give(move |_| match calls.fetch_add(1, Ordering::SeqCst) {
            0 => Err(BindingResolutionError::NotBound {
                service: "missing".into(),
            }),
            1 => panic!("test panic"),
            _ => Ok(Arc::new("custom".into())),
        })
        .unwrap();
    assert!(app.make::<Consumer>().is_err());
    assert_eq!(&*app.make::<String>().unwrap(), "default");
    assert!(catch_unwind(AssertUnwindSafe(|| app.make::<Consumer>())).is_err());
    assert_eq!(&*app.make::<String>().unwrap(), "default");
    assert_eq!(&*app.make::<Consumer>().unwrap().0, "custom");
}

#[tokio::test]
async fn contextual_factories_bypass_scoped_overrides_without_replacing_them() {
    let app = container();
    app.scoped::<String, _>(|_| Ok(Arc::new("scoped".into())));
    app.when::<Consumer>()
        .unwrap()
        .needs::<String>()
        .give(|_| Ok(Arc::new("custom".into())))
        .unwrap();
    app.scope(async {
        let default = app.make::<String>().unwrap();
        let custom = app.make::<Consumer>().unwrap();
        assert_eq!(&*custom.0, "custom");
        assert!(Arc::ptr_eq(&default, &app.make::<String>().unwrap()));
        app.instance(Arc::new(String::from("override")));
        assert_eq!(&*app.make::<Consumer>().unwrap().0, "custom");
        assert_eq!(&*app.make::<String>().unwrap(), "override");
    })
    .await;
}

#[test]
fn configuration_is_read_lazily_with_missing_defaults_nulls_and_checked_types() {
    struct Configured(Arc<String>, Arc<Option<String>>);
    let app = Container::default();
    use openrust::{contracts::foundation::Application as _, foundation::Application};
    let application = Application::configure(std::env::current_dir().unwrap())
        .create()
        .unwrap();
    let repo = application
        .container()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .unwrap();
    app.instanceNamed::<dyn openrust::contracts::config::Repository>("config", repo.clone());
    app.bind::<Configured, _>(|c| Ok(Arc::new(Configured(c.get("$name")?, c.get("$optional")?))));
    app.when::<Configured>()
        .unwrap()
        .needsNamed::<String>("$name")
        .giveConfig("app.name", Some("fallback".into()))
        .unwrap()
        .needsNamed::<Option<String>>("$optional")
        .giveConfig("app.optional", Some("fallback".into()))
        .unwrap();
    assert_eq!(&*app.make::<Configured>().unwrap().0, "fallback");
    repo.set("app.name", "current".into()).unwrap();
    repo.set("app.optional", serde_json::Value::Null).unwrap();
    let resolved = app.make::<Configured>().unwrap();
    assert_eq!(&*resolved.0, "current");
    assert_eq!(*resolved.1, None);
    repo.set("app.name", 42.into()).unwrap();
    assert!(matches!(
        app.make::<Configured>(),
        Err(BindingResolutionError::FactoryFailed { .. })
    ));
}

#[test]
fn tags_are_lazy_rewindable_ordered_and_preserve_duplicates_and_late_registration() {
    let app = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    app.bindNamed::<usize, _>("first", move |_| {
        Ok(Arc::new(count.fetch_add(1, Ordering::SeqCst)))
    });
    app.instanceNamed("second", Arc::new(99_usize));
    let absent = app.tagged::<usize>("services");
    app.tag(["first"], ["services"]);
    let tagged = app.tagged::<usize>("services");
    assert_eq!(tagged.len(), 1);
    app.tag(["second", "first"], ["services"]);
    assert_eq!(tagged.len(), 1); // Laravel caches count separately from repeat iteration.
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(absent.is_empty());
    let values: Vec<_> = tagged.iter().map(|result| *result.unwrap()).collect();
    assert_eq!(values, [0, 99, 1]);
    assert_eq!(
        tagged
            .iter()
            .map(|result| *result.unwrap())
            .collect::<Vec<_>>(),
        [2, 99, 3]
    );
    app.bindNamed::<Vec<Arc<usize>>, _>("consumer", |c| c.get("$services"));
    app.whenNamed("consumer")
        .unwrap()
        .needsNamed::<Vec<Arc<usize>>>("$services")
        .giveTagged("services")
        .unwrap();
    assert_eq!(
        app.get::<Vec<Arc<usize>>>("consumer")
            .unwrap()
            .iter()
            .map(|v| **v)
            .collect::<Vec<_>>(),
        [4, 99, 5]
    );
    app.tag(["missing"], ["services"]);
    assert!(app.get::<Vec<Arc<usize>>>("consumer").is_err());
}

#[test]
fn replacing_a_contextual_factory_drops_captures_outside_the_registry_lock() {
    struct Reenter(Arc<Container>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            self.0.instance(Arc::new(42_u32));
        }
    }
    let app = Arc::new(container());
    let capture = Reenter(app.clone());
    app.when::<Consumer>()
        .unwrap()
        .needs::<String>()
        .give(move |_| {
            let _ = &capture;
            Ok(Arc::new("old".into()))
        })
        .unwrap();
    app.when::<Consumer>()
        .unwrap()
        .needs::<String>()
        .give(|_| Ok(Arc::new("new".into())))
        .unwrap();
    assert_eq!(*app.make::<u32>().unwrap(), 42);
}

#[test]
fn concurrent_threads_and_nested_containers_do_not_share_consumers() {
    let first = Arc::new(container());
    let second = Arc::new(container());
    for (app, value) in [(&first, "first"), (&second, "second")] {
        app.when::<Consumer>()
            .unwrap()
            .needs::<String>()
            .give(move |_| Ok(Arc::new(value.into())))
            .unwrap();
    }
    let nested = second.clone();
    first.bind::<Other, _>(move |_| Ok(Arc::new(Other(nested.make::<Consumer>()?.0.clone()))));
    let gate = Arc::new(std::sync::Barrier::new(4));
    std::thread::scope(|scope| {
        for app in [first.clone(), second.clone(), first.clone(), second.clone()] {
            let gate = gate.clone();
            let expected = if Arc::ptr_eq(&app, &first) {
                "first"
            } else {
                "second"
            };
            scope.spawn(move || {
                gate.wait();
                for _ in 0..50 {
                    assert_eq!(&*app.make::<Consumer>().unwrap().0, expected);
                }
            });
        }
    });
    assert_eq!(&*first.make::<Other>().unwrap().0, "second");
    assert_eq!(&*first.make::<Consumer>().unwrap().0, "first");
}
