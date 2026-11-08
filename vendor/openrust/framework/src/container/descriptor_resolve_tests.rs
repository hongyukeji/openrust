//! 从真实 Resolver 验证描述符构造、类型预检与选参。
use super::class_descriptor::{ClassDescriptor, ConstructorParam};
use super::parameters::ParameterValue;
use super::{ContainerBuilder, Error, Lifetime, Result, ServiceKey, lock};
use crate::class_projection;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Dependency(u32);
struct Consumer(Arc<Dependency>);

#[tokio::test]
async fn typed_descriptor_uses_transient_fallback_and_records_only_success_history() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let constructions = Arc::clone(&calls);
    let container = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<Dependency>("App/Dependency", vec![], |_| {
            Ok(Arc::new(Dependency(7)))
        }),
        ClassDescriptor::concrete::<Consumer>(
            "App/Consumer",
            vec![ConstructorParam::class::<Dependency>("dependency")],
            move |args| {
                constructions.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::new(Consumer(args.service(0)?)))
            },
        ),
    ])?
    .build();
    let key = ServiceKey::typed::<Consumer>();
    assert!(!lock(&container.inner.state).resolved_key(&key)?);

    let first = container.resolve::<Consumer>().await?;
    let second = container.resolve::<Consumer>().await?;
    assert_eq!(first.0.0, 7);
    assert_eq!(second.0.0, 7);
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&first.0, &second.0));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert!(lock(&container.inner.state).resolved_key(&key)?);
    assert!(!container.contains::<Consumer>());
    assert!(!container.contains::<Dependency>());
    assert!(container.bindings_snapshot()?.is_empty());
    assert!(container.get_ready::<Consumer>().is_none());
    assert!(lock(&container.inner.state).slots.is_empty());
    Ok(())
}

struct OptionalConsumer(Option<Arc<Dependency>>);

#[tokio::test]
async fn class_default_precedes_unbound_descriptor_but_explicit_instance_wins() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let constructions = Arc::clone(&calls);
    let container = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<Dependency>("App/Dependency", vec![], move |_| {
            constructions.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(Dependency(7)))
        }),
        ClassDescriptor::concrete::<OptionalConsumer>(
            "App/OptionalConsumer",
            vec![
                ConstructorParam::class::<Dependency>("dependency")
                    .nullable()
                    .with_default(|| Ok(ParameterValue::null())),
            ],
            |args| {
                let dependency = if args.value(0)?.is_null() {
                    None
                } else {
                    Some(args.service(0)?)
                };
                Ok(Arc::new(OptionalConsumer(dependency)))
            },
        ),
    ])?
    .build();

    assert!(container.resolve::<OptionalConsumer>().await?.0.is_none());
    assert_eq!(calls.load(Ordering::Relaxed), 0);

    let explicit = Arc::new(Dependency(19));
    container.instance(Arc::clone(&explicit)).await?;
    let consumer = container.resolve::<OptionalConsumer>().await?;
    assert!(
        consumer
            .0
            .as_ref()
            .is_some_and(|dependency| Arc::ptr_eq(dependency, &explicit))
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test]
async fn nullable_class_without_default_preserves_resolution_failure() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let constructions = Arc::clone(&calls);
    let container = ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<
        OptionalConsumer,
    >(
        "App/OptionalConsumer",
        vec![ConstructorParam::class::<Dependency>("dependency").nullable()],
        move |_| {
            constructions.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(OptionalConsumer(None)))
        },
    )])?
    .build();

    assert!(
        matches!(container.resolve::<OptionalConsumer>().await, Err(error) if error.code() == "service_not_bound")
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(!lock(&container.inner.state).resolved_key(&ServiceKey::typed::<OptionalConsumer>())?);
    Ok(())
}

#[tokio::test]
async fn wrong_requested_projection_and_dependency_output_execute_no_user_code() -> Result<()> {
    let constructors = Arc::new(AtomicUsize::new(0));
    let constructions = Arc::clone(&constructors);
    let mut builder =
        ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<Consumer>(
            "App/Consumer",
            vec![ConstructorParam::class::<Dependency>("dependency")],
            move |args| {
                constructions.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::new(Consumer(args.service(0)?)))
            },
        )])?;
    let factories = Arc::new(AtomicUsize::new(0));
    let factory_calls = Arc::clone(&factories);
    let miswired = ServiceKey::named("miswired");
    builder.register_key::<String, _>(
        miswired.clone(),
        Lifetime::Bind,
        move |_| {
            factory_calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(Arc::new(String::from("wrong output"))) })
        },
        false,
    );
    builder.alias_key(miswired, ServiceKey::typed::<Dependency>())?;
    let container = builder.build();
    let resolver = container.factory_resolver_internal()?;

    assert!(
        matches!(resolver.resolve_key::<String>(ServiceKey::named("App/Consumer")).await,
            Err(error) if error.code() == "service_type_mismatch")
    );
    assert!(
        matches!(resolver.resolve::<Consumer>().await, Err(error) if error.code() == "service_type_mismatch")
    );
    assert_eq!(factories.load(Ordering::Relaxed), 0);
    assert_eq!(constructors.load(Ordering::Relaxed), 0);
    assert!(!lock(&container.inner.state).resolved_key(&ServiceKey::typed::<Consumer>())?);
    Ok(())
}

