use openrust::{
    container::{
        Bind, BindWhen, ClassAttribute, ClassDescriptor, ConstructorParam, Container,
        FactoryFuture, Lifetime, ParameterKey, ParameterOverrides, ParameterValue, ResolvedArgs,
        ServiceKey,
    },
    contracts::container::{BindingResolutionError, Container as _, SelfBuilding},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Item(u32);
struct Marker;

#[tokio::test]
async fn singleton_class_shares_both_modes_but_build_and_parameter_overrides_bypass_cache() {
    let container = Container::default();
    let callbacks = Arc::new(AtomicUsize::new(0));
    container.after_resolving_attribute::<Marker>({
        let callbacks = callbacks.clone();
        move |event, _| {
            assert!(event.parameter.is_none());
            callbacks.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    });
    container
        .register_class(
            ClassDescriptor::concrete(
                vec![
                    ConstructorParam::scalar::<u32>("number")
                        .with_default(|| Ok(ParameterValue::new(1_u32))),
                ],
                |args| Ok(Arc::new(Item(*args.get::<u32>(0)?))),
            )
            .unwrap()
            .with_attribute(ClassAttribute::marker(|| Ok(Marker)))
            .with_lifetime(Lifetime::Singleton)
            .with_lifetime(Lifetime::Scoped),
        )
        .unwrap();

    assert!(container.is_shared(ServiceKey::typed::<Item>()));
    assert!(!container.bound::<Item>());
    let first = container.make::<Item>().unwrap();
    assert!(container.bound::<Item>());
    assert!(Arc::ptr_eq(
        &first,
        &container.make_async::<Item>().await.unwrap()
    ));
    assert!(Arc::ptr_eq(
        &first,
        &container
            .scope(container.make_async::<Item>())
            .await
            .unwrap()
    ));
    assert!(container.get_bindings().is_empty());

    let mut overrides = ParameterOverrides::new();
    overrides.insert(ParameterKey::name("number"), ParameterValue::new(9_u32));
    assert_eq!(
        container
            .make_with_async::<Item>(overrides)
            .await
            .unwrap()
            .0,
        9
    );
    assert!(!Arc::ptr_eq(&first, &container.build::<Item>().unwrap()));
    assert_eq!(callbacks.load(Ordering::Relaxed), 3);
    container.forget_instance(ServiceKey::typed::<Item>());
    assert!(!Arc::ptr_eq(
        &first,
        &container.make_async::<Item>().await.unwrap()
    ));
    container.flush();
    assert!(container.is_shared(ServiceKey::typed::<Item>()));
    assert!(!container.bound::<Item>());
}

trait Reporter: Send + Sync {}
struct Report;
impl Reporter for Report {}

#[tokio::test]
async fn bind_and_bind_when_share_at_the_declared_service_lifetime() {
    let container = Container::default();
    container.resolve_environment_using(Some(Arc::new(|_| Ok(true.into()))));
    container
        .register_class(ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Report))).unwrap())
        .unwrap();
    container.register_lifetime::<dyn Reporter>(Lifetime::Scoped);
    let bind = || Bind::new::<dyn Reporter, Report>(|value| value);
    container
        .register_binding_attributes::<dyn Reporter>(vec![
            BindWhen::new(bind(), |_| Ok(false)).into(),
            bind().for_environments(["testing"]).into(),
        ])
        .unwrap();

    let first = container
        .scope(async {
            let first = container.make_async::<dyn Reporter>().await.unwrap();
            assert!(Arc::ptr_eq(
                &first,
                &container.make::<dyn Reporter>().unwrap()
            ));
            container.forget_scoped_instances();
            assert!(!Arc::ptr_eq(
                &first,
                &container.make_async::<dyn Reporter>().await.unwrap()
            ));
            first
        })
        .await;
    let second = container
        .scope(container.make_async::<dyn Reporter>())
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    let first = container.make::<dyn Reporter>().unwrap();
    container.forget_scoped_instances();
    assert!(!Arc::ptr_eq(
        &first,
        &container.make::<dyn Reporter>().unwrap()
    ));

    let next = Container::default();
    next.register_class(ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Report))).unwrap())
        .unwrap();
    next.register_lifetime::<dyn Reporter>(Lifetime::Singleton);
    next.register_binding_attributes::<dyn Reporter>(vec![
        BindWhen::new(bind(), |_| Ok(true)).into(),
    ])
    .unwrap();
    let (a, b) = tokio::join!(
        next.make_async::<dyn Reporter>(),
        next.make_async::<dyn Reporter>()
    );
    assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
}

