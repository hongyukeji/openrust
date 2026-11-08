//! 编译类共享声明与同一容器缓存的行为回归。

use super::attributes::{Bind, BindWhen};
use super::{
    Arc, ClassDescriptor, ConstructorParam, ContainerBuilder, ParameterKey, ParameterOverrides,
    ParameterValue, Result, ServiceKey,
};
use crate::config::Environment;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Debug)]
struct Counter(u32);

fn counter() -> ClassDescriptor {
    ClassDescriptor::concrete::<Counter>(
        "Tests/Counter",
        vec![
            ConstructorParam::scalar::<u32>("value")
                .with_default(|| Ok(ParameterValue::new(1_u32))),
        ],
        |args| Ok(Arc::new(Counter(*args.value(0)?.downcast::<u32>()?))),
    )
}

fn value_override(value: u32) -> ParameterOverrides {
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("value"), ParameterValue::new(value));
    parameters
}

#[tokio::test]
async fn class_lifetime_singleton_shares_without_registering_a_factory() -> Result<()> {
    let container =
        ContainerBuilder::with_class_descriptors(vec![counter().with_singleton().with_scoped()])?
            .build();
    let key = ServiceKey::typed::<Counter>();
    let alias = ServiceKey::named("counter");
    container.alias(key.clone(), alias.clone())?;
    assert!(container.is_shared_key(&key));
    assert!(!container.contains_key(&key));
    assert!(container.get_ready::<Counter>().is_none());

    let first_scope = container.scope()?;
    let second_scope = container.scope()?;
    let (first, second) = tokio::try_join!(
        first_scope.resolve::<Counter>(),
        second_scope.resolve_key::<Counter>(alias)
    )?;
    assert!(Arc::ptr_eq(&first, &second));
    assert!(container.contains_key(&key));
    assert!(container.resolved_key(&key)?);
    assert!(container.get_bindings()?.is_empty());
    assert!(container.get_instance::<Counter>().is_none());
    assert!(
        container
            .get_ready::<Counter>()
            .is_some_and(|value| Arc::ptr_eq(&first, &value))
    );

    let special = first_scope
        .resolve_with::<Counter>(value_override(7))
        .await?;
    assert_eq!(special.0, 7);
    assert!(!Arc::ptr_eq(&first, &special));
    assert!(Arc::ptr_eq(
        &first,
        &first_scope.resolve::<Counter>().await?
    ));

    let calls = Arc::new(AtomicUsize::new(0));
    let listener = Arc::clone(&calls);
    let current = container
        .rebinding::<Counter, _>(move |_, _| {
            listener.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(()) })
        })
        .await?;
    assert!(current.is_some_and(|value| Arc::ptr_eq(&first, &value)));
    first_scope
        .extend::<Counter, _>(|old, _| Box::pin(async move { Ok(Arc::new(Counter(old.0 + 1))) }))
        .await?;
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(container.resolve::<Counter>().await?.0, 2);
    container.forget_instance::<Counter>().await?;
    assert!(!container.contains_key(&key));
    assert!(container.resolved_key(&key)?);
    assert_eq!(container.resolve::<Counter>().await?.0, 1);
    Ok(())
}

#[tokio::test]
async fn class_lifetime_scoped_observes_each_scope_and_root_reset() -> Result<()> {
    let container =
        ContainerBuilder::with_class_descriptors(vec![counter().with_scoped()])?.build();
    let key = ServiceKey::typed::<Counter>();
    let first = container.scope()?;
    let second = container.scope()?;
    let value = first.resolve::<Counter>().await?;
    assert!(first.resolver().is_bound::<Counter>()?);
    assert!(!second.resolver().is_bound::<Counter>()?);
    assert!(!container.contains_key(&key));
    assert!(Arc::ptr_eq(&value, &first.resolve::<Counter>().await?));
    assert!(!Arc::ptr_eq(&value, &second.resolve::<Counter>().await?));
    assert!(container.get_ready::<Counter>().is_none());

    let root = container.resolve::<Counter>().await?;
    assert!(container.contains_key(&key));
    container
        .forget_scoped_instances(tokio::time::Instant::now() + Duration::from_secs(3))
        .await?;
    assert!(!container.contains_key(&key));
    assert!(!Arc::ptr_eq(&root, &container.resolve::<Counter>().await?));
    Ok(())
}