trait DependencyView: Send + Sync {
    fn number(&self) -> u32;
}

impl DependencyView for Dependency {
    fn number(&self) -> u32 {
        self.0
    }
}

#[tokio::test]
async fn named_descriptor_projection_returns_the_same_object_as_a_trait() -> Result<()> {
    let concrete = Arc::new(Dependency(23));
    let constructed = Arc::clone(&concrete);
    let descriptor = ClassDescriptor::concrete::<Dependency>("App/Dependency", vec![], move |_| {
        Ok(Arc::clone(&constructed))
    })
    .with_projection(class_projection!(Dependency => dyn DependencyView));
    let container = ContainerBuilder::with_class_descriptors(vec![descriptor])?.build();
    let resolver = container.factory_resolver_internal()?;

    let view = resolver
        .resolve_key::<dyn DependencyView>(ServiceKey::named("App/Dependency"))
        .await?;
    let expected: Arc<dyn DependencyView> = concrete;
    assert_eq!(view.number(), 23);
    assert!(Arc::ptr_eq(&view, &expected));
    assert!(lock(&container.inner.state).resolved_key(&ServiceKey::typed::<Dependency>())?);
    assert!(container.bindings_snapshot()?.is_empty());
    Ok(())
}

struct VariadicConsumer(usize);

#[tokio::test]
async fn class_variadic_empty_for_unbound_interface_preserves_same_code_business_error()
-> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let constructions = Arc::clone(&calls);
    let container = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::interface::<dyn DependencyView>("App/DependencyView"),
        ClassDescriptor::concrete::<VariadicConsumer>(
            "App/VariadicConsumer",
            vec![ConstructorParam::class::<dyn DependencyView>("dependencies").variadic()],
            move |args| {
                constructions.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::new(VariadicConsumer(args.values(0)?.len())))
            },
        ),
    ])?
    .build();

    assert_eq!(container.resolve::<VariadicConsumer>().await?.0, 0);
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    let factories = Arc::new(AtomicUsize::new(0));
    let factory_calls = Arc::clone(&factories);
    container
        .bind::<dyn DependencyView, _>(move |_| {
            factory_calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async {
                Err(Error::http_with_source(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "service_not_bound",
                    "业务构造失败",
                    std::io::Error::other("业务来源"),
                ))
            })
        })
        .await?;
    let Err(failure) = container.resolve::<VariadicConsumer>().await else {
        return Err(Error::custom(
            "test_missing_failure",
            "变参不得吞掉业务错误",
        ));
    };
    assert_eq!(failure.code(), "service_not_bound");
    assert!(
        std::error::Error::source(&failure)
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .is_some_and(|source| source.kind() == std::io::ErrorKind::Other)
    );
    assert_eq!(factories.load(Ordering::Relaxed), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    Ok(())
}

#[tokio::test]
async fn scalar_diagnostic_scope_close_stops_later_conversion_and_constructor() -> Result<()> {
    use super::{
        ParameterKey, ParameterOverrides, PhpBytes, PhpStringable, PhpTarget, ScalarDiagnostics,
        Scope,
    };
    use std::sync::{Mutex, Weak};

    struct DiagnosticConsumer;

    let conversions = Arc::new(AtomicUsize::new(0));
    let constructed = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&constructed);
    let mut builder = ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<
        DiagnosticConsumer,
    >(
        "App/DiagnosticConsumer",
        vec![
            ConstructorParam::php("first", PhpTarget::Int),
            ConstructorParam::php("later", PhpTarget::Bytes),
        ],
        move |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(DiagnosticConsumer))
        },
    )])?;
    let scope_slot = Arc::new(Mutex::new(Weak::<Scope>::new()));
    let close = Arc::clone(&scope_slot);
    builder.scalar_diagnostics(ScalarDiagnostics::new(move |_| {
        let scope = lock(&close)
            .upgrade()
            .ok_or_else(|| Error::custom("test_scope_missing", "缺少待关闭的作用域"))?;
        scope.close();
        Ok(())
    }));
    let container = builder.build();
    let scope = Arc::new(container.scope()?);
    *lock(&scope_slot) = Arc::downgrade(&scope);

    let later = Arc::clone(&conversions);
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("first"), ParameterValue::new(2.5_f64));
    parameters.insert(
        ParameterKey::name("later"),
        ParameterValue::new(PhpStringable::new(move || {
            later.fetch_add(1, Ordering::Relaxed);
            Ok(PhpBytes::new(&b"skipped"[..]))
        })),
    );
    assert!(matches!(
        scope.resolve_with::<DiagnosticConsumer>(parameters).await,
        Err(error) if error.code() == "scope_closed"
    ));
    assert_eq!(conversions.load(Ordering::Relaxed), 0);
    assert_eq!(constructed.load(Ordering::Relaxed), 0);
    Ok(())
}
