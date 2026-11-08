//! 通过真实类型投影验证环境绑定发现、优先级和失败恢复。

use crate::config::Environment;
use crate::container::{
    BindingSnapshot, ClassDescriptor, ConstructorParam, Container, ContainerBuilder, ParameterKey,
    ParameterOverrides, ParameterValue, ServiceKey, attributes::Bind,
};
use crate::{Error, Result, class_projection};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

trait View: Send + Sync {
    fn selected(&self) -> (u8, u32);
}

struct Concrete<const ID: u8>(u32);

impl<const ID: u8> View for Concrete<ID> {
    fn selected(&self) -> (u8, u32) {
        (ID, self.0)
    }
}

struct Consumer(Arc<dyn View>);

fn concrete<const ID: u8>(symbol: &'static str) -> ClassDescriptor {
    ClassDescriptor::concrete::<Concrete<ID>>(
        symbol,
        vec![
            ConstructorParam::scalar::<u32>("value")
                .with_default(|| Ok(ParameterValue::new(0_u32))),
        ],
        |args| Ok(Arc::new(Concrete(*args.value(0)?.downcast::<u32>()?))),
    )
    .with_projection(class_projection!(Concrete<ID> => dyn View))
}

fn key<T: ?Sized + 'static>() -> ServiceKey {
    ServiceKey::typed::<T>()
}

fn fallback_view() -> ClassDescriptor {
    ClassDescriptor::interface::<dyn View>("Test/View").with_bind(Bind::new(key::<Concrete<1>>()))
}

fn only_binding(container: &Container) -> Result<BindingSnapshot> {
    let mut bindings = container.get_bindings()?;
    assert_eq!(bindings.len(), 1);
    let binding = bindings
        .pop()
        .ok_or_else(|| Error::custom("test_binding_missing", "缺少环境绑定"))?;
    assert_eq!(binding.key, key::<dyn View>());
    assert!(!binding.shared);
    Ok(binding)
}

#[tokio::test]
async fn fallback_waits_for_environment_and_publishes_transient_factory() -> Result<()> {
    let container = ContainerBuilder::with_class_descriptors(vec![
        fallback_view(),
        concrete::<1>("Test/First"),
    ])?
    .build();
    assert!(!container.contains::<dyn View>());
    assert!(matches!(container.resolve::<dyn View>().await, Err(error)
        if error.code() == "container_class_not_instantiable"));
    assert!(container.get_bindings()?.is_empty());

    container.set_binding_environment(Environment::Production)?;
    assert!(!container.contains::<dyn View>());
    let first = container
        .resolve_key::<dyn View>(ServiceKey::named("Test/View"))
        .await?;
    assert!(container.contains::<dyn View>());
    let second = container.resolve::<dyn View>().await?;
    let binding = only_binding(&container)?;
    let direct = binding
        .concrete
        .invoke::<dyn View>(container.factory_resolver()?)
        .await?;
    assert_eq!(first.selected(), (1, 0));
    assert_eq!(second.selected(), first.selected());
    assert_eq!(direct.selected(), first.selected());
    assert!(!Arc::ptr_eq(&first, &second));
    Ok(())
}

#[tokio::test]
async fn declaration_order_distinguishes_wildcard_after_falsey_hole_from_fallback() -> Result<()> {
    let fallback = Bind::for_environments(key::<Concrete<1>>(), ["*", "0", ""]);
    let wildcard = Bind::for_environments(key::<Concrete<2>>(), ["", "*"]);
    let exact = Bind::for_environments(key::<Concrete<3>>(), ["testing"]);
    for (first, second, expected) in [(wildcard.clone(), exact.clone(), 2), (exact, wildcard, 3)] {
        let container = ContainerBuilder::with_class_descriptors(vec![
            ClassDescriptor::interface::<dyn View>("Test/View")
                .with_bind(fallback.clone())
                .with_bind(first)
                .with_bind(second),
            concrete::<1>("Test/First"),
            concrete::<2>("Test/Second"),
            concrete::<3>("Test/Third"),
        ])?
        .build();
        container.set_binding_environment(Environment::Testing)?;
        assert_eq!(
            container.resolve::<dyn View>().await?.selected(),
            (expected, 0)
        );
    }
    Ok(())
}

