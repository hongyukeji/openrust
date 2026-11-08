//! 验证外部 package 可沿公开 API 装饰 dyn trait 与隔离 scope。

use openrust::class_projection;
use openrust::container::{
    ClassAttribute, ClassDescriptor, ConstructorParam, ContainerBuilder, ParameterKey,
    ParameterOverrides, ParameterValue, PhpBytes, PhpStringable, PhpTarget, ResolutionEvent,
    ResolvedArgs, Resolver, ScalarDiagnostic, ScalarDiagnostics, SelfBuilding, ServiceKey,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Barrier;

trait Greeting: Send + Sync {
    fn value(&self) -> String;
}

struct BaseGreeting;

impl Greeting for BaseGreeting {
    fn value(&self) -> String {
        "hello".to_owned()
    }
}

const CLASS_GREETING_SYMBOL: &str = "consumer::ClassGreeting";

struct ClassGreeting(String);

impl Greeting for ClassGreeting {
    fn value(&self) -> String {
        self.0.clone()
    }
}

fn class_greeting_descriptor(constructions: Arc<AtomicUsize>) -> ClassDescriptor {
    ClassDescriptor::concrete::<ClassGreeting>(
        CLASS_GREETING_SYMBOL,
        vec![
            ConstructorParam::scalar::<String>("title")
                .with_default(|| Ok(ParameterValue::new(String::from("描述符默认值")))),
        ],
        move |args| {
            constructions.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(ClassGreeting(
                args.value(0)?.downcast::<String>()?.as_ref().clone(),
            )))
        },
    )
    .with_projection(class_projection!(ClassGreeting => dyn Greeting))
}

fn class_greeting_parameters(title: &str) -> ParameterOverrides {
    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        ParameterKey::name("title"),
        ParameterValue::new(title.to_owned()),
    );
    parameters
}

struct AmbientGreeting(Arc<dyn Greeting>);

#[tokio::test]
async fn same_class_factory_builds_with_ambient_parameters_and_no_inner_publication()
-> openrust::Result<()> {
    let constructions = Arc::new(AtomicUsize::new(0));
    let extensions = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::with_class_descriptors(vec![class_greeting_descriptor(
        Arc::clone(&constructions),
    )])?;
    let key = ServiceKey::typed::<ClassGreeting>();
    let symbol = ServiceKey::named(CLASS_GREETING_SYMBOL);
    builder.singleton_class::<ClassGreeting>(symbol, key.clone());
    let extension_calls = Arc::clone(&extensions);
    builder.extend::<ClassGreeting, _>(move |value, _| {
        extension_calls.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move { Ok(Arc::new(ClassGreeting(format!("{}（装饰）", value.0)))) })
    })?;
    let container = builder.build();
    let binding = container
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == key)
        .ok_or_else(|| openrust::Error::custom("test_binding_missing", "缺少同键类工厂"))?;
    assert!(binding.shared);
    let factory = binding.concrete;
    assert!(matches!(
        factory.invoke::<bool>(container.factory_resolver()?).await,
        Err(error) if error.code() == "service_type_mismatch"
    ));
    assert_eq!(constructions.load(Ordering::Relaxed), 0);
    let raw = factory
        .invoke_with::<ClassGreeting>(
            container.factory_resolver()?,
            class_greeting_parameters("原工厂的显式实参"),
        )
        .await?;
    assert_eq!(raw.value(), "描述符默认值");
    assert!(!container.resolved_key(&key)?);
    assert_eq!(extensions.load(Ordering::Relaxed), 0);

    container
        .bind::<AmbientGreeting, _>(move |resolver| {
            let factory = factory.clone();
            Box::pin(async move {
                let greeting = factory
                    .invoke_with::<dyn Greeting>(
                        resolver,
                        class_greeting_parameters("原工厂的显式实参"),
                    )
                    .await?;
                Ok(Arc::new(AmbientGreeting(greeting)))
            })
        })
        .await?;
    let ambient = container
        .resolve_with::<AmbientGreeting>(class_greeting_parameters("外层参数"))
        .await?;
    assert_eq!(ambient.0.value(), "外层参数");
    assert!(!container.resolved_key(&key)?);

    let explicit = container
        .resolve_with::<ClassGreeting>(class_greeting_parameters("普通调用参数"))
        .await?;
    assert_eq!(explicit.value(), "普通调用参数（装饰）");
    assert!(!container.resolved_key(&key)?);
    let shared = container.resolve::<ClassGreeting>().await?;
    assert_eq!(shared.value(), "描述符默认值（装饰）");
    assert!(Arc::ptr_eq(
        &shared,
        &container.resolve::<ClassGreeting>().await?
    ));
    assert!(container.resolved_key(&key)?);
    assert_eq!(constructions.load(Ordering::Relaxed), 4);
    assert_eq!(extensions.load(Ordering::Relaxed), 2);
    Ok(())
}

