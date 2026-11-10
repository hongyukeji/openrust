use openrust::{
    container::{
        ClassDescriptor, ConstructorParam, Container, ParameterKey, ParameterOverrides,
        ParameterValue, ServiceKey,
    },
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
trait Filter: Send + Sync {
    fn value(&self) -> u32;
}
struct Base(u32);
impl Filter for Base {
    fn value(&self) -> u32 {
        self.0
    }
}
struct Filters(Vec<Arc<dyn Filter>>);
fn describe(c: &Container) {
    c.registerClass(
        ClassDescriptor::concrete::<Filters, _>(
            vec![ConstructorParam::class::<dyn Filter>("filters").variadic()],
            |args| Ok(Arc::new(Filters(args.services(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
}
fn values(filters: &Filters) -> Vec<u32> {
    filters.0.iter().map(|f| f.value()).collect()
}
#[test]
fn absent_primitive_and_class_variadics_are_empty_and_slot_shapes_are_explicit() {
    let c = Container::default();
    describe(&c);
    assert!(c.make::<Filters>().unwrap().0.is_empty());
    struct Scalars;
    c.registerClass(
        ClassDescriptor::concrete::<Scalars, _>(
            vec![
                ConstructorParam::scalar::<u32>("numbers")
                    .nullable()
                    .variadic(),
            ],
            |args| {
                assert_eq!(args.len(), 1);
                assert!(args.values(0)?.is_empty());
                assert!(matches!(
                    args.value(0),
                    Err(BindingResolutionError::InvalidArgumentShape { index: 0 })
                ));
                Ok(Arc::new(Scalars))
            },
        )
        .unwrap(),
    )
    .unwrap();
    c.make::<Scalars>().unwrap();
}
#[test]
fn concrete_class_variadic_falls_back_to_one_automatic_instance() {
    struct Concrete;
    struct List(Vec<Arc<Concrete>>);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<Concrete, _>(vec![], |_| Ok(Arc::new(Concrete))).unwrap(),
    )
    .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<List, _>(
            vec![ConstructorParam::class::<Concrete>("items").variadic()],
            |args| Ok(Arc::new(List(args.services(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(c.make::<List>().unwrap().0.len(), 1);
}
#[test]
fn service_arrays_preserve_order_duplicates_and_the_original_consumer_context() {
    let c = Container::default();
    describe(&c);
    c.bindNamed::<dyn Filter, _>("first", |_| Ok(Arc::new(Base(1))))
        .unwrap();
    c.bindNamed::<dyn Filter, _>("second", |_| Ok(Arc::new(Base(2))))
        .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needsNamed::<dyn Filter>("first")
        .give(|_| Ok(Arc::new(Base(7))))
        .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["first", "second", "first"])
        .unwrap();
    let before = Arc::new(AtomicUsize::new(0));
    let count = before.clone();
    c.beforeResolving(Some(ServiceKey::typed::<dyn Filter>()), move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    assert_eq!(values(&c.make::<Filters>().unwrap()), [7, 2, 7]);
    assert_eq!(before.load(Ordering::SeqCst), 0);
}
#[test]
fn tagged_collection_expansion_runs_as_a_closure_and_does_not_reuse_the_consumer_for_tagged_items()
{
    let c = Container::default();
    describe(&c);
    c.bindNamed::<dyn Filter, _>("first", |_| Ok(Arc::new(Base(1))))
        .unwrap();
    c.bindNamed::<dyn Filter, _>("second", |_| Ok(Arc::new(Base(2))))
        .unwrap();
    c.tag(["first", "second"], ["filters"]);
    c.when::<Filters>()
        .unwrap()
        .needsNamed::<dyn Filter>("first")
        .give(|_| Ok(Arc::new(Base(7))))
        .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<Vec<Arc<dyn Filter>>>()
        .giveTagged("filters")
        .unwrap();
    assert_eq!(values(&c.make::<Filters>().unwrap()), [1, 2]);
    c.tag(["first"], ["filters"]);
    assert_eq!(values(&c.make::<Filters>().unwrap()), [1, 2, 1]);
}
#[test]
fn primitive_vectors_preserve_arc_identity_order_and_nullable_carriers() {
    struct List(Vec<u32>);
    let c = Container::default();
    let shared = Arc::new(3u32);
    let same = shared.clone();
    c.when::<List>()
        .unwrap()
        .needsNamed::<Vec<Arc<u32>>>("$numbers")
        .give(move |_| Ok(Arc::new(vec![same.clone(), Arc::new(1), Arc::new(2)])))
        .unwrap();
    let expected = shared.clone();
    c.registerClass(
        ClassDescriptor::concrete::<List, _>(
            vec![ConstructorParam::scalar::<u32>("numbers").variadic()],
            move |args| {
                assert!(Arc::ptr_eq(
                    &expected,
                    &args.values(0)?[0].downcast::<u32>()?
                ));
                Ok(Arc::new(List(
                    args.values(0)?
                        .iter()
                        .map(|v| v.downcast::<u32>().map(|n| *n))
                        .collect::<Result<_, _>>()?,
                )))
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(c.make::<List>().unwrap().0, [3, 1, 2]);
    struct Nullable;
    c.when::<Nullable>()
        .unwrap()
        .needsNamed::<Vec<ParameterValue>>("$numbers")
        .give(|_| {
            Ok(Arc::new(vec![
                ParameterValue::new(1u32),
                ParameterValue::null(),
            ]))
        })
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Nullable, _>(
            vec![
                ConstructorParam::scalar::<u32>("numbers")
                    .nullable()
                    .variadic(),
            ],
            |args| {
                assert_eq!(args.values(0)?.len(), 2);
                assert!(args.values(0)?[1].is_null());
                Ok(Arc::new(Nullable))
            },
        )
        .unwrap(),
    )
    .unwrap();
    c.make::<Nullable>().unwrap();
}
#[test]
fn explicit_named_overrides_are_one_argument_and_integer_keys_do_not_replace_the_named_slot() {
    struct List(usize);
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<List, _>(
            vec![ConstructorParam::scalar::<Vec<u32>>("items").variadic()],
            |args| {
                assert_eq!(*args.values(0)?[0].downcast::<Vec<u32>>()?, [3, 1, 2]);
                Ok(Arc::new(List(args.values(0)?.len())))
            },
        )
        .unwrap(),
    )
    .unwrap();
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::name("items"),
        ParameterValue::new(vec![3u32, 1, 2]),
    );
    assert_eq!(c.makeWith::<List>(args).unwrap().0, 1);
    struct Numbers(usize);
    c.registerClass(
        ClassDescriptor::concrete::<Numbers, _>(
            vec![ConstructorParam::scalar::<u32>("items").variadic()],
            |args| Ok(Arc::new(Numbers(args.values(0)?.len()))),
        )
        .unwrap(),
    )
    .unwrap();
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::name("items"),
        ParameterValue::new(vec![1u32, 2]),
    );
    assert!(matches!(
        c.makeWith::<Numbers>(args),
        Err(BindingResolutionError::ParameterTypeMismatch { .. })
    ));
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::Index(0), ParameterValue::new(5u32));
    assert_eq!(c.makeWith::<Numbers>(args).unwrap().0, 0);
}
#[test]
fn explicit_class_overrides_preserve_one_arc_and_nullable_null_is_one_item() {
    let c = Container::default();
    describe(&c);
    let filter: Arc<dyn Filter> = Arc::new(Base(8));
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::name("filters"),
        ParameterValue::new(filter.clone()),
    );
    let list = c.makeWith::<Filters>(args).unwrap();
    assert_eq!(list.0.len(), 1);
    assert!(Arc::ptr_eq(&filter, &list.0[0]));
    struct Nullable;
    c.registerClass(
        ClassDescriptor::concrete::<Nullable, _>(
            vec![
                ConstructorParam::class::<dyn Filter>("filters")
                    .nullable()
                    .variadic(),
            ],
            |args| {
                assert_eq!(args.values(0)?.len(), 1);
                assert!(args.values(0)?[0].is_null());
                Ok(Arc::new(Nullable))
            },
        )
        .unwrap(),
    )
    .unwrap();
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::name("filters"), ParameterValue::null());
    c.makeWith::<Nullable>(args).unwrap();
}
#[test]
fn variadic_signatures_reject_defaults_multiple_slots_and_nonfinal_positions() {
    struct Invalid;
    for signature in [
        vec![
            ConstructorParam::scalar::<u32>("a").variadic(),
            ConstructorParam::scalar::<u32>("b"),
        ],
        vec![
            ConstructorParam::scalar::<u32>("a").variadic(),
            ConstructorParam::scalar::<u32>("b").variadic(),
        ],
        vec![
            ConstructorParam::scalar::<u32>("a")
                .variadic()
                .withDefault(|| panic!("invalid default must not run")),
        ],
    ] {
        assert!(matches!(
            ClassDescriptor::concrete::<Invalid, _>(signature, |_| panic!(
                "invalid constructor must not run"
            )),
            Err(BindingResolutionError::InvalidConstructor { .. })
        ));
    }
}
#[test]
fn missing_class_list_items_empty_the_whole_list_but_business_and_type_errors_propagate() {
    let c = Container::default();
    describe(&c);
    c.bindNamed::<dyn Filter, _>("ok", |_| Ok(Arc::new(Base(1))))
        .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["ok", "missing"])
        .unwrap();
    assert!(c.make::<Filters>().unwrap().0.is_empty());
    c.bindNamed::<dyn Filter, _>("bad", |_| {
        Err(BindingResolutionError::FactoryFailed {
            service: "bad",
            source: std::io::Error::other("private business failure").into(),
        })
    })
    .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["ok", "bad"])
        .unwrap();
    assert!(matches!(
        c.make::<Filters>(),
        Err(BindingResolutionError::FactoryFailed { .. })
    ));
    c.instanceNamed("wrong", Arc::new(String::from("wrong output")))
        .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["wrong"])
        .unwrap();
    assert!(matches!(
        c.make::<Filters>(),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
}
#[tokio::test]
async fn async_service_arrays_wait_for_dependencies_and_keep_the_registration_snapshot() {
    let c = Container::default();
    describe(&c);
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let (a, b) = (entered.clone(), release.clone());
    c.bindAsyncNamed::<dyn Filter, _>("first", move |_| {
        let (a, b) = (a.clone(), b.clone());
        Box::pin(async move {
            a.notify_one();
            b.notified().await;
            Ok(Arc::new(Base(1)) as Arc<dyn Filter>)
        })
    })
    .await
    .unwrap();
    c.bindNamed::<dyn Filter, _>("second", |_| Ok(Arc::new(Base(2))))
        .unwrap();
    c.bindNamed::<dyn Filter, _>("new", |_| Ok(Arc::new(Base(9))))
        .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["first", "second"])
        .unwrap();
    assert!(matches!(
        c.make::<Filters>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    let mut pending = Box::pin(c.makeAsync::<Filters>());
    tokio::select! {value=&mut pending=>panic!("unexpected {}",value.unwrap().0.len()),_=entered.notified()=>{}}
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["new"])
        .unwrap();
    release.notify_one();
    assert_eq!(values(&pending.await.unwrap()), [1, 2]);
    assert_eq!(values(&c.makeAsync::<Filters>().await.unwrap()), [9]);
}
#[test]
fn primitive_context_runs_inline_without_service_events_and_keeps_constructor_parameters() {
    struct Item(u32);
    let c = Container::default();
    let events = Arc::new(AtomicUsize::new(0));
    let count = events.clone();
    c.beforeResolving(Some(ServiceKey::named("$value")), move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })
    .unwrap();
    c.when::<Item>()
        .unwrap()
        .needsNamed::<u32>("$value")
        .give(|c| {
            c.getLastParameterOverride()
                .get(&ParameterKey::name("other"))
                .unwrap()
                .downcast::<u32>()
        })
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![ConstructorParam::scalar::<u32>("value")],
            |args| Ok(Arc::new(Item(*args.get::<u32>(0)?))),
        )
        .unwrap(),
    )
    .unwrap();
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::name("other"), ParameterValue::new(8u32));
    assert_eq!(c.makeWith::<Item>(args).unwrap().0, 8);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    struct Nullable;
    c.when::<Nullable>()
        .unwrap()
        .needsNamed::<ParameterValue>("$value")
        .give(|_| Ok(Arc::new(ParameterValue::null())))
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Nullable, _>(
            vec![ConstructorParam::scalar::<String>("value").nullable()],
            |args| {
                assert!(args.optional::<String>(0)?.is_none());
                Ok(Arc::new(Nullable))
            },
        )
        .unwrap(),
    )
    .unwrap();
    c.make::<Nullable>().unwrap();
}

#[test]
fn missing_nested_primitive_dependencies_empty_class_variadics_but_not_primitive_variadics() {
    struct NeedsNumber;
    struct List(usize);
    struct Numbers;
    let c = Container::default();
    c.registerClass(
        ClassDescriptor::concrete::<NeedsNumber, _>(
            vec![ConstructorParam::scalar::<u32>("number")],
            |_| Ok(Arc::new(NeedsNumber)),
        )
        .unwrap(),
    )
    .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<List, _>(
            vec![ConstructorParam::class::<NeedsNumber>("items").variadic()],
            |args| Ok(Arc::new(List(args.values(0)?.len()))),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(c.make::<List>().unwrap().0, 0);
    c.when::<Numbers>()
        .unwrap()
        .needsNamed::<Vec<Arc<u32>>>("$numbers")
        .give(|_| {
            Err(BindingResolutionError::NotBound {
                service: "explicit primitive failure".into(),
            })
        })
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Numbers, _>(
            vec![ConstructorParam::scalar::<u32>("numbers").variadic()],
            |_| Ok(Arc::new(Numbers)),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        c.make::<Numbers>(),
        Err(BindingResolutionError::NotBound { .. })
    ));
}
#[test]
fn circular_and_closed_lifecycle_failures_are_not_converted_to_empty_class_lists() {
    let c = Container::default();
    describe(&c);
    c.bindNamed::<dyn Filter, _>("cycle", |c| c.get("cycle"))
        .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["cycle"])
        .unwrap();
    assert!(matches!(
        c.make::<Filters>(),
        Err(BindingResolutionError::CircularDependency { .. })
    ));
    c.bindNamed::<dyn Filter, _>("closed", |_| Err(BindingResolutionError::ScopeClosed))
        .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["closed"])
        .unwrap();
    assert!(matches!(
        c.make::<Filters>(),
        Err(BindingResolutionError::ScopeClosed)
    ));
}
#[tokio::test]
async fn asynchronous_primitive_context_waits_inline_and_validates_all_vector_values() {
    struct Item(u32);
    let c = Container::default();
    c.when::<Item>()
        .unwrap()
        .needsNamed::<Vec<ParameterValue>>("$numbers")
        .giveAsync(|c| {
            Box::pin(async move {
                tokio::task::yield_now().await;
                let n = c
                    .getLastParameterOverride()
                    .get(&ParameterKey::name("other"))
                    .unwrap()
                    .downcast::<u32>()?;
                Ok(Arc::new(vec![
                    ParameterValue::new(*n),
                    ParameterValue::new(2u32),
                ]))
            })
        })
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Item, _>(
            vec![ConstructorParam::scalar::<u32>("numbers").variadic()],
            |args| {
                Ok(Arc::new(Item(
                    args.values(0)?
                        .iter()
                        .map(|v| v.downcast::<u32>().map(|v| *v))
                        .collect::<Result<Vec<_>, _>>()?
                        .iter()
                        .sum(),
                )))
            },
        )
        .unwrap(),
    )
    .unwrap();
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::name("other"), ParameterValue::new(8u32));
    assert_eq!(c.makeWithAsync::<Item>(args).await.unwrap().0, 10);
    c.when::<Item>()
        .unwrap()
        .needsNamed::<Vec<ParameterValue>>("$numbers")
        .give(|_| Ok(Arc::new(vec![ParameterValue::new("private invalid value")])))
        .unwrap();
    assert!(matches!(
        c.makeAsync::<Item>().await,
        Err(BindingResolutionError::ParameterTypeMismatch { .. })
    ));
}
#[tokio::test]
async fn cancelling_a_partial_service_array_releases_values_in_the_original_parameter_frame() {
    struct Probe(std::sync::Weak<Container>, Arc<Mutex<Vec<u32>>>);
    impl Filter for Probe {
        fn value(&self) -> u32 {
            1
        }
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            let c = self.0.upgrade().unwrap();
            let value = c
                .getLastParameterOverride()
                .get(&ParameterKey::name("marker"))
                .unwrap()
                .downcast::<u32>()
                .unwrap();
            self.1.lock().unwrap().push(*value);
        }
    }
    let c = Arc::new(Container::default());
    describe(&c);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let weak = Arc::downgrade(&c);
    let log = seen.clone();
    c.bindNamed::<dyn Filter, _>("first", move |_| {
        Ok(Arc::new(Probe(weak.clone(), log.clone())))
    })
    .unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let start = entered.clone();
    c.bindAsyncNamed::<dyn Filter, _>("second", move |_| {
        let start = start.clone();
        Box::pin(async move {
            start.notify_one();
            std::future::pending().await
        })
    })
    .await
    .unwrap();
    c.when::<Filters>()
        .unwrap()
        .needs::<dyn Filter>()
        .giveServices(["first", "second"])
        .unwrap();
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::name("marker"), ParameterValue::new(7u32));
    let mut pending = Box::pin(c.makeWithAsync::<Filters>(args));
    tokio::select! {value=&mut pending=>panic!("unexpected {}",value.unwrap().0.len()),_=entered.notified()=>{}}
    drop(pending);
    assert_eq!(*seen.lock().unwrap(), [7]);
    assert!(c.getLastParameterOverride().is_empty());
}

