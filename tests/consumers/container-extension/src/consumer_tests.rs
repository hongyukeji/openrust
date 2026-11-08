//! 验证外部 package 可沿公开 API 装饰 dyn trait 与隔离 scope。

use openrust::class_projection;
use openrust::container::{
    ClassDescriptor, ConstructorParam, ContainerBuilder, ParameterKey, ParameterOverrides,
    ParameterValue, ServiceKey,
};
use std::sync::{Arc, Mutex};
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