#[tokio::test]
async fn explicit_factory_and_context_precede_attribute_discovery() -> Result<()> {
    let container = ContainerBuilder::with_class_descriptors(vec![
        fallback_view(),
        concrete::<1>("Test/First"),
        concrete::<2>("Test/Second"),
        ClassDescriptor::concrete::<Consumer>(
            "Test/Consumer",
            vec![ConstructorParam::class::<dyn View>("view")],
            |args| Ok(Arc::new(Consumer(args.service(0)?))),
        ),
    ])?
    .build();
    container.set_binding_environment(Environment::Testing)?;
    container
        .when(key::<Consumer>())?
        .needs(key::<dyn View>())
        .give(key::<Concrete<2>>())?;
    assert_eq!(container.resolve::<Consumer>().await?.0.selected(), (2, 0));
    assert!(container.get_bindings()?.is_empty());

    container
        .bind::<dyn View, _>(|_| {
            Box::pin(async { Ok(Arc::new(Concrete::<9>(0)) as Arc<dyn View>) })
        })
        .await?;
    let factory = only_binding(&container)?.concrete;
    assert_eq!(container.resolve::<dyn View>().await?.selected(), (9, 0));
    assert_eq!(container.resolve::<Consumer>().await?.0.selected(), (2, 0));
    assert!(only_binding(&container)?.concrete.same_factory(&factory));
    Ok(())
}

#[tokio::test]
async fn explicit_instance_wins_until_make_with_discovers_a_factory() -> Result<()> {
    let container = ContainerBuilder::with_class_descriptors(vec![
        fallback_view(),
        concrete::<1>("Test/First"),
    ])?
    .build();
    container.set_binding_environment(Environment::Testing)?;
    let instance: Arc<dyn View> = Arc::new(Concrete::<9>(0));
    container
        .instance::<dyn View>(Arc::clone(&instance))
        .await?;
    assert!(Arc::ptr_eq(
        &container.resolve::<dyn View>().await?,
        &instance
    ));
    assert!(container.get_bindings()?.is_empty());

    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("value"), ParameterValue::new(91_u32));
    assert_eq!(
        container
            .resolve_with::<dyn View>(parameters)
            .await?
            .selected(),
        (1, 91)
    );
    only_binding(&container)?;
    Ok(())
}

#[tokio::test]
async fn checked_environment_miss_is_not_reselected_after_environment_change() -> Result<()> {
    let container = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::interface::<dyn View>("Test/View")
            .with_bind(Bind::for_environments(key::<Concrete<1>>(), ["testing"])),
        concrete::<1>("Test/First"),
    ])?
    .build();

    for environment in [Environment::Production, Environment::Testing] {
        container.set_binding_environment(environment)?;
        assert!(matches!(container.resolve::<dyn View>().await, Err(error)
            if error.code() == "container_class_not_instantiable"));
        assert!(!container.contains::<dyn View>());
        assert!(container.get_bindings()?.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn constructor_failure_retains_discovered_factory_for_retry() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let constructions = Arc::clone(&calls);
    let container = ContainerBuilder::with_class_descriptors(vec![
        fallback_view().with_bind(Bind::for_environments(key::<Concrete<2>>(), ["testing"])),
        concrete::<1>("Test/First"),
        ClassDescriptor::concrete::<Concrete<2>>("Test/Second", vec![], move |_| {
            if constructions.fetch_add(1, Ordering::Relaxed) == 0 {
                return Err(Error::custom("test_constructor_failed", "首次构造失败"));
            }
            Ok(Arc::new(Concrete(2)))
        })
        .with_projection(class_projection!(Concrete<2> => dyn View)),
    ])?
    .build();
    container.set_binding_environment(Environment::Testing)?;
    assert!(container.resolve::<dyn View>().await.is_err());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(!container.resolved_key(&key::<dyn View>())?);
    let factory = only_binding(&container)?.concrete;

    container.set_binding_environment(Environment::Production)?;
    assert_eq!(container.resolve::<dyn View>().await?.selected(), (2, 2));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(container.resolved_key(&key::<dyn View>())?);
    assert!(only_binding(&container)?.concrete.same_factory(&factory));
    Ok(())
}