#[tokio::test]
async fn variadic_primitive_config_arrays_use_the_current_repository() {
    use openrust::contracts::config::Repository as _;
    struct Names(Vec<String>);
    let c = Container::default();
    let config = Arc::new(openrust::config::Repository::new(Default::default()));
    config
        .set("app.names", serde_json::json!(["first", "second"]))
        .unwrap();
    let repository: Arc<dyn openrust::contracts::config::Repository> = config.clone();
    c.instanceNamed("config", repository).unwrap();
    c.when::<Names>()
        .unwrap()
        .needsNamed::<Vec<Arc<String>>>("$names")
        .giveConfig("app.names", None)
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Names, _>(
            vec![ConstructorParam::scalar::<String>("names").variadic()],
            |args| {
                Ok(Arc::new(Names(
                    args.values(0)?
                        .iter()
                        .map(|v| v.downcast::<String>().map(|s| (*s).clone()))
                        .collect::<Result<_, _>>()?,
                )))
            },
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(c.make::<Names>().unwrap().0, ["first", "second"]);
    config
        .set("app.names", serde_json::json!(["updated"]))
        .unwrap();
    assert_eq!(c.makeAsync::<Names>().await.unwrap().0, ["updated"]);
}

#[test]
fn nonclone_scalars_and_parameter_value_objects_keep_their_declared_types() {
    struct Locks(Arc<Mutex<u32>>);
    struct Carriers;
    let c = Container::default();
    let lock = Arc::new(Mutex::new(4));
    let shared = lock.clone();
    c.when::<Locks>()
        .unwrap()
        .needsNamed::<Vec<Arc<Mutex<u32>>>>("$locks")
        .give(move |_| Ok(Arc::new(vec![shared.clone()])))
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Locks, _>(
            vec![ConstructorParam::scalar::<Mutex<u32>>("locks").variadic()],
            |args| Ok(Arc::new(Locks(args.values(0)?[0].downcast()?))),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&lock, &c.make::<Locks>().unwrap().0));
    c.when::<Carriers>()
        .unwrap()
        .needsNamed::<Vec<ParameterValue>>("$items")
        .give(|_| Ok(Arc::new(vec![ParameterValue::new(5u32)])))
        .unwrap();
    c.registerClass(
        ClassDescriptor::concrete::<Carriers, _>(
            vec![ConstructorParam::scalar::<ParameterValue>("items").variadic()],
            |args| {
                let carrier = args.values(0)?[0].downcast::<ParameterValue>()?;
                assert_eq!(*carrier.downcast::<u32>()?, 5);
                Ok(Arc::new(Carriers))
            },
        )
        .unwrap(),
    )
    .unwrap();
    c.make::<Carriers>().unwrap();
}