#[tokio::test]
async fn different_class_factory_uses_supplied_container_and_only_publishes_inner_resolution()
-> openrust::Result<()> {
    let source_constructions = Arc::new(AtomicUsize::new(0));
    let target_constructions = Arc::new(AtomicUsize::new(0));
    let abstract_key = ServiceKey::typed::<dyn Greeting>();
    let concrete_key = ServiceKey::typed::<ClassGreeting>();
    let concrete_symbol = ServiceKey::named(CLASS_GREETING_SYMBOL);

    let mut source = ContainerBuilder::with_class_descriptors(vec![class_greeting_descriptor(
        Arc::clone(&source_constructions),
    )])?;
    source.singleton_class::<dyn Greeting>(abstract_key.clone(), concrete_symbol.clone());
    let source = source.build();
    let snapshot = source
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == abstract_key)
        .ok_or_else(|| openrust::Error::custom("test_binding_missing", "缺少异键类工厂"))?;
    assert!(snapshot.shared);
    let factory = snapshot.concrete;

    let mut target = ContainerBuilder::with_class_descriptors(vec![class_greeting_descriptor(
        Arc::clone(&target_constructions),
    )])?;
    target.singleton_class::<ClassGreeting>(concrete_key.clone(), concrete_symbol);
    target.bind::<dyn Greeting, _>(|_| {
        Box::pin(async { Ok(Arc::new(BaseGreeting) as Arc<dyn Greeting>) })
    });
    target
        .when(concrete_key.clone())?
        .needs_parameter("title")
        .give_value(ParameterValue::new(String::from("调用方容器")))?;
    target.extend::<ClassGreeting, _>(|value, _| {
        Box::pin(async move { Ok(Arc::new(ClassGreeting(format!("{}（内层装饰）", value.0)))) })
    })?;
    let target = target.build();

    assert!(matches!(
        factory.invoke::<bool>(target.factory_resolver()?).await,
        Err(error) if error.code() == "service_type_mismatch"
    ));
    assert_eq!(target_constructions.load(Ordering::Relaxed), 0);

    source
        .bind::<dyn Greeting, _>(|_| {
            Box::pin(async { Ok(Arc::new(BaseGreeting) as Arc<dyn Greeting>) })
        })
        .await?;
    let rebound = source
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == abstract_key)
        .ok_or_else(|| openrust::Error::custom("test_binding_missing", "缺少重绑工厂"))?;
    assert!(!rebound.concrete.same_factory(&factory));

    let explicit = factory
        .invoke_with::<dyn Greeting>(
            target.factory_resolver()?,
            class_greeting_parameters("原工厂实参"),
        )
        .await?;
    assert_eq!(explicit.value(), "原工厂实参（内层装饰）");
    assert!(!target.resolved_key(&concrete_key)?);
    let shared = factory
        .invoke::<dyn Greeting>(target.factory_resolver()?)
        .await?;
    let shared_concrete = factory
        .invoke::<ClassGreeting>(target.factory_resolver()?)
        .await?;
    assert_eq!(shared.value(), "调用方容器（内层装饰）");
    let projected: Arc<dyn Greeting> = Arc::<ClassGreeting>::clone(&shared_concrete);
    assert!(Arc::ptr_eq(&shared, &projected));
    assert!(Arc::ptr_eq(
        &shared_concrete,
        &target.resolve::<ClassGreeting>().await?
    ));
    assert!(target.resolved_key(&concrete_key)?);
    assert!(!target.resolved_key(&abstract_key)?);
    assert!(!source.resolved_key(&abstract_key)?);
    assert_eq!(source_constructions.load(Ordering::Relaxed), 0);
    assert_eq!(target_constructions.load(Ordering::Relaxed), 2);
    assert_eq!(source.resolve::<dyn Greeting>().await?.value(), "hello");
    Ok(())
}

#[tokio::test]
async fn external_conditional_factory_receives_only_current_named_parameters()
-> openrust::Result<()> {
    let key = ServiceKey::named("consumer.message");
    let mut builder = ContainerBuilder::new();
    assert!(
        builder.singleton_key_if::<String, _>(key.clone(), |resolver| {
            Box::pin(async move {
                let frame = resolver.parameters();
                frame.get(&ParameterKey::name("message")).map_or_else(
                    || Ok(Arc::new(String::from("普通"))),
                    ParameterValue::downcast::<String>,
                )
            })
        })
    );
    let container = builder.build();
    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        ParameterKey::name("message"),
        ParameterValue::new(String::from("当次")),
    );
    let value = container
        .resolve_key_with::<String>(key.clone(), parameters)
        .await?;
    assert_eq!(value.as_str(), "当次");
    assert!(!container.resolved_key(&key)?);
    let shared = container.resolve_key::<String>(key.clone()).await?;
    assert_eq!(shared.as_str(), "普通");
    assert!(Arc::ptr_eq(
        &shared,
        &container.resolve_key::<String>(key).await?
    ));
    Ok(())
}

struct ContextualGreeting;

impl Greeting for ContextualGreeting {
    fn value(&self) -> String {
        "当前消费者".to_owned()
    }
}

struct FactoryDependency(Arc<dyn Greeting>);
struct Report {
    selected: Arc<dyn Greeting>,
    nested: Arc<dyn Greeting>,
}

#[tokio::test]
async fn contextual_redirect_uses_projected_alias_and_bypasses_shared_abstract()
-> openrust::Result<()> {
    let abstract_key = ServiceKey::typed::<dyn Greeting>();
    let abstract_alias = ServiceKey::named("consumer.greeting.need");
    let concrete = ServiceKey::typed::<ContextualGreeting>();
    let concrete_alias = ServiceKey::named("consumer.greeting.contextual");
    let consumer_alias = ServiceKey::named("consumer.report");
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<ContextualGreeting>(
            "consumer::ContextualGreeting",
            vec![],
            |_| Ok(Arc::new(ContextualGreeting)),
        )
        .with_projection(class_projection!(ContextualGreeting => dyn Greeting)),
        ClassDescriptor::concrete::<Report>(
            "consumer::Report",
            vec![
                ConstructorParam::class::<dyn Greeting>("selected"),
                ConstructorParam::class::<FactoryDependency>("nested"),
            ],
            |args| {
                Ok(Arc::new(Report {
                    selected: args.service::<dyn Greeting>(0)?,
                    nested: Arc::clone(&args.service::<FactoryDependency>(1)?.0),
                }))
            },
        ),
    ])?;
    builder.alias(abstract_key.clone(), abstract_alias.clone())?;
    builder.alias(concrete, concrete_alias.clone())?;
    builder.alias(ServiceKey::typed::<Report>(), consumer_alias.clone())?;
    builder.singleton::<ContextualGreeting, _>(|_| {
        Box::pin(async { Ok(Arc::new(ContextualGreeting)) })
    });
    builder.singleton::<dyn Greeting, _>(|_| {
        Box::pin(async { Ok(Arc::new(BaseGreeting) as Arc<dyn Greeting>) })
    });
    builder.bind::<FactoryDependency, _>(|resolver| {
        Box::pin(async move {
            Ok(Arc::new(FactoryDependency(
                resolver.resolve::<dyn Greeting>().await?,
            )))
        })
    });
    builder
        .when(consumer_alias.clone())?
        .needs(abstract_alias)
        .give(concrete_alias.clone())?;
    let container = builder.build();
    let ordinary = container.resolve::<dyn Greeting>().await?;
    let selected: Arc<dyn Greeting> = container.resolve::<ContextualGreeting>().await?;
    for _ in 0..2 {
        let report = container
            .resolve_key::<Report>(consumer_alias.clone())
            .await?;
        assert_eq!(report.selected.value(), "当前消费者");
        assert!(Arc::ptr_eq(&report.selected, &selected));
        assert!(Arc::ptr_eq(&report.nested, &ordinary));
    }
    assert!(Arc::ptr_eq(
        &ordinary,
        &container.resolve::<dyn Greeting>().await?
    ));
    Ok(())
}

