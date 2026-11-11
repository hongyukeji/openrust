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
    container.afterResolvingAttribute::<Marker>({
        let callbacks = callbacks.clone();
        move |event, _| {
            assert!(event.parameter.is_none());
            callbacks.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    });
    container
        .registerClass(
            ClassDescriptor::concrete(
                vec![
                    ConstructorParam::scalar::<u32>("number")
                        .withDefault(|| Ok(ParameterValue::new(1_u32))),
                ],
                |args| Ok(Arc::new(Item(*args.get::<u32>(0)?))),
            )
            .unwrap()
            .withAttribute(ClassAttribute::marker(|| Ok(Marker)))
            .withLifetime(Lifetime::Singleton)
            .withLifetime(Lifetime::Scoped),
        )
        .unwrap();

    assert!(container.isShared(ServiceKey::typed::<Item>()));
    assert!(!container.bound::<Item>());
    let first = container.make::<Item>().unwrap();
    assert!(container.bound::<Item>());
    assert!(Arc::ptr_eq(
        &first,
        &container.makeAsync::<Item>().await.unwrap()
    ));
    assert!(Arc::ptr_eq(
        &first,
        &container
            .scope(container.makeAsync::<Item>())
            .await
            .unwrap()
    ));
    assert!(container.getBindings().is_empty());

    let mut overrides = ParameterOverrides::new();
    overrides.insert(ParameterKey::name("number"), ParameterValue::new(9_u32));
    assert_eq!(
        container.makeWithAsync::<Item>(overrides).await.unwrap().0,
        9
    );
    assert!(!Arc::ptr_eq(&first, &container.build::<Item>().unwrap()));
    assert_eq!(callbacks.load(Ordering::Relaxed), 3);
    container.forgetInstance(ServiceKey::typed::<Item>());
    assert!(!Arc::ptr_eq(
        &first,
        &container.makeAsync::<Item>().await.unwrap()
    ));
    container.flush();
    assert!(container.isShared(ServiceKey::typed::<Item>()));
    assert!(!container.bound::<Item>());
}

trait Reporter: Send + Sync {}
struct Report;
impl Reporter for Report {}

#[tokio::test]
async fn bind_and_bind_when_share_at_the_declared_service_lifetime() {
    let container = Container::default();
    container.resolveEnvironmentUsing(Some(Arc::new(|_| Ok(true.into()))));
    container
        .registerClass(ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Report))).unwrap())
        .unwrap();
    container.registerLifetime::<dyn Reporter>(Lifetime::Scoped);
    let bind = || Bind::new::<dyn Reporter, Report>(|value| value);
    container
        .registerBindingAttributes::<dyn Reporter>(vec![
            BindWhen::new(bind(), |_| Ok(false)).into(),
            bind().forEnvironments(["testing"]).into(),
        ])
        .unwrap();

    let first = container
        .scope(async {
            let first = container.makeAsync::<dyn Reporter>().await.unwrap();
            assert!(Arc::ptr_eq(
                &first,
                &container.make::<dyn Reporter>().unwrap()
            ));
            container.forgetScopedInstances();
            assert!(!Arc::ptr_eq(
                &first,
                &container.makeAsync::<dyn Reporter>().await.unwrap()
            ));
            first
        })
        .await;
    let second = container
        .scope(container.makeAsync::<dyn Reporter>())
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    let first = container.make::<dyn Reporter>().unwrap();
    container.forgetScopedInstances();
    assert!(!Arc::ptr_eq(
        &first,
        &container.make::<dyn Reporter>().unwrap()
    ));

    let next = Container::default();
    next.registerClass(ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Report))).unwrap())
        .unwrap();
    next.registerLifetime::<dyn Reporter>(Lifetime::Singleton);
    next.registerBindingAttributes::<dyn Reporter>(vec![
        BindWhen::new(bind(), |_| Ok(true)).into(),
    ])
    .unwrap();
    let (a, b) = tokio::join!(
        next.makeAsync::<dyn Reporter>(),
        next.makeAsync::<dyn Reporter>()
    );
    assert!(Arc::ptr_eq(&a.unwrap(), &b.unwrap()));
}

#[tokio::test]
async fn dual_mode_shared_class_can_resolve_async_dependencies_and_retry() {
    let container = Container::default();
    let attempts = Arc::new(AtomicUsize::new(0));
    container
        .bindAsync::<u32, _>({
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
        .registerClass(
            ClassDescriptor::concrete(vec![ConstructorParam::class::<u32>("number")], |args| {
                Ok(Arc::new(Item(*args.service::<u32>(0)?)))
            })
            .unwrap()
            .withLifetime(Lifetime::Singleton),
        )
        .unwrap();
    assert!(matches!(
        container.make::<Item>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    assert!(container.makeAsync::<Item>().await.is_err());
    let (a, b) = tokio::join!(container.makeAsync::<Item>(), container.makeAsync::<Item>());
    let a = a.unwrap();
    assert_eq!(a.0, 8);
    assert!(Arc::ptr_eq(&a, &b.unwrap()));
    assert!(Arc::ptr_eq(&a, &container.make::<Item>().unwrap()));
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
}

impl SelfBuilding for Item {
    #[allow(non_snake_case)]
    fn newInstance(_: ResolvedArgs, container: &Container) -> FactoryFuture<'_, Self> {
        Box::pin(async move { Ok(Arc::new(Self(container.makeAsync::<Self>().await?.0 + 1))) })
    }
}

#[tokio::test]
async fn singleton_self_building_reentry_uses_the_ordinary_constructor() {
    let container = Container::default();
    container
        .registerClass(
            ClassDescriptor::selfBuilding::<Item>(vec![], |_| Ok(Arc::new(Item(3))), vec![])
                .unwrap()
                .withLifetime(Lifetime::Singleton),
        )
        .unwrap();
    let (a, b) = tokio::join!(container.makeAsync::<Item>(), container.makeAsync::<Item>());
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
        .registerClass(
            ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Item(7))))
                .unwrap()
                .withLifetime(Lifetime::Singleton),
        )
        .unwrap();
    let first = container.make::<Item>().unwrap();
    assert_eq!(first.0, 5);
    assert!(Arc::ptr_eq(&first, &container.make::<Item>().unwrap()));
    container.offsetUnset(ServiceKey::typed::<Item>());
    let automatic = container.make::<Item>().unwrap();
    assert_eq!(automatic.0, 7);
    assert!(!Arc::ptr_eq(&first, &automatic));
    assert!(container.getBindings().is_empty());
}
