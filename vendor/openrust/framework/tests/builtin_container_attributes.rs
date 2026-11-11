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
        .instanceNamed::<dyn ConfigRepository>("config", config.clone())
        .unwrap();
    container
        .registerClass(
            ClassDescriptor::concrete(
                vec![
                    ConstructorParam::scalar::<u16>("port").withAttribute(
                        ClassAttribute::contextual(|| Ok(Config::<u16>::new("app.port"))),
                    ),
                    ConstructorParam::scalar::<u16>("missing").withAttribute(
                        ClassAttribute::contextual(|| {
                            Ok(Config::new("app.missing").withDefault(42_u16))
                        }),
                    ),
                    ConstructorParam::scalar::<u16>("nullable")
                        .withDefault(|| Ok(ParameterValue::new(7_u16)))
                        .withAttribute(ClassAttribute::contextual(|| {
                            Ok(Config::new("app.nullable").withDefault(11_u16))
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
    let error = container.makeAsync::<Settings>().await.err().unwrap();
    assert!(matches!(
        error,
        BindingResolutionError::ConfigurationValueTypeMismatch { .. }
    ));
    assert!(!error.to_string().contains("private-config-value"));
    config.set("app.port", json!(9090)).unwrap();
    assert_eq!(container.makeAsync::<Settings>().await.unwrap().0, 9090);

    let signature = [ConstructorParam::scalar::<u16>("nullable")
        .nullable()
        .withAttribute(ClassAttribute::contextual(|| {
            Ok(Config::new("app.nullable").withDefault(11_u16))
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
        .registerClass(
            ClassDescriptor::concreteAsync(
                vec![ConstructorParam::scalar::<u32>("number")],
                |args| {
                    Box::pin(async move {
                        tokio::task::yield_now().await;
                        Ok(Arc::new(Concrete(*args.get(0)?)))
                    })
                },
            )
            .unwrap()
            .withProjection(class_projection!(Concrete => dyn Contract)),
        )
        .unwrap();
    container
        .alias(ServiceKey::typed::<Concrete>(), "implementation")
        .unwrap();
    container
        .registerClass(
            ClassDescriptor::concrete(
                vec![
                    ConstructorParam::class::<dyn Contract>("contract").withAttribute(
                        ClassAttribute::contextual(|| {
                            let mut parameters = ParameterOverrides::new();
                            parameters
                                .insert(ParameterKey::name("number"), ParameterValue::new(6_u32));
                            Ok(Give::<dyn Contract>::new("implementation")
                                .withParameters(parameters))
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
        container.makeWithAsync::<Consumer>(outer),
        container.makeAsync::<Consumer>()
    );
    assert_eq!(a.unwrap().0.value(), 6);
    assert_eq!(b.unwrap().0.value(), 6);
}

#[tokio::test]
async fn tag_is_lazy_rewindable_ordered_and_container_bound() {
    let container = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    container
        .bindNamed::<u32, _>("first", {
            let calls = calls.clone();
            move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::new(1))
            }
        })
        .unwrap();
    container
        .bindAsyncNamed::<u32, _>("next", |_| Box::pin(async { Ok(Arc::new(2)) }))
        .await
        .unwrap();
    container.tag(["first", "first"], ["numbers"]);
    let attribute = ClassAttribute::contextual(|| Ok(Tag::<u32>::new("numbers")));
    let result = container
        .resolveFromAttribute(
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
        let values = tagged.collectAsync(&container).await.unwrap();
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
        .resolveFromAttribute(
            &attribute,
            AttributeParameter {
                name: "consumer",
                variadic: false,
            },
        )
        .unwrap();
    assert!(matches!(result, AttributeValue::One(value) if value.is_null()));
    container
        .registerClass(
            ClassDescriptor::concrete(
                vec![ConstructorParam::scalar::<ServiceKey>("consumer").withAttribute(attribute)],
                |args| Ok(Arc::new(Identity((*args.get::<ServiceKey>(0)?).clone()))),
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        container.makeAsync::<Identity>().await.unwrap().0,
        ServiceKey::typed::<Identity>()
    );
    assert!(container.currentlyResolving().is_none());
}