struct ConfiguredReport(Option<Arc<String>>, Arc<dyn Greeting>);

#[tokio::test]
async fn contextual_rules_precede_defaults_but_explicit_null_precedes_context()
-> openrust::Result<()> {
    let consumer = ServiceKey::typed::<ConfiguredReport>();
    let abstract_key = ServiceKey::typed::<dyn Greeting>();
    let mut builder = ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<
        ConfiguredReport,
    >(
        "consumer::ConfiguredReport",
        vec![
            ConstructorParam::scalar::<String>("title")
                .nullable()
                .with_default(|| Ok(ParameterValue::new(String::from("默认标题")))),
            ConstructorParam::class::<dyn Greeting>("message").with_default(|| {
                Ok(ParameterValue::new(
                    Arc::new(BaseGreeting) as Arc<dyn Greeting>
                ))
            }),
        ],
        |args| {
            let title = args.value(0)?;
            let title = if title.is_null() {
                None
            } else {
                Some(title.downcast::<String>()?)
            };
            Ok(Arc::new(ConfiguredReport(
                title,
                args.service::<dyn Greeting>(1)?,
            )))
        },
    )])?;
    builder
        .when(consumer.clone())?
        .needs(abstract_key.clone())
        .give_factory::<dyn Greeting, _>(|_| {
            Box::pin(async { Ok(Arc::new(ContextualGreeting) as Arc<dyn Greeting>) })
        })?;
    builder
        .when(consumer)?
        .needs_parameter("title")
        .give_value(ParameterValue::new(String::from("上下文标题")))?;
    let container = builder.build();
    let contextual = container.resolve::<ConfiguredReport>().await?;
    assert_eq!(
        contextual.0.as_deref().map(String::as_str),
        Some("上下文标题")
    );
    assert_eq!(contextual.1.value(), "当前消费者");
    assert!(!container.resolved_key(&abstract_key)?);

    let mut parameters = ParameterOverrides::new();
    let _ = parameters.insert(ParameterKey::name("title"), ParameterValue::null());
    let explicit = container
        .resolve_with::<ConfiguredReport>(parameters)
        .await?;
    assert!(explicit.0.is_none());
    assert_eq!(explicit.1.value(), "当前消费者");
    assert!(!container.resolved_key(&abstract_key)?);
    Ok(())
}

struct Channels(Vec<Arc<dyn Greeting>>);

struct ObserveChannel;

struct AttributedChannels(Vec<Arc<dyn Greeting>>);

impl SelfBuilding for AttributedChannels {
    fn new_instance(
        args: ResolvedArgs<'_>,
        _: Resolver,
    ) -> openrust::support::BoxFuture<'static, openrust::Result<Arc<Self>>> {
        let channels = args.values(0).and_then(|values| {
            values
                .iter()
                .map(|value| {
                    value
                        .downcast::<Arc<dyn Greeting>>()
                        .map(|greeting| Arc::clone(greeting.as_ref()))
                })
                .collect()
        });
        Box::pin(async move { channels.map(|channels| Arc::new(Self(channels))) })
    }
}

#[tokio::test]
async fn self_building_variadic_attributes_follow_projected_services_in_order()
-> openrust::Result<()> {
    let first = ServiceKey::named("consumer.attribute.first");
    let second = ServiceKey::named("consumer.attribute.second");
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        class_greeting_descriptor(Arc::new(AtomicUsize::new(0))),
        ClassDescriptor::self_building::<AttributedChannels>(
            "consumer::AttributedChannels",
            vec![],
            |_| {
                Err(openrust::Error::custom(
                    "test_unexpected_constructor",
                    "应调用自构造方法",
                ))
            },
            vec![
                ConstructorParam::class::<dyn Greeting>("channels")
                    .variadic()
                    .with_attribute(ClassAttribute::marker(|| Ok(ObserveChannel))),
            ],
        ),
    ])?;
    builder.bind_key::<ClassGreeting, _>(first.clone(), |_| {
        Box::pin(async { Ok(Arc::new(ClassGreeting("第一项".to_owned()))) })
    });
    builder.bind_key::<ClassGreeting, _>(second.clone(), |_| {
        Box::pin(async { Ok(Arc::new(ClassGreeting("第二项".to_owned()))) })
    });
    builder
        .when(ServiceKey::typed::<AttributedChannels>())?
        .needs(ServiceKey::typed::<dyn Greeting>())
        .give_many([first.clone(), second, first])?;

    let observed = Arc::new(Mutex::new(Vec::new()));
    let capture = Arc::clone(&observed);
    builder.after_resolving_attribute::<ClassGreeting>(move |event, _| {
        let capture = Arc::clone(&capture);
        Box::pin(async move {
            event.attribute().downcast::<ObserveChannel>()?;
            let concrete = event.service::<ClassGreeting>()?.ok_or_else(|| {
                openrust::Error::custom("test_concrete_missing", "缺少参数具体类型")
            })?;
            let view = event
                .service::<dyn Greeting>()?
                .ok_or_else(|| openrust::Error::custom("test_view_missing", "缺少参数接口视图"))?;
            let expected: Arc<dyn Greeting> = concrete;
            assert!(Arc::ptr_eq(&view, &expected));
            capture
                .lock()
                .map_err(|error| openrust::Error::custom("test_lock_failed", error.to_string()))?
                .push(view);
            Ok(())
        })
    })?;
    let container = builder.build();
    let channels = container.resolve::<AttributedChannels>().await?;
    assert_eq!(
        channels
            .0
            .iter()
            .map(|channel| channel.value())
            .collect::<Vec<_>>(),
        ["第一项", "第二项", "第一项"]
    );
    let observed = observed
        .lock()
        .map_err(|error| openrust::Error::custom("test_lock_failed", error.to_string()))?;
    assert_eq!(observed.len(), channels.0.len());
    assert!(
        observed
            .iter()
            .zip(&channels.0)
            .all(|(left, right)| Arc::ptr_eq(left, right))
    );
    Ok(())
}

