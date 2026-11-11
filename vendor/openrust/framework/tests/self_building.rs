use openrust::{
    container::{
        ClassDescriptor, ConstructorParam, Container, FactoryFuture, ParameterKey,
        ParameterOverrides, ParameterValue, ResolvedArgs, ServiceKey, StaticCall,
    },
    contracts::container::{BindingResolutionError, Container as _, SelfBuilding},
};
use std::sync::Arc;

#[derive(Debug)]
struct Item(u32);

impl SelfBuilding for Item {
    #[allow(non_snake_case)]
    fn newInstance(args: ResolvedArgs, container: &Container) -> FactoryFuture<'_, Self> {
        Box::pin(async move {
            let ordinary = container.makeAsync::<Self>().await?;
            Ok(Arc::new(Self(ordinary.0 + *args.service::<u32>(0)?)))
        })
    }
}

fn descriptor() -> ClassDescriptor {
    ClassDescriptor::selfBuilding::<Item>(
        vec![
            ConstructorParam::scalar::<u32>("number")
                .withDefault(|| Ok(ParameterValue::new(3_u32))),
        ],
        |args| Ok(Arc::new(Item(*args.get::<u32>(0)?))),
        vec![ConstructorParam::class::<u32>("number")],
    )
    .unwrap()
}

#[tokio::test]
async fn new_instance_injects_static_dependencies_and_reentry_uses_the_ordinary_constructor() {
    let container = Container::default();
    container.registerClass(descriptor()).unwrap();
    assert!(!container.bound::<Item>());
    assert!(matches!(
        container.make::<Item>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    assert!(matches!(
        container.makeAsync::<Item>().await,
        Err(BindingResolutionError::NotBound { .. })
    ));
    container.instance(Arc::new(8_u32)).unwrap();

    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("number"), ParameterValue::new(99_u32));
    assert_eq!(
        container.makeWithAsync::<Item>(parameters).await.unwrap().0,
        11
    );
    assert_eq!(container.buildAsync::<Item>().await.unwrap().0, 11);
    assert!(container.resolved(ServiceKey::typed::<Item>()).unwrap());
}

#[tokio::test]
async fn static_method_binding_and_explicit_service_binding_keep_their_precedence() {
    let container = Container::default();
    container.registerClass(descriptor()).unwrap();
    container
        .bindStaticMethod::<Item, Arc<Item>, _>("newInstance", |key, _| {
            assert_eq!(key, ServiceKey::typed::<Item>());
            Box::pin(async { Ok(Arc::new(Item(17))) })
        })
        .unwrap();
    assert_eq!(container.makeAsync::<Item>().await.unwrap().0, 17);

    container.instance(Arc::new(8_u32)).unwrap();
    assert_eq!(
        container
            .call::<Arc<Item>, _>(
                StaticCall::<Item>::qualified("NEWINSTANCE"),
                ParameterOverrides::new(),
            )
            .await
            .unwrap()
            .0,
        25
    );
    container
        .bind::<Item, _>(|_| Ok(Arc::new(Item(23))))
        .unwrap();
    assert_eq!(container.makeAsync::<Item>().await.unwrap().0, 23);
    assert_eq!(container.buildAsync::<Item>().await.unwrap().0, 17);
}

#[tokio::test]
async fn ordinary_reentry_still_detects_constructor_cycles() {
    let container = Container::default();
    container
        .registerClass(
            ClassDescriptor::selfBuilding::<Item>(
                vec![ConstructorParam::class::<Item>("cycle")],
                |args| args.service(0),
                vec![ConstructorParam::class::<u32>("number")],
            )
            .unwrap(),
        )
        .unwrap();
    container.instance(Arc::new(8_u32)).unwrap();
    assert!(matches!(
        container.makeAsync::<Item>().await,
        Err(BindingResolutionError::CircularDependency { .. })
    ));
}

#[test]
fn self_building_signature_and_method_declaration_are_validated() {
    assert!(matches!(
        ClassDescriptor::selfBuilding::<Item>(
            vec![],
            |_| Ok(Arc::new(Item(3))),
            vec![
                ConstructorParam::class::<u32>("number"),
                ConstructorParam::class::<u32>("number")
            ],
        ),
        Err(BindingResolutionError::InvalidConstructor { .. })
    ));
    let declare = |container: &Container| {
        container.registerStaticMethod::<Item, Arc<Item>, _>("newInstance", vec![], |_, _| {
            Box::pin(async { Ok(Arc::new(Item(1))) })
        })
    };
    let first = Container::default();
    first.registerClass(descriptor()).unwrap();
    assert!(matches!(
        declare(&first),
        Err(BindingResolutionError::InvalidMethod { .. })
    ));
    let second = Container::default();
    declare(&second).unwrap();
    assert!(matches!(
        second.registerClass(descriptor()),
        Err(BindingResolutionError::InvalidMethod { .. })
    ));
}

#[tokio::test]
async fn reentry_does_not_wait_for_or_publish_into_the_outer_shared_factory() {
    let container = Container::default();
    container.registerClass(descriptor()).unwrap();
    container.instance(Arc::new(8_u32)).unwrap();
    container
        .singletonAsync::<Item, _>(|container| {
            Box::pin(async move {
                tokio::task::yield_now().await;
                container.buildAsync::<Item>().await
            })
        })
        .await
        .unwrap();

    let (first, second) =
        tokio::join!(container.makeAsync::<Item>(), container.makeAsync::<Item>());
    let (first, second) = (first.unwrap(), second.unwrap());
    assert_eq!(first.0, 11);
    assert!(Arc::ptr_eq(&first, &second));
}