#[tokio::test]
async fn dual_mode_shared_class_can_resolve_async_dependencies_and_retry() {
    let container = Container::default();
    let attempts = Arc::new(AtomicUsize::new(0));
    container
        .bind_async::<u32, _>({
            let attempts = attempts.clone();
            move |_| {
                let attempts = attempts.clone();
                Box::pin(async move {
                    tokio::task::yield_now().await;
                    if attempts.fetch_add(1, Ordering::Relaxed) == 0 {
                        return Err(BindingResolutionError::NotBound {
                            service: "first attempt".into(),
                        });
                    }
                    Ok(Arc::new(8))
                })
            }
        })
        .await
        .unwrap();
    container
        .register_class(
            ClassDescriptor::concrete(vec![ConstructorParam::class::<u32>("number")], |args| {
                Ok(Arc::new(Item(*args.service::<u32>(0)?)))
            })
            .unwrap()
            .with_lifetime(Lifetime::Singleton),
        )
        .unwrap();
    assert!(matches!(
        container.make::<Item>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    assert!(container.make_async::<Item>().await.is_err());
    let (a, b) = tokio::join!(
        container.make_async::<Item>(),
        container.make_async::<Item>()
    );
    let a = a.unwrap();
    assert_eq!(a.0, 8);
    assert!(Arc::ptr_eq(&a, &b.unwrap()));
    assert!(Arc::ptr_eq(&a, &container.make::<Item>().unwrap()));
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
}

impl SelfBuilding for Item {
    fn new_instance(_: ResolvedArgs, container: &Container) -> FactoryFuture<'_, Self> {
        Box::pin(async move { Ok(Arc::new(Self(container.make_async::<Self>().await?.0 + 1))) })
    }
}

#[tokio::test]
async fn singleton_self_building_reentry_uses_the_ordinary_constructor() {
    let container = Container::default();
    container
        .register_class(
            ClassDescriptor::self_building::<Item>(vec![], |_| Ok(Arc::new(Item(3))), vec![])
                .unwrap()
                .with_lifetime(Lifetime::Singleton),
        )
        .unwrap();
    let (a, b) = tokio::join!(
        container.make_async::<Item>(),
        container.make_async::<Item>()
    );
    let a = a.unwrap();
    assert_eq!(a.0, 4);
    assert!(Arc::ptr_eq(&a, &b.unwrap()));
}

#[test]
fn explicit_binding_of_annotated_class_uses_its_declared_lifetime() {
    let container = Container::default();
    container
        .bind::<Item, _>(|_| Ok(Arc::new(Item(5))))
        .unwrap();
    container
        .register_class(
            ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Item(7))))
                .unwrap()
                .with_lifetime(Lifetime::Singleton),
        )
        .unwrap();
    let first = container.make::<Item>().unwrap();
    assert_eq!(first.0, 5);
    assert!(Arc::ptr_eq(&first, &container.make::<Item>().unwrap()));
    container.offset_unset(ServiceKey::typed::<Item>());
    let automatic = container.make::<Item>().unwrap();
    assert_eq!(automatic.0, 7);
    assert!(!Arc::ptr_eq(&first, &automatic));
    assert!(container.get_bindings().is_empty());
}