#[tokio::test]
async fn class_lifetime_honors_an_explicit_scoped_factory_before_the_singleton_marker() -> Result<()>
{
    let mut builder =
        ContainerBuilder::with_class_descriptors(vec![counter().with_scoped().with_singleton()])?;
    builder.bind::<Counter, _>(|_| Box::pin(async { Ok(Arc::new(Counter(4))) }));
    let container = builder.build();
    let first_scope = container.scope()?;
    let second_scope = container.scope()?;
    let first = first_scope.resolve::<Counter>().await?;
    assert_eq!(first.0, 4);
    assert!(Arc::ptr_eq(
        &first,
        &second_scope.resolve::<Counter>().await?
    ));
    assert!(!Arc::ptr_eq(
        &first,
        &first_scope
            .resolve_with::<Counter>(value_override(3))
            .await?
    ));
    assert!(
        container
            .get_bindings()?
            .iter()
            .any(|entry| entry.key == ServiceKey::typed::<Counter>() && !entry.shared)
    );

    container
        .scoped::<Counter, _>(|_| Box::pin(async { Ok(Arc::new(Counter(8))) }))
        .await?;
    let first = first_scope.resolve::<Counter>().await?;
    assert_eq!(first.0, 8);
    assert!(Arc::ptr_eq(
        &first,
        &first_scope.resolve::<Counter>().await?
    ));
    assert!(!Arc::ptr_eq(
        &first,
        &second_scope.resolve::<Counter>().await?
    ));
    Ok(())
}

trait Backend: Send + Sync {}
struct Local;
impl Backend for Local {}
struct Conditional;
impl Backend for Conditional {}

#[tokio::test]
async fn class_lifetime_abstract_class_can_share_an_explicit_ordinary_factory() -> Result<()> {
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::abstract_class::<Counter>("Tests/AbstractCounter").with_scoped(),
    ])?;
    builder.bind::<Counter, _>(|_| Box::pin(async { Ok(Arc::new(Counter(9))) }));
    let container = builder.build();
    let first = container.scope()?;
    let second = container.scope()?;
    assert!(container.is_shared_key(&ServiceKey::typed::<Counter>()));
    let value = first.resolve::<Counter>().await?;
    assert_eq!(value.0, 9);
    assert!(Arc::ptr_eq(&value, &first.resolve::<Counter>().await?));
    assert!(!Arc::ptr_eq(&value, &second.resolve::<Counter>().await?));
    Ok(())
}

#[tokio::test]
async fn class_lifetime_is_used_when_environment_and_condition_publish_factories() -> Result<()> {
    let condition_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&condition_calls);
    let builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::interface::<dyn Backend>("Tests/Backend")
            .with_bind(Bind::for_environments(
                ServiceKey::typed::<Local>(),
                ["local"],
            ))
            .with_scoped(),
        ClassDescriptor::concrete::<Local>("Tests/Local", vec![], |_| Ok(Arc::new(Local)))
            .with_projection(crate::class_projection!(Local => dyn Backend)),
        ClassDescriptor::concrete::<Conditional>("Tests/Conditional", vec![], |_| {
            Ok(Arc::new(Conditional))
        })
        .with_bind_when(BindWhen::new(
            ServiceKey::typed::<Conditional>(),
            move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                async { Ok(false) }
            },
        ))
        .with_singleton(),
    ])?;
    let container = builder.build();
    container.set_binding_environment(Environment::Local)?;
    let key = ServiceKey::typed::<dyn Backend>();
    assert!(!container.contains_key(&key));
    let first = container.scope()?;
    let second = container.scope()?;
    let backend = first.resolve::<dyn Backend>().await?;
    assert!(Arc::ptr_eq(
        &backend,
        &first.resolve::<dyn Backend>().await?
    ));
    assert!(!Arc::ptr_eq(
        &backend,
        &second.resolve::<dyn Backend>().await?
    ));
    assert!(
        container
            .get_bindings()?
            .iter()
            .any(|entry| entry.key == key && entry.shared)
    );

    let value = first.resolve::<Conditional>().await?;
    assert!(Arc::ptr_eq(&value, &second.resolve::<Conditional>().await?));
    assert_eq!(condition_calls.load(Ordering::Relaxed), 1);
    assert!(
        !container
            .get_bindings()?
            .iter()
            .any(|entry| entry.key == ServiceKey::typed::<Conditional>())
    );
    Ok(())
}