#[tokio::test]
async fn contextual_variadic_resolves_every_key_in_declared_order() -> openrust::Result<()> {
    let first = ServiceKey::named("consumer.channel.first");
    let second = ServiceKey::named("consumer.channel.second");
    let consumer = ServiceKey::typed::<Channels>();
    let dependency = ServiceKey::typed::<dyn Greeting>();
    let mut builder =
        ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<Channels>(
            "consumer::Channels",
            vec![ConstructorParam::class::<dyn Greeting>("channels").variadic()],
            |args| {
                let channels = args
                    .values(0)?
                    .iter()
                    .map(|value| {
                        value
                            .downcast::<Arc<dyn Greeting>>()
                            .map(|arc| Arc::clone(arc.as_ref()))
                    })
                    .collect::<openrust::Result<Vec<_>>>()?;
                Ok(Arc::new(Channels(channels)))
            },
        )])?;
    builder.bind_key::<dyn Greeting, _>(first.clone(), |_| {
        Box::pin(async { Ok(Arc::new(BaseGreeting) as Arc<dyn Greeting>) })
    });
    builder.bind_key::<dyn Greeting, _>(second.clone(), |_| {
        Box::pin(async { Ok(Arc::new(ContextualGreeting) as Arc<dyn Greeting>) })
    });
    builder
        .when(consumer.clone())?
        .needs(dependency.clone())
        .give_many([second.clone(), first.clone(), second.clone()])?;
    let container = builder.build();
    let channels = container.resolve::<Channels>().await?;
    assert_eq!(
        channels
            .0
            .iter()
            .map(|channel| channel.value())
            .collect::<Vec<_>>(),
        ["当前消费者", "hello", "当前消费者"]
    );
    container.tag([first, second], ["channels"])?;
    container
        .when(consumer.clone())?
        .needs(dependency.clone())
        .give_tagged("channels")?;
    let channels = container.resolve::<Channels>().await?;
    assert_eq!(
        channels
            .0
            .iter()
            .map(|channel| channel.value())
            .collect::<Vec<_>>(),
        ["hello", "当前消费者"]
    );
    container
        .when(consumer.clone())?
        .needs(dependency.clone())
        .give_variadic_factory::<dyn Greeting, _>(|_| {
            Box::pin(async {
                Ok(vec![
                    Arc::new(ContextualGreeting) as Arc<dyn Greeting>,
                    Arc::new(BaseGreeting),
                ])
            })
        })?;
    let channels = container.resolve::<Channels>().await?;
    assert_eq!(
        channels
            .0
            .iter()
            .map(|channel| channel.value())
            .collect::<Vec<_>>(),
        ["当前消费者", "hello"]
    );
    container
        .when(consumer.clone())?
        .needs(dependency.clone())
        .give_many([ServiceKey::named("consumer.channel.missing")])?;
    assert!(container.resolve::<Channels>().await?.0.is_empty());
    let failing = ServiceKey::named("consumer.channel.business-failure");
    container
        .bind_key::<dyn Greeting, _>(failing.clone(), |_| {
            Box::pin(async {
                Err(openrust::Error::custom("service_not_bound", "业务主动拒绝"))
            })
        })
        .await?;
    container
        .when(consumer.clone())?
        .needs(dependency.clone())
        .give_many([failing])?;
    let Err(error) = container.resolve::<Channels>().await else {
        return Err(openrust::Error::custom(
            "test_missing_failure",
            "业务错误必须保留",
        ));
    };
    assert_eq!(error.to_string(), "业务主动拒绝");
    container.when(consumer)?.needs(dependency).give_many([])?;
    assert!(container.resolve::<Channels>().await?.0.is_empty());
    Ok(())
}

struct SameClassConsumer(Arc<String>);

#[tokio::test]
async fn contextual_same_class_symbol_bypasses_normal_instance() -> openrust::Result<()> {
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<String>("consumer::String", vec![], |_| {
            Ok(Arc::new(String::from("新构造")))
        }),
        ClassDescriptor::concrete::<SameClassConsumer>(
            "consumer::SameClassConsumer",
            vec![ConstructorParam::class::<String>("dependency")],
            |args| Ok(Arc::new(SameClassConsumer(args.service::<String>(0)?))),
        ),
    ])?;
    builder.instance(Arc::new(String::from("普通实例")));
    builder
        .when(ServiceKey::typed::<SameClassConsumer>())?
        .needs(ServiceKey::typed::<String>())
        .give(ServiceKey::named("consumer::String"))?;
    let container = builder.build();
    assert_eq!(
        container.resolve::<SameClassConsumer>().await?.0.as_str(),
        "新构造"
    );
    assert_eq!(container.resolve::<String>().await?.as_str(), "普通实例");
    Ok(())
}

#[tokio::test]
async fn external_original_factory_snapshot_survives_instance_override() -> openrust::Result<()> {
    let key = ServiceKey::named("consumer.raw");
    let alias = ServiceKey::named("consumer.raw.alias");
    let mut builder = ContainerBuilder::new();
    builder.singleton_key::<String, _>(key.clone(), |resolver| {
        Box::pin(async move {
            let arguments = resolver.parameters();
            arguments.get(&ParameterKey::Index(0)).map_or_else(
                || Ok(Arc::new(String::from("原始"))),
                ParameterValue::downcast::<String>,
            )
        })
    });
    builder.alias(key.clone(), alias.clone())?;
    builder.extend_key::<String, _>(&key, |original, _| {
        Box::pin(async move { Ok(Arc::new(format!("{}已装饰", original))) })
    })?;
    let container = builder.build();
    let mut bindings = container.get_bindings()?;
    assert_eq!(bindings.len(), 1);
    let binding = bindings.remove(0);
    assert_eq!(binding.key, key);
    assert!(binding.shared);
    let mut arguments = ParameterOverrides::new();
    let _ = arguments.insert(
        ParameterKey::Index(0),
        ParameterValue::new(String::from("实参")),
    );
    let raw = binding
        .concrete
        .invoke_with::<String>(container.factory_resolver()?, arguments)
        .await?;
    assert_eq!(raw.as_str(), "实参");
    assert!(!container.resolved_key(&key)?);

    let shared = container.resolve_key::<String>(alias.clone()).await?;
    assert_eq!(shared.as_str(), "原始已装饰");
    container
        .instance_key(key.clone(), Arc::new(String::from("覆盖")))
        .await?;
    let scope = container.scope()?;
    let raw = binding
        .concrete
        .invoke::<String>(scope.factory_resolver()?)
        .await?;
    assert_eq!(raw.as_str(), "原始");
    assert_eq!(
        container.resolve_key::<String>(alias).await?.as_str(),
        "覆盖"
    );
    Ok(())
}

