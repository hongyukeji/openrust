//! 验证外部 package 可沿公开 API 装饰 dyn trait 与隔离 scope。

use openrust::class_projection;
use openrust::container::{
    ClassDescriptor, ContainerBuilder, ParameterKey, ParameterOverrides, ParameterValue, ServiceKey,
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
