use openrust::{
    container::{
        ClassDescriptor, ConstructorParam, Container, ParameterKey, ParameterOverrides,
        ParameterValue, ServiceKey,
    },
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn arguments(value: u32) -> ParameterOverrides {
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::name("number"), ParameterValue::new(value));
    args
}
#[test]
fn metadata_is_not_a_binding_and_typed_and_aliased_construction_share_history() {
    struct Item(u32);
    let c = Container::default();
    let descriptor = ClassDescriptor::concrete::<Item, _>(
        vec![
            ConstructorParam::scalar::<u32>("number").withDefault(|| Ok(ParameterValue::new(3u32))),
        ],
        |args| Ok(Arc::new(Item(*args.get::<u32>(0)?))),
    )
    .unwrap();
    c.registerClass(descriptor.clone()).unwrap();
    c.registerClass(descriptor).unwrap();
    assert!(!c.bound::<Item>());
    assert!(!c.resolved(ServiceKey::typed::<Item>()).unwrap());
    c.alias(ServiceKey::typed::<Item>(), "item").unwrap();
    assert_eq!(c.get::<Item>("item").unwrap().0, 3);
    assert!(!c.bound::<Item>());
    assert!(c.resolved(ServiceKey::typed::<Item>()).unwrap());
    assert!(matches!(
        c.get::<String>(ServiceKey::typed::<Item>()),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    assert_eq!(c.makeWith::<Item>(arguments(9)).unwrap().0, 9);
}
#[tokio::test]
async fn synchronous_and_asynchronous_resolution_inject_registered_concrete_dependencies() {
    struct Dependency;
    struct Item(Arc<Dependency>);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<Dependency, _>(vec![], |_| Ok(Arc::new(Dependency))).unwrap(),
    )
    .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![ConstructorParam::class::<Dependency>("dependency")],
            |args| Ok(Arc::new(Item(args.service(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    let a = c.make::<Item>().unwrap();
    let b = c.makeAsync::<Item>().await.unwrap();
    assert!(!Arc::ptr_eq(&a.0, &b.0));
    assert!(!c.bound::<Item>());
    assert!(!c.bound::<Dependency>());
    assert!(c.resolved(ServiceKey::typed::<Dependency>()).unwrap());
}
#[test]
fn primitive_context_overrides_defaults_and_explicit_parameters_override_context() {
    struct Item(u32);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![
                ConstructorParam::scalar::<u32>("number")
                    .withDefault(|| Ok(ParameterValue::new(1u32))),
            ],
            |args| Ok(Arc::new(Item(*args.get::<u32>(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    c.instanceNamed("$number", Arc::new(99u32)).unwrap();
    assert_eq!(c.make::<Item>().unwrap().0, 1);
    c.when::<Item>()
        .unwrap()
        .needsNamed::<u32>("$number")
        .give(|_| Ok(Arc::new(4)))
        .unwrap();
    assert_eq!(c.make::<Item>().unwrap().0, 4);
    assert_eq!(c.makeWith::<Item>(arguments(7)).unwrap().0, 7);
}
#[test]
fn class_defaults_win_over_unbound_metadata_but_not_explicit_or_direct_contextual_bindings() {
    struct Dependency(u32);
    struct Item(Arc<Dependency>);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<Dependency, _>(vec![], |_| Ok(Arc::new(Dependency(1))))
            .unwrap(),
    )
    .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![
                ConstructorParam::class::<Dependency>("dep")
                    .withDefault(|| Ok(ParameterValue::new(Arc::new(Dependency(2))))),
            ],
            |args| Ok(Arc::new(Item(args.service(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(c.make::<Item>().unwrap().0.0, 2);
    c.when::<Item>()
        .unwrap()
        .needs::<Dependency>()
        .give(|_| Ok(Arc::new(Dependency(3))))
        .unwrap();
    assert_eq!(c.make::<Item>().unwrap().0.0, 3);
    struct Other(Arc<Dependency>);
    c.registerClass(
        ClassDescriptor::concrete::<Other, _>(
            vec![
                ConstructorParam::class::<Dependency>("dep")
                    .withDefault(|| Ok(ParameterValue::new(Arc::new(Dependency(2))))),
            ],
            |args| Ok(Arc::new(Other(args.service(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    c.instance(Arc::new(Dependency(4))).unwrap();
    assert_eq!(c.make::<Other>().unwrap().0.0, 4);
}
#[test]
fn nullable_classes_still_resolve_and_null_defaults_and_explicit_null_are_distinct() {
    struct Missing;
    struct Item(Option<Arc<Missing>>);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![ConstructorParam::class::<Missing>("dep").nullable()],
            |args| Ok(Arc::new(Item(args.optionalService(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        c.make::<Item>(),
        Err(BindingResolutionError::NotBound { .. })
    ));
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::name("dep"), ParameterValue::null());
    assert!(c.makeWith::<Item>(args).unwrap().0.is_none());
    struct Optional;
    c.registerClass(
        ClassDescriptor::concrete::<Optional, _>(
            vec![
                ConstructorParam::class::<Missing>("dep")
                    .nullable()
                    .withDefault(|| Ok(ParameterValue::null())),
                ConstructorParam::scalar::<String>("title").nullable(),
            ],
            |args| {
                assert!(args.optionalService::<Missing>(0)?.is_none());
                assert!(args.optional::<String>(1)?.is_none());
                Ok(Arc::new(Optional))
            },
        )
        .unwrap(),
    )
    .unwrap();
    c.make::<Optional>().unwrap();
}
#[test]
fn invalid_signatures_and_duplicate_definitions_fail_before_construction() {
    struct Item;
    let c = Container::default();
    for names in [vec![""], vec!["number", "number"]] {
        assert!(matches!(
            ClassDescriptor::concrete::<Item, _>(
                names
                    .into_iter()
                    .map(ConstructorParam::scalar::<u32>)
                    .collect(),
                |_| Ok(Arc::new(Item))
            ),
            Err(BindingResolutionError::InvalidConstructor { .. })
        ));
    }
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(vec![], |args| {
            assert!(args.is_empty());
            assert!(matches!(
                args.value(0),
                Err(BindingResolutionError::InvalidArgumentSlot { index: 0 })
            ));
            Ok(Arc::new(Item))
        })
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        c.registerClass(
            ClassDescriptor::concrete::<Item, _>(vec![], |_| panic!("duplicate must not replace"))
                .unwrap()
        ),
        Err(BindingResolutionError::InvalidConstructor { .. })
    ));
    c.make::<Item>().unwrap();
}
#[test]
fn build_bypasses_the_targets_cache_decorators_and_events_but_preserves_dependency_events() {
    struct Item(u32);
    struct Dependency;
    let c = Container::default();
    let seen = Arc::new(AtomicUsize::new(0));
    let n = seen.clone();
    let dependency_events = Arc::new(AtomicUsize::new(0));
    let events = dependency_events.clone();
    c.bind::<Dependency, _>(|_| Ok(Arc::new(Dependency)))
        .unwrap();
    c.resolving(Some(ServiceKey::typed::<Dependency>()), move |_, _| {
        events.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![ConstructorParam::class::<Dependency>("dependency")],
            |args| {
                let _dependency = args.service::<Dependency>(0)?;
                Ok(Arc::new(Item(1)))
            },
        )
        .unwrap(),
    )
    .unwrap();
    c.resolving(Some(ServiceKey::typed::<Item>()), move |_, _| {
        n.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    c.extend::<Item, _>(ServiceKey::typed::<Item>(), |v, _| {
        Ok(Arc::new(Item(v.0 + 10)))
    })
    .unwrap();
    c.singleton::<Item, _>(|c| c.build()).unwrap();
    let built = c.build::<Item>().unwrap();
    assert_eq!(built.0, 1);
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert_eq!(dependency_events.load(Ordering::SeqCst), 1);
    assert!(!c.resolved(ServiceKey::typed::<Item>()).unwrap());
    let normal = c.make::<Item>().unwrap();
    assert_eq!(normal.0, 11);
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&normal, &c.make::<Item>().unwrap()));
    assert_eq!(c.build::<Item>().unwrap().0, 1);
    assert_eq!(dependency_events.load(Ordering::SeqCst), 3);
}
#[test]
fn named_overrides_do_not_flow_into_a_nested_constructors_parameter_frame() {
    struct Dependency(u32);
    struct Item(u32, Arc<Dependency>);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<Dependency, _>(
            vec![
                ConstructorParam::scalar::<u32>("number")
                    .withDefault(|| Ok(ParameterValue::new(0u32))),
            ],
            |args| Ok(Arc::new(Dependency(*args.get::<u32>(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![
                ConstructorParam::scalar::<u32>("number"),
                ConstructorParam::class::<Dependency>("dep"),
            ],
            |args| Ok(Arc::new(Item(*args.get::<u32>(0)?, args.service(1)?))),
        )
        .unwrap(),
    )
    .unwrap();
    let value = c.makeWith::<Item>(arguments(8)).unwrap();
    assert_eq!((value.0, value.1.0), (8, 0));
    assert!(!c.resolved(ServiceKey::typed::<Item>()).unwrap());
    assert!(c.resolved(ServiceKey::typed::<Dependency>()).unwrap());
}
#[test]
fn arguments_are_selected_in_order_and_validated_before_the_constructor_runs() {
    struct Dependency;
    struct Item;
    let c = Container::default();
    let count = Arc::new(AtomicUsize::new(0));
    let n = count.clone();
    c.bind::<Dependency, _>(move |_| {
        n.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(Dependency))
    })
    .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![
                ConstructorParam::scalar::<u32>("number"),
                ConstructorParam::class::<Dependency>("dep"),
            ],
            |_| panic!("invalid arguments must not reach constructor"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut args = arguments(1);
    args.insert(
        ParameterKey::name("number"),
        ParameterValue::new("private wrong type"),
    );
    assert!(matches!(
        c.makeWith::<Item>(args),
        Err(BindingResolutionError::ParameterTypeMismatch { .. })
    ));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(matches!(
        c.make::<Item>(),
        Err(BindingResolutionError::UnresolvableParameter {
            parameter: "number",
            ..
        })
    ));
}
#[tokio::test]
async fn asynchronous_constructors_and_dependencies_require_async_resolution_and_keep_scoped_lifetimes()
 {
    struct Dependency;
    struct Item(Arc<Dependency>);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concreteAsync::<Dependency, _>(vec![], |_| {
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(Dependency))
            })
        })
        .unwrap(),
    )
    .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![ConstructorParam::class::<Dependency>("dep")],
            |args| Ok(Arc::new(Item(args.service(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        c.make::<Item>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    c.scopedAsync::<Item, _>(|c| Box::pin(async move { c.buildAsync().await }))
        .await
        .unwrap();
    let first = c
        .scope(async {
            let a = c.makeAsync::<Item>().await.unwrap();
            assert!(Arc::ptr_eq(&a, &c.makeAsync::<Item>().await.unwrap()));
            a
        })
        .await;
    let second = c.scope(c.makeAsync::<Item>()).await.unwrap();
    assert!(!Arc::ptr_eq(&first.0, &second.0));
}
#[tokio::test]
async fn construction_cycles_and_failures_leave_metadata_retryable() {
    struct Item;
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(vec![ConstructorParam::class::<Item>("self")], |_| {
            Ok(Arc::new(Item))
        })
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        c.make::<Item>(),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
    assert!(matches!(
        c.makeAsync::<Item>().await,
        Err(BindingResolutionError::CircularDependency { .. })
    ));
    struct Retry;
    let count = Arc::new(AtomicUsize::new(0));
    let n = count.clone();
    c.registerClass(
        ClassDescriptor::concrete::<Retry, _>(vec![], move |_| {
            if n.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(BindingResolutionError::FactoryFailed {
                    service: "retry",
                    source: std::io::Error::other("failure").into(),
                });
            }
            Ok(Arc::new(Retry))
        })
        .unwrap(),
    )
    .unwrap();
    assert!(c.makeAsync::<Retry>().await.is_err());
    assert!(!c.resolved(ServiceKey::typed::<Retry>()).unwrap());
    c.makeAsync::<Retry>().await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
}
#[test]
fn rejected_descriptor_captures_are_released_outside_the_registry_lock() {
    struct Item;
    struct Probe(std::sync::Weak<Container>, Arc<AtomicUsize>);
    impl Drop for Probe {
        fn drop(&mut self) {
            assert!(!self.0.upgrade().unwrap().bound::<Item>());
            self.1.fetch_add(1, Ordering::SeqCst);
        }
    }
    let c = Arc::new(Container::default());
    let count = Arc::new(AtomicUsize::new(0));
    c.registerClass(ClassDescriptor::concrete::<Item, _>(vec![], |_| Ok(Arc::new(Item))).unwrap())
        .unwrap();
    let probe = Probe(Arc::downgrade(&c), count.clone());
    let duplicate = ClassDescriptor::concrete::<Item, _>(vec![], move |_| {
        let _keep = &probe;
        Ok(Arc::new(Item))
    })
    .unwrap();
    assert!(c.registerClass(duplicate).is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn trait_dependencies_and_explicit_service_overrides_keep_the_original_arc() {
    trait Service: Send + Sync {
        fn value(&self) -> u32;
    }
    struct Base(u32);
    impl Service for Base {
        fn value(&self) -> u32 {
            self.0
        }
    }
    struct Item(Arc<dyn Service>);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![ConstructorParam::class::<dyn Service>("service")],
            |args| Ok(Arc::new(Item(args.service(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    c.bind::<dyn Service, _>(|_| Ok(Arc::new(Base(2)))).unwrap();
    assert_eq!(c.make::<Item>().unwrap().0.value(), 2);
    let shared: Arc<dyn Service> = Arc::new(Base(9));
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::name("service"),
        ParameterValue::new(shared.clone()),
    );
    assert!(Arc::ptr_eq(&shared, &c.makeWith::<Item>(args).unwrap().0));
}
#[test]
fn a_constructor_that_registers_a_new_binding_does_not_mark_the_new_recipe_resolved() {
    struct Item(u32);
    let c = Arc::new(Container::default());
    let weak = Arc::downgrade(&c);
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(vec![], move |_| {
            weak.upgrade()
                .unwrap()
                .bind::<Item, _>(|_| Ok(Arc::new(Item(2))))?;
            Ok(Arc::new(Item(1)))
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(c.make::<Item>().unwrap().0, 1);
    assert!(!c.resolved(ServiceKey::typed::<Item>()).unwrap());
    assert_eq!(c.make::<Item>().unwrap().0, 2);
    assert!(c.resolved(ServiceKey::typed::<Item>()).unwrap());
}
#[test]
fn defaults_are_lazy_fresh_and_alias_context_does_not_override_class_default_selection() {
    struct Dependency;
    struct Item(Arc<Dependency>);
    let c = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let n = calls.clone();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![
                ConstructorParam::class::<Dependency>("dep").withDefault(move || {
                    n.fetch_add(1, Ordering::SeqCst);
                    Ok(ParameterValue::new(Arc::new(Dependency)))
                }),
            ],
            |args| Ok(Arc::new(Item(args.service(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    c.when::<Item>()
        .unwrap()
        .needsNamed::<Dependency>("dependency-alias")
        .give(|_| panic!("alias-only context must not override a class default"))
        .unwrap();
    c.alias(ServiceKey::typed::<Dependency>(), "dependency-alias")
        .unwrap();
    let a = c.make::<Item>().unwrap();
    let b = c.make::<Item>().unwrap();
    assert!(!Arc::ptr_eq(&a.0, &b.0));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::name("dep"),
        ParameterValue::new(Arc::new(Dependency)),
    );
    c.makeWith::<Item>(args).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn asynchronous_contextual_aliases_are_normalized_before_default_selection() {
    struct Dependency(u32);
    struct Item(Arc<Dependency>);
    let c = Container::default();
    c.alias(ServiceKey::typed::<Dependency>(), "dependency")
        .unwrap();
    c.when::<Item>()
        .unwrap()
        .needsNamed::<Dependency>("dependency")
        .giveAsync(|_| Box::pin(async { Ok(Arc::new(Dependency(8))) }))
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![
                ConstructorParam::class::<Dependency>("dep")
                    .withDefault(|| Ok(ParameterValue::new(Arc::new(Dependency(1))))),
            ],
            |args| Ok(Arc::new(Item(args.service(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(c.makeAsync::<Item>().await.unwrap().0.0, 8);
}

#[tokio::test]
async fn described_contracts_validate_extenders_and_async_rebinding_uses_the_async_constructor() {
    struct Item(u32);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concreteAsync::<Item, _>(vec![], |_| {
            Box::pin(async {
                tokio::task::yield_now().await;
                Ok(Arc::new(Item(1)))
            })
        })
        .unwrap(),
    )
    .unwrap();
    let key = ServiceKey::typed::<Item>();
    assert!(matches!(
        c.extend::<String, _>(key.clone(), |_, _| panic!("wrong contract")),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    let seen = Arc::new(AtomicUsize::new(0));
    let n = seen.clone();
    assert!(
        c.rebindingAsync::<Item, _>(key.clone(), move |_, value| {
            n.store(value.0 as usize, Ordering::SeqCst);
            Ok(())
        })
        .await
        .unwrap()
        .is_none()
    );
    c.makeAsync::<Item>().await.unwrap();
    c.extendAsync::<Item, _>(key, |value, _| Ok(Arc::new(Item(value.0 + 2))))
        .await
        .unwrap();
    assert_eq!(seen.load(Ordering::SeqCst), 3);
}