#[tokio::test]
async fn named_class_alias_and_typed_lookup_share_the_explicit_instance() -> openrust::Result<()> {
    let class = ServiceKey::named("consumer::BaseGreeting");
    let alias = ServiceKey::named("consumer.greeting");
    let interface = ServiceKey::named("consumer::Greeting");
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<BaseGreeting>("consumer::BaseGreeting", vec![], |_| {
            Ok(Arc::new(BaseGreeting))
        })
        .with_projection(class_projection!(BaseGreeting => dyn Greeting)),
        ClassDescriptor::interface::<dyn Greeting>("consumer::Greeting"),
    ])?;
    builder.alias(class.clone(), alias.clone())?;
    let container = builder.build();

    assert!(!container.contains_key(&class));
    assert!(container.contains_key(&alias));
    assert!(container.is_alias(&alias));
    assert!(!container.is_shared_key(&alias));
    assert!(!container.resolved_key(&alias)?);
    assert_eq!(
        container.get_alias(&alias)?,
        ServiceKey::typed::<BaseGreeting>()
    );
    assert!(matches!(
        container.resolve_key::<dyn Greeting>(interface).await,
        Err(error) if error.code() == "container_class_not_instantiable"
    ));
    assert_eq!(
        container
            .resolve_key::<dyn Greeting>(alias.clone())
            .await?
            .value(),
        "hello"
    );
    assert!(container.resolved_key(&class)?);
    assert!(!container.contains::<dyn Greeting>());

    let instance = Arc::new(BaseGreeting);
    container
        .instance_key(class.clone(), Arc::clone(&instance))
        .await?;
    assert!(container.is_shared_key(&class));
    assert!(!container.is_shared_key(&alias));
    assert!(Arc::ptr_eq(
        &container.resolve::<BaseGreeting>().await?,
        &instance
    ));
    let expected: Arc<dyn Greeting> = instance;
    assert!(Arc::ptr_eq(
        &container.resolve_key::<dyn Greeting>(alias.clone()).await?,
        &expected
    ));
    container.forget_instance_key(alias.clone()).await?;
    assert!(container.is_shared_key(&class));
    container.forget_instance_key(class.clone()).await?;
    assert!(!container.contains_key(&class));
    assert!(container.resolved_key(&alias)?);
    Ok(())
}

#[tokio::test]
async fn conditional_binding_counts_instances_and_aliases_but_not_class_descriptors()
-> openrust::Result<()> {
    let class = ServiceKey::named("consumer::BaseGreeting");
    let alias = ServiceKey::named("consumer.greeting.default");
    let mut builder =
        ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<BaseGreeting>(
            "consumer::BaseGreeting",
            vec![],
            |_| Ok(Arc::new(BaseGreeting)),
        )])?;
    let registered = Arc::new(BaseGreeting);
    let default_instance = Arc::clone(&registered);
    builder.alias(class.clone(), alias.clone())?;
    assert!(!builder.contains_key(&class));
    assert!(!builder.bind_key_if(alias.clone(), |_| Box::pin(async {
        Ok(Arc::new(BaseGreeting))
    })));
    assert!(builder.singleton_if(move |_| {
        let value = Arc::clone(&default_instance);
        Box::pin(async move { Ok(value) })
    }));
    let fixed = Arc::new(7_usize);
    builder.instance(Arc::clone(&fixed));
    assert!(!builder.scoped_if(|_| Box::pin(async { Ok(Arc::new(9_usize)) })));

    let container = builder.build();
    assert!(container.is_alias(&alias));
    assert!(
        !container
            .singleton_key_if(alias.clone(), |_| Box::pin(async {
                Ok(Arc::new(BaseGreeting))
            }))
            .await?
    );
    assert!(Arc::ptr_eq(
        &container.resolve_key::<BaseGreeting>(alias).await?,
        &registered
    ));
    assert!(
        !container
            .bind_if(|_| Box::pin(async { Ok(Arc::new(10_usize)) }))
            .await?
    );
    assert!(Arc::ptr_eq(&container.resolve::<usize>().await?, &fixed));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn competing_conditional_bindings_publish_one_factory_with_its_lifetime()
-> openrust::Result<()> {
    let container = ContainerBuilder::new().build();
    let key = ServiceKey::named("consumer.raced");
    let start = Arc::new(Barrier::new(2));
    let singleton_attempt = {
        let container = container.clone();
        let key = key.clone();
        let start = Arc::clone(&start);
        tokio::spawn(async move {
            start.wait().await;
            container
                .singleton_key_if(key, |_| Box::pin(async { Ok(Arc::new(13_usize)) }))
                .await
        })
    };
    let scoped_attempt = {
        let container = container.clone();
        let key = key.clone();
        tokio::spawn(async move {
            start.wait().await;
            container
                .scoped_key_if(key, |_| Box::pin(async { Ok(Arc::new(17_usize)) }))
                .await
        })
    };
    let singleton_published = singleton_attempt
        .await
        .map_err(openrust::Error::internal)??;
    let scoped_published = scoped_attempt.await.map_err(openrust::Error::internal)??;
    assert_ne!(singleton_published, scoped_published);

    let first_scope = container.scope()?;
    let second_scope = container.scope()?;
    let first = first_scope.resolve_key::<usize>(key.clone()).await?;
    let second = second_scope.resolve_key::<usize>(key).await?;
    let expected = if singleton_published { 13 } else { 17 };
    assert_eq!((*first, *second), (expected, expected));
    assert_eq!(Arc::ptr_eq(&first, &second), singleton_published);
    first_scope.close();
    second_scope.close();
    Ok(())
}

struct DecoratedGreeting {
    inner: Arc<dyn Greeting>,
    suffix: String,
}

impl Greeting for DecoratedGreeting {
    fn value(&self) -> String {
        format!("{}{}", self.inner.value(), self.suffix)
    }
}

trait ScopeLabel: Send + Sync {
    fn value(&self) -> String;
}

struct FixedScopeLabel(String);

impl ScopeLabel for FixedScopeLabel {
    fn value(&self) -> String {
        self.0.clone()
    }
}

