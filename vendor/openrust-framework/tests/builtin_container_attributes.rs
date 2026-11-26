use openrust::{
    class_projection,
    config::Repository,
    container::{
        AttributeParameter, AttributeValue, Callback, ClassAttribute, ClassDescriptor,
        ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue, ServiceKey,
        TaggedServices,
        attributes::{Config, CurrentResolving, Give, Tag},
    },
    contracts::{
        config::Repository as ConfigRepository,
        container::{BindingResolutionError, Container as _},
    },
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Settings(u16, u16, u16);

#[tokio::test]
async fn config_reads_current_repository_preserves_null_and_redacts_type_failure() {
    let container = Container::default();
    let config = Arc::new(Repository::new(BTreeMap::from([(
        "app".into(),
        json!({"port": 8080, "nullable": null}),
    )])));
    container
        .instance_named::<dyn ConfigRepository>("config", config.clone())
        .unwrap();
    container
        .register_class(
            ClassDescriptor::concrete(
                vec![
                    ConstructorParam::scalar::<u16>("port").with_attribute(
                        ClassAttribute::contextual(|| Ok(Config::<u16>::new("app.port"))),
                    ),
                    ConstructorParam::scalar::<u16>("missing").with_attribute(
                        ClassAttribute::contextual(|| {
                            Ok(Config::new("app.missing").with_default(42_u16))
                        }),
                    ),
                    ConstructorParam::scalar::<u16>("nullable")
                        .with_default(|| Ok(ParameterValue::new(7_u16)))
                        .with_attribute(ClassAttribute::contextual(|| {
                            Ok(Config::new("app.nullable").with_default(11_u16))
                        })),
                ],
                |args| {
                    Ok(Arc::new(Settings(
                        *args.get(0)?,
                        *args.get(1)?,
                        *args.get(2)?,
                    )))
                },
            )
            .unwrap(),
        )
        .unwrap();
    let settings = container.make::<Settings>().unwrap();
    assert_eq!((settings.0, settings.1, settings.2), (8080, 42, 7));

    config
        .set("app.port", json!("private-config-value"))
        .unwrap();
    let error = container.make_async::<Settings>().await.err().unwrap();
    assert!(matches!(
        error,
        BindingResolutionError::ConfigurationValueTypeMismatch { .. }
    ));
    assert!(!error.to_string().contains("private-config-value"));
    config.set("app.port", json!(9090)).unwrap();
    assert_eq!(container.make_async::<Settings>().await.unwrap().0, 9090);

    let signature = [ConstructorParam::scalar::<u16>("nullable")
        .nullable()
        .with_attribute(ClassAttribute::contextual(|| {
            Ok(Config::new("app.nullable").with_default(11_u16))
        }))];
    let null = container
        .call(
            Callback::new(&signature, |args| async move {
                Ok(args.resolved.value(0)?.is_null())
            }),
            ParameterOverrides::new(),
        )
        .await
        .unwrap();
    assert!(null);
}

trait Contract: Send + Sync {
    fn value(&self) -> u32;
}
struct Concrete(u32);
impl Contract for Concrete {
    fn value(&self) -> u32 {
        self.0
    }
}
struct Consumer(Arc<dyn Contract>);

#[tokio::test]
async fn give_awaits_the_selected_implementation_and_uses_its_own_parameters() {
    let container = Container::default();
    container
        .register_class(
            ClassDescriptor::concrete_async(
                vec![ConstructorParam::scalar::<u32>("number")],
                |args| {
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        Ok(Arc::new(Concrete(*args.get(0)?)))
                    })
                },
            )
            .unwrap()
            .with_projection(class_projection!(Concrete => dyn Contract)),
        )
        .unwrap();
    container
        .alias(ServiceKey::typed::<Concrete>(), "implementation")
        .unwrap();
    container
        .register_class(
            ClassDescriptor::concrete(
                vec![
                    ConstructorParam::class::<dyn Contract>("contract").with_attribute(
                        ClassAttribute::contextual(|| {
                            let mut parameters = ParameterOverrides::new();
                            parameters
                                .insert(ParameterKey::name("number"), ParameterValue::new(6_u32));
                            Ok(Give::<dyn Contract>::new("implementation")
                                .with_parameters(parameters))
                        }),
                    ),
                ],
                |args| Ok(Arc::new(Consumer(args.service(0)?))),
            )
            .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        container.make::<Consumer>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    let mut outer = ParameterOverrides::new();
    outer.insert(ParameterKey::name("number"), ParameterValue::new(99_u32));
    let (a, b) = tokio::join!(
        container.make_with_async::<Consumer>(outer),
        container.make_async::<Consumer>()
    );
    assert_eq!(a.unwrap().0.value(), 6);
    assert_eq!(b.unwrap().0.value(), 6);
}

#[tokio::test]
async fn tag_is_lazy_rewindable_ordered_and_container_bound() {
    let container = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    container
        .bind_named::<u32, _>("first", {
            let calls = calls.clone();
            move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::new(1))
            }
        })
        .unwrap();
    container
        .bind_async_named::<u32, _>("next", |_| Box::pin(async { Ok(Arc::new(2)) }))
        .await
        .unwrap();
    container.tag(["first", "first"], ["numbers"]);
    let attribute = ClassAttribute::contextual(|| Ok(Tag::<u32>::new("numbers")));
    let result = container
        .resolve_from_attribute(
            &attribute,
            AttributeParameter {
                name: "items",
                variadic: false,
            },
        )
        .unwrap();
    let AttributeValue::One(result) = result else {
        panic!("owned tag sequence");
    };
    let tagged = result.downcast::<TaggedServices<u32>>().unwrap();
    assert_eq!(tagged.len(), 2);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(matches!(
        tagged.iter(&Container::default()),
        Err(BindingResolutionError::TaggedContainerMismatch)
    ));
    container.tag(["next"], ["numbers"]);
    for _ in 0..2 {
        let values = tagged.collect_async(&container).await.unwrap();
        assert_eq!(
            values.iter().map(|value| **value).collect::<Vec<_>>(),
            [1, 1, 2]
        );
    }
    assert_eq!(tagged.len(), 2);
    assert_eq!(calls.load(Ordering::Relaxed), 4);
}

struct Identity(ServiceKey);

#[tokio::test]
async fn current_resolving_tracks_consumer_and_returns_null_outside_resolution() {
    let container = Container::default();
    let attribute = ClassAttribute::contextual(|| Ok(CurrentResolving));
    let result = container
        .resolve_from_attribute(
            &attribute,
            AttributeParameter {
                name: "consumer",
                variadic: false,
            },
        )
        .unwrap();
    assert!(matches!(result, AttributeValue::One(value) if value.is_null()));
    container
        .register_class(
            ClassDescriptor::concrete(
                vec![ConstructorParam::scalar::<ServiceKey>("consumer").with_attribute(attribute)],
                |args| Ok(Arc::new(Identity((*args.get::<ServiceKey>(0)?).clone()))),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        container.make_async::<Identity>().await.unwrap().0,
        ServiceKey::typed::<Identity>()
    );
    assert!(container.currently_resolving().is_none());
}