#[tokio::test]
async fn external_trait_factory_survives_ordered_async_extensions_and_rebinding()
-> openrust::Result<()> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut bindings = ContainerBuilder::new();
    bindings.instance(Arc::new(7usize));
    bindings.extend::<dyn Greeting, _>(|inner, resolver| {
        Box::pin(async move {
            let suffix = resolver.resolve::<usize>().await?;
            Ok(Arc::new(DecoratedGreeting {
                inner,
                suffix: format!("-{suffix}"),
            }) as Arc<dyn Greeting>)
        })
    })?;
    bindings.singleton::<dyn Greeting, _>(|_| {
        Box::pin(async { Ok(Arc::new(BaseGreeting) as Arc<dyn Greeting>) })
    });
    let observed = Arc::clone(&events);
    bindings.rebinding::<dyn Greeting, _>(move |_, current| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            observed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(current.value());
            Ok(())
        })
    })?;
    let container = bindings.build();
    assert_eq!(
        container.resolve::<dyn Greeting>().await?.value(),
        "hello-7"
    );
    container
        .extend::<dyn Greeting, _>(|inner, _| {
            Box::pin(async move {
                Ok(Arc::new(DecoratedGreeting {
                    inner,
                    suffix: "-run".into(),
                }) as Arc<dyn Greeting>)
            })
        })
        .await?;
    assert_eq!(
        container.resolve::<dyn Greeting>().await?.value(),
        "hello-7-run"
    );
    assert_eq!(
        events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        vec!["hello-7-run".to_owned()]
    );
    Ok(())
}

#[tokio::test]
async fn local_scope_extension_does_not_change_sibling_or_future_scope() -> openrust::Result<()> {
    let mut bindings = ContainerBuilder::new();
    bindings.scoped::<dyn ScopeLabel, _>(|_| {
        Box::pin(async { Ok(Arc::new(FixedScopeLabel("base".into())) as Arc<dyn ScopeLabel>) })
    });
    let container = bindings.build();
    let first = container.scope()?;
    let second = container.scope()?;
    first
        .extend::<dyn ScopeLabel, _>(|current, _| {
            Box::pin(async move {
                Ok(
                    Arc::new(FixedScopeLabel(format!("{}-first", current.value())))
                        as Arc<dyn ScopeLabel>,
                )
            })
        })
        .await?;
    assert_eq!(
        first.resolve::<dyn ScopeLabel>().await?.value(),
        "base-first"
    );
    assert_eq!(second.resolve::<dyn ScopeLabel>().await?.value(), "base");
    first.close();
    let third = container.scope()?;
    assert_eq!(third.resolve::<dyn ScopeLabel>().await?.value(), "base");
    second.close();
    third.close();
    Ok(())
}

struct SelectedScalars {
    text: Vec<u8>,
    integer: i64,
}

#[tokio::test]
async fn weak_scalars_finish_selecting_before_materializing_any_argument() -> openrust::Result<()> {
    let string_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&string_calls);
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&events);
    let constructions = Arc::new(AtomicUsize::new(0));
    let constructed = Arc::clone(&constructions);
    let descriptor = ClassDescriptor::concrete::<SelectedScalars>(
        "consumer::SelectedScalars",
        vec![
            ConstructorParam::php("text", PhpTarget::Bytes),
            ConstructorParam::php("integer", PhpTarget::Int),
            ConstructorParam::scalar::<String>("required"),
        ],
        move |args| {
            let text = args.value(0)?.downcast::<PhpBytes>()?.as_bytes().to_vec();
            let integer = *args.value(1)?.downcast::<i64>()?;
            constructed.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(SelectedScalars { text, integer }))
        },
    );
    let mut builder = ContainerBuilder::with_class_descriptors(vec![descriptor])?;
    builder.scalar_diagnostics(ScalarDiagnostics::new(move |event| {
        recorded
            .lock()
            .map_err(|_| openrust::Error::custom("test_diagnostic_lock", "无法记录构造诊断"))?
            .push(event);
        Ok(())
    }));
    let container = builder.build();
    let mut arguments = ParameterOverrides::new();
    arguments.insert(
        ParameterKey::name("text"),
        ParameterValue::new(PhpStringable::new(move || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(PhpBytes::new(Arc::<[u8]>::from(&b"a\0\xff"[..])))
        })),
    );
    arguments.insert(ParameterKey::name("integer"), ParameterValue::new(3.75_f64));
    assert!(matches!(
        container.resolve_with::<SelectedScalars>(arguments.clone()).await,
        Err(error) if error.code() == "container_parameter_missing"
    ));
    assert_eq!(string_calls.load(Ordering::Relaxed), 0);
    assert_eq!(constructions.load(Ordering::Relaxed), 0);
    assert_eq!(
        *events
            .lock()
            .map_err(|_| openrust::Error::custom("test_diagnostic_lock", "无法读取构造诊断"))?,
        []
    );

    arguments.insert(
        ParameterKey::name("required"),
        ParameterValue::new(String::from("严格输入")),
    );
    let result = container.resolve_with::<SelectedScalars>(arguments).await?;
    assert_eq!(result.text, b"a\0\xff");
    assert_eq!(result.integer, 3);
    assert_eq!(string_calls.load(Ordering::Relaxed), 1);
    assert_eq!(constructions.load(Ordering::Relaxed), 1);
    assert_eq!(
        *events
            .lock()
            .map_err(|_| openrust::Error::custom("test_diagnostic_lock", "无法读取构造诊断"))?,
        [ScalarDiagnostic::FloatLosesPrecision]
    );
    Ok(())
}

struct WeakVariadic {
    default_or_null: Option<i64>,
    items: Vec<i64>,
}

#[tokio::test]
async fn weak_scalars_keep_nullable_and_variadic_shape_and_leave_exact_types_strict()
-> openrust::Result<()> {
    let mut builder =
        ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<WeakVariadic>(
            "consumer::WeakVariadic",
            vec![
                ConstructorParam::php("limit", PhpTarget::Int)
                    .nullable()
                    .with_default(|| Ok(ParameterValue::new(String::from(" 8 ")))),
                ConstructorParam::scalar::<i64>("strict")
                    .with_default(|| Ok(ParameterValue::new(1_i64))),
                ConstructorParam::php("items", PhpTarget::Int).variadic(),
            ],
            |args| {
                let first = args.value(0)?;
                let default_or_null = if first.is_null() {
                    None
                } else {
                    Some(*first.downcast::<i64>()?)
                };
                let items = args
                    .values(2)?
                    .iter()
                    .map(|item| item.downcast::<i64>().map(|value| *value))
                    .collect::<openrust::Result<Vec<_>>>()?;
                Ok(Arc::new(WeakVariadic {
                    default_or_null,
                    items,
                }))
            },
        )])?;
    builder.scalar_diagnostics(ScalarDiagnostics::new(|_| Ok(())));
    let container = builder.build();
    let default = container.resolve::<WeakVariadic>().await?;
    assert_eq!(default.default_or_null, Some(8));
    assert!(default.items.is_empty());

    let mut arguments = ParameterOverrides::new();
    arguments.insert(ParameterKey::name("limit"), ParameterValue::null());
    arguments.insert(
        ParameterKey::name("items"),
        ParameterValue::new(String::from(" 2.75 ")),
    );
    let explicit = container
        .resolve_with::<WeakVariadic>(arguments.clone())
        .await?;
    assert_eq!(explicit.default_or_null, None);
    assert_eq!(explicit.items, [2]);

    arguments.insert(
        ParameterKey::name("strict"),
        ParameterValue::new(String::from("1")),
    );
    assert!(matches!(
        container.resolve_with::<WeakVariadic>(arguments).await,
        Err(error) if error.code() == "container_parameter_type_mismatch"
    ));
    Ok(())
}

struct RejectedUnion;

#[tokio::test]
async fn weak_scalar_diagnostic_rejection_stops_union_and_constructor() -> openrust::Result<()> {
    let constructions = Arc::new(AtomicUsize::new(0));
    let constructed = Arc::clone(&constructions);
    let mut builder = ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<
        RejectedUnion,
    >(
        "consumer::RejectedUnion",
        vec![ConstructorParam::php("value", PhpTarget::IntBool)],
        move |_| {
            constructed.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(RejectedUnion))
        },
    )])?;
    builder.scalar_diagnostics(ScalarDiagnostics::new(|event| {
        assert_eq!(event, ScalarDiagnostic::FloatLosesPrecision);
        Err(openrust::Error::custom(
            "test_precision_rejected",
            "诊断策略拒绝精度丢失",
        ))
    }));
    let container = builder.build();
    let mut arguments = ParameterOverrides::new();
    arguments.insert(ParameterKey::name("value"), ParameterValue::new(7.25_f64));
    assert!(matches!(
        container.resolve_with::<RejectedUnion>(arguments).await,
        Err(error) if error.code() == "test_precision_rejected"
    ));
    assert_eq!(constructions.load(Ordering::Relaxed), 0);
    Ok(())
}

type HookLog = Arc<Mutex<Vec<&'static str>>>;

fn record_resolution_hook(
    events: HookLog,
    label: &'static str,
) -> impl Fn(ResolutionEvent, Resolver) -> openrust::support::BoxFuture<'static, openrust::Result<()>>
{
    move |_, _| {
        let events = Arc::clone(&events);
        Box::pin(async move {
            events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(label);
            Ok(())
        })
    }
}

fn take_resolution_hooks(events: &HookLog) -> Vec<&'static str> {
    std::mem::take(
        &mut *events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

#[tokio::test]
async fn resolving_hooks_order_alias_groups_and_observe_shared_hits() -> openrust::Result<()> {
    let key = ServiceKey::named("consumer.hooks.message");
    let alias = ServiceKey::named("consumer.hooks.message.alias");
    let mut builder = ContainerBuilder::new();
    builder.singleton_key::<String, _>(key.clone(), |resolver| {
        Box::pin(async move {
            let parameters = resolver.parameters();
            parameters.get(&ParameterKey::name("text")).map_or_else(
                || Ok(Arc::new(String::from("默认"))),
                ParameterValue::downcast,
            )
        })
    });
    builder.alias(key.clone(), alias.clone())?;

    let events = HookLog::default();
    let observe = |label| record_resolution_hook(Arc::clone(&events), label);
    let parameters_seen = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let observed_parameters = Arc::clone(&parameters_seen);
    let expected_key = key.clone();
    builder.before_resolving(Some(alias.clone()), move |event, _| {
        let expected_key = expected_key.clone();
        let observed_parameters = Arc::clone(&observed_parameters);
        Box::pin(async move {
            assert_eq!(event.key(), &expected_key);
            assert!(event.value::<String>()?.is_none());
            let parameters = event.parameters();
            let text = parameters
                .get(&ParameterKey::name("text"))
                .map(ParameterValue::downcast::<String>)
                .transpose()?
                .map(|value| value.as_ref().clone());
            observed_parameters
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(text);
            Ok(())
        })
    })?;
    builder.before_resolving(Some(alias.clone()), observe("指定键解析前"))?;
    builder.before_resolving(None, observe("全局解析前"))?;
    builder.resolving(Some(ServiceKey::typed::<String>()), observe("类型解析一"))?;
    builder.resolving(Some(alias.clone()), observe("别名解析"))?;
    builder.resolving(Some(ServiceKey::typed::<String>()), observe("类型解析二"))?;
    builder.resolving(None, observe("全局解析"))?;
    builder.after_resolving(Some(alias.clone()), observe("别名解析后"))?;
    builder.after_resolving(None, observe("全局解析后"))?;

    let container = builder.build();
    let phases = [
        "全局解析前",
        "指定键解析前",
        "全局解析",
        "类型解析一",
        "类型解析二",
        "别名解析",
        "全局解析后",
        "别名解析后",
    ];
    let first = container.resolve_key::<String>(alias.clone()).await?;
    assert_eq!(first.as_str(), "默认");
    assert_eq!(take_resolution_hooks(&events), phases);
    let hit = container.resolve_key::<String>(key.clone()).await?;
    assert!(Arc::ptr_eq(&first, &hit));
    assert_eq!(
        take_resolution_hooks(&events),
        ["全局解析前", "指定键解析前"]
    );

    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        ParameterKey::name("text"),
        ParameterValue::new(String::from("带参")),
    );
    let overridden = container
        .resolve_key_with::<String>(alias, parameters)
        .await?;
    assert_eq!(overridden.as_str(), "带参");
    assert!(!Arc::ptr_eq(&first, &overridden));
    assert_eq!(take_resolution_hooks(&events), phases);
    assert_eq!(
        *parameters_seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        [None, None, Some(String::from("带参"))],
    );
    Ok(())
}

struct HookDependency(&'static str);
struct HookGreeting(Arc<HookDependency>);

impl Greeting for HookGreeting {
    fn value(&self) -> String {
        self.0.0.to_owned()
    }
}

#[tokio::test]
async fn resolving_hooks_keep_wrapped_dependencies_and_skip_raw_outer_callbacks()
-> openrust::Result<()> {
    let abstract_key = ServiceKey::typed::<dyn Greeting>();
    let concrete_key = ServiceKey::typed::<HookGreeting>();
    let dependency_key = ServiceKey::typed::<HookDependency>();
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<HookDependency>("consumer::HookDependency", vec![], |_| {
            Ok(Arc::new(HookDependency("依赖已构造")))
        }),
        ClassDescriptor::concrete::<HookGreeting>(
            "consumer::HookGreeting",
            vec![ConstructorParam::class::<HookDependency>("dependency")],
            |args| Ok(Arc::new(HookGreeting(args.service(0)?))),
        )
        .with_projection(class_projection!(HookGreeting => dyn Greeting)),
    ])?;
    builder.bind_class::<dyn Greeting>(abstract_key.clone(), concrete_key.clone());

    let events = HookLog::default();
    let observe = |label| record_resolution_hook(Arc::clone(&events), label);
    builder.before_resolving(Some(abstract_key.clone()), observe("外层解析前"))?;
    builder.before_resolving(Some(concrete_key.clone()), observe("内层误触发"))?;
    builder.before_resolving(Some(dependency_key.clone()), observe("依赖解析前"))?;
    builder.resolving(Some(dependency_key.clone()), observe("依赖解析"))?;
    builder.after_resolving(Some(dependency_key), observe("依赖解析后"))?;
    let projected = Arc::clone(&events);
    let expected_key = abstract_key.clone();
    builder.resolving(Some(concrete_key), move |event, _| {
        let projected = Arc::clone(&projected);
        let expected_key = expected_key.clone();
        Box::pin(async move {
            assert_eq!(event.key(), &expected_key);
            let concrete = event.value::<HookGreeting>()?.ok_or_else(|| {
                openrust::Error::custom("hook_concrete_missing", "解析事件缺少具体类型视图")
            })?;
            let actual = event.value::<dyn Greeting>()?.ok_or_else(|| {
                openrust::Error::custom("hook_trait_missing", "解析事件缺少接口视图")
            })?;
            let expected: Arc<dyn Greeting> = concrete;
            assert!(Arc::ptr_eq(&expected, &actual));
            projected
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push("外层具体类型");
            Ok(())
        })
    })?;
    builder.resolving(Some(abstract_key.clone()), observe("外层接口类型"))?;
    builder.after_resolving(Some(abstract_key.clone()), observe("外层解析后"))?;

    let container = builder.build();
    let first = container.resolve::<dyn Greeting>().await?;
    assert_eq!(first.value(), "依赖已构造");
    assert_eq!(
        take_resolution_hooks(&events),
        [
            "外层解析前",
            "依赖解析前",
            "依赖解析",
            "依赖解析后",
            "外层具体类型",
            "外层接口类型",
            "外层解析后"
        ],
    );

    let wrapper = container
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == abstract_key)
        .ok_or_else(|| openrust::Error::custom("hook_binding_missing", "缺少类绑定包装器"))?;
    let raw = wrapper
        .concrete
        .invoke::<dyn Greeting>(container.factory_resolver()?)
        .await?;
    assert_eq!(raw.value(), "依赖已构造");
    assert!(!Arc::ptr_eq(&first, &raw));
    assert_eq!(
        take_resolution_hooks(&events),
        ["依赖解析前", "依赖解析", "依赖解析后"]
    );
    Ok(())
}

#[tokio::test]
async fn before_resolving_can_bind_the_service_and_keeps_the_current_alias_target()
-> openrust::Result<()> {
    let original = ServiceKey::named("consumer.hooks.original");
    let alternate = ServiceKey::named("consumer.hooks.alternate");
    let alias = ServiceKey::named("consumer.hooks.repoint");
    let mut builder = ContainerBuilder::new();
    builder.bind_key::<String, _>(alternate.clone(), |_| {
        Box::pin(async { Ok(Arc::new(String::from("下一次解析"))) })
    });
    builder.alias(original.clone(), alias.clone())?;

    let alias_for_hook = alias.clone();
    builder.before_resolving(Some(alias.clone()), move |event, _| {
        let alias = alias_for_hook.clone();
        let alternate = alternate.clone();
        Box::pin(async move {
            let container = event.container()?;
            container.alias(alternate, alias)?;
            container
                .bind_key::<String, _>(event.key().clone(), |_| {
                    Box::pin(async { Ok(Arc::new(String::from("本次解析"))) })
                })
                .await
        })
    })?;

    let container = builder.build();
    assert_eq!(
        container
            .resolve_key::<String>(alias.clone())
            .await?
            .as_str(),
        "本次解析"
    );
    assert_eq!(
        container.resolve_key::<String>(alias).await?.as_str(),
        "下一次解析"
    );
    assert!(container.resolved_key(&original)?);
    Ok(())
}

#[tokio::test]
async fn resolving_callback_can_read_the_current_published_singleton() -> openrust::Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.singleton::<String, _>(|_| Box::pin(async { Ok(Arc::new(String::from("共享"))) }));
    let observed = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&observed);
    builder.resolving(
        Some(ServiceKey::typed::<String>()),
        move |event, resolver| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                let cached = resolver.resolve::<String>().await?;
                let current = event.value::<String>()?.ok_or_else(|| {
                    openrust::Error::custom("hook_shared_missing", "解析事件缺少共享对象")
                })?;
                assert!(Arc::ptr_eq(&cached, &current));
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
        },
    )?;
    let container = builder.build();
    let first = openrust::tokio::time::timeout(
        std::time::Duration::from_secs(3),
        container.resolve::<String>(),
    )
    .await
    .map_err(|_| {
        openrust::Error::custom("hook_shared_wait_timeout", "解析回调等待当前共享对象超时")
    })??;
    let second = container.resolve::<String>().await?;
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(observed.load(Ordering::Relaxed), 1);
    Ok(())
}
