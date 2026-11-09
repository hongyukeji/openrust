use super::*;
use crate::container::{ParameterKey, ParameterValue};
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::sync::{Notify, oneshot};

struct Receiver;
struct Dependency(&'static str);

#[tokio::test]
async fn bound_dispatch_resolves_class_first_and_bypasses_default_injection() -> Result<()> {
    let constructed = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::new();
    let observed = Arc::clone(&constructed);
    builder.bind::<Receiver, _>(move |resolver| {
        observed.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            assert!(resolver.parameters().is_empty());
            Ok(Arc::new(Receiver))
        })
    });
    builder.register_method::<Receiver, Arc<Receiver>>(
        "run",
        vec![ConstructorParam::scalar::<String>("required")],
        |_, _, _| {
            Box::pin(async {
                Err(container_error("unexpected_default", "默认方法不应执行"))
            })
        },
    )?;
    builder.bind_method("run", |receiver: Arc<Receiver>, _| {
        Box::pin(async move { Ok(receiver) })
    })?;
    let container = builder.build();
    assert!(container.has_method_binding::<Receiver>("run"));
    let given = Arc::new(Receiver);
    let received: Arc<Receiver> = container
        .call_method(
            MethodCall::on(Arc::clone(&given), "run"),
            ParameterOverrides::new(),
        )
        .await?;
    assert!(Arc::ptr_eq(&received, &given));
    assert_eq!(constructed.load(Ordering::Relaxed), 0);

    let mut input = ParameterOverrides::new();
    let _ = input.insert(ParameterKey::name("other"), ParameterValue::new("ignored"));
    let resolved: Arc<Receiver> = container
        .call_method(MethodCall::<Receiver>::class("run"), input)
        .await?;
    assert!(!Arc::ptr_eq(&resolved, &given));
    assert_eq!(constructed.load(Ordering::Relaxed), 1);

    container.bind_method("run", |_: Arc<Receiver>, _| {
        Box::pin(async {
            Err::<Arc<Receiver>, _>(container_error("replacement", "替代绑定执行"))
        })
    })?;
    container.flush()?;
    assert!(container.has_method_binding::<Receiver>("run"));
    let failed: Result<Arc<Receiver>> = container
        .call_method(MethodCall::on(given, "run"), ParameterOverrides::new())
        .await;
    assert!(matches!(failed, Err(error) if error.code() == "replacement"));

    container
        .bind::<Receiver, _>(|_| {
            Box::pin(async { Err(container_error("test_constructor", "接收者构造失败")) })
        })
        .await?;
    let failed: Result<Arc<Receiver>> = container
        .call_method(
            MethodCall::<Receiver>::class("run"),
            ParameterOverrides::new(),
        )
        .await;
    assert!(matches!(failed, Err(error) if error.code() == "test_constructor"));
    Ok(())
}

#[tokio::test]
async fn instance_and_class_method_context_match_their_entry_paths() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    let receiver = Arc::new(Receiver);
    builder.instance(Arc::clone(&receiver));
    builder.instance(Arc::new(Dependency("root")));
    builder
        .when(ServiceKey::typed::<Receiver>())?
        .needs(ServiceKey::typed::<Dependency>())
        .give_factory(|_| Box::pin(async { Ok(Arc::new(Dependency("receiver"))) }))?;
    builder.register_method::<Receiver, (&'static str, Resolver)>(
        "read",
        vec![ConstructorParam::class::<Dependency>("dep")],
        |_, args, resolver| {
            let result = args
                .resolved
                .service::<Dependency>(0)
                .map(|dep| (dep.0, resolver));
            Box::pin(async move { result })
        },
    )?;
    let container = builder.build();
    let (label, escaped): (&str, Resolver) = container
        .call_method(MethodCall::on(receiver, "read"), ParameterOverrides::new())
        .await?;
    assert_eq!(label, "receiver");
    assert_eq!(escaped.resolve::<Dependency>().await?.0, "root");
    let (label, _): (&str, Resolver) = container
        .call_method(
            MethodCall::<Receiver>::class("read"),
            ParameterOverrides::new(),
        )
        .await?;
    assert_eq!(label, "root");
    let wrong: Result<bool> = container
        .call_method(
            MethodCall::<Receiver>::class("read"),
            ParameterOverrides::new(),
        )
        .await;
    assert!(matches!(wrong, Err(error) if error.code() == "container_method_output_mismatch"));
    Ok(())
}

#[tokio::test]
async fn invokable_class_uses_binding_and_direct_receiver_uses_the_declaration() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    let receiver = Arc::new(Receiver);
    builder.instance(Arc::clone(&receiver));
    builder.bind_method("__invoke", |_: Arc<Receiver>, _| {
        Box::pin(async { Ok("bound") })
    })?;
    let container = builder.build();
    let missing: Result<&str> = container
        .call_method(
            MethodCall::<Receiver>::invokable(),
            ParameterOverrides::new(),
        )
        .await;
    assert!(matches!(missing, Err(error) if error.code() == "container_method_missing"));
    container.register_method("__invoke", vec![], |_: Arc<Receiver>, _, _| {
        Box::pin(async { Ok("declared") })
    })?;
    let bound: &str = container
        .call_method(
            MethodCall::<Receiver>::invokable(),
            ParameterOverrides::new(),
        )
        .await?;
    assert_eq!(bound, "bound");
    let declared: &str = container
        .call_method(
            MethodCall::direct(receiver, "__invoke"),
            ParameterOverrides::new(),
        )
        .await?;
    assert_eq!(declared, "declared");
    Ok(())
}

#[tokio::test]
async fn live_rebinding_does_not_replace_a_running_method() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(Receiver));
    let entered = Arc::new(Notify::new());
    let (release, wait) = oneshot::channel();
    let wait = std::sync::Mutex::new(Some(wait));
    let ready = Arc::clone(&entered);
    builder.bind_method("run", move |_: Arc<Receiver>, _| {
        let wait = lock(&wait).take();
        let ready = Arc::clone(&ready);
        Box::pin(async move {
            ready.notify_one();
            wait.ok_or_else(missing_slot)?
                .await
                .map_err(crate::Error::internal)?;
            Ok("original")
        })
    })?;
    let container = builder.build();
    let running = container.clone();
    let old = tokio::spawn(async move {
        running
            .call_method::<Receiver, &str>(MethodCall::class("run"), ParameterOverrides::new())
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .map_err(crate::Error::internal)?;
    container.bind_method("run", |_: Arc<Receiver>, _| {
        Box::pin(async { Ok("replacement") })
    })?;
    let new: &str = container
        .call_method(
            MethodCall::<Receiver>::class("run"),
            ParameterOverrides::new(),
        )
        .await?;
    assert_eq!(new, "replacement");
    release.send(()).map_err(|()| missing_slot())?;
    assert_eq!(old.await.map_err(crate::Error::internal)??, "original");
    Ok(())
}

struct Middle;

#[tokio::test]
async fn nested_methods_keep_active_consumer_and_restore_it_after_error() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.instance(Arc::new(Receiver));
    builder.instance(Arc::new(Middle));
    builder.instance(Arc::new(Dependency("root")));
    builder
        .when(ServiceKey::typed::<Receiver>())?
        .needs(ServiceKey::typed::<Dependency>())
        .give_factory(|_| Box::pin(async { Ok(Arc::new(Dependency("outer"))) }))?;
    builder
        .when(ServiceKey::typed::<Middle>())?
        .needs(ServiceKey::typed::<Dependency>())
        .give_factory(|_| Box::pin(async { Ok(Arc::new(Dependency("middle"))) }))?;
    builder.register_method::<Receiver, &str>(
        "read",
        vec![ConstructorParam::class::<Dependency>("dependency")],
        |_, args, _| {
            let result = args.resolved.service::<Dependency>(0).map(|value| value.0);
            Box::pin(async move { result })
        },
    )?;
    builder.bind_method("middle", |_: Arc<Middle>, resolver| {
        Box::pin(async move {
            let receiver = resolver.resolve::<Receiver>().await?;
            let inner: &str = resolver
                .call_method(MethodCall::on(receiver, "read"), ParameterOverrides::new())
                .await?;
            assert_eq!(inner, "middle");
            Err::<(), _>(container_error("nested_failure", "嵌套绑定失败"))
        })
    })?;
    builder.bind_method("outer", |_: Arc<Receiver>, resolver| {
        Box::pin(async move {
            let receiver = resolver.resolve::<Middle>().await?;
            let nested: Result<()> = resolver
                .call_method(
                    MethodCall::on(receiver, "middle"),
                    ParameterOverrides::new(),
                )
                .await;
            assert!(matches!(nested, Err(error) if error.code() == "nested_failure"));
            Ok(resolver.resolve::<Dependency>().await?.0)
        })
    })?;
    let container = builder.build();
    let receiver = container.resolve::<Receiver>().await?;
    let output: &str = container
        .call_method(MethodCall::on(receiver, "outer"), ParameterOverrides::new())
        .await?;
    assert_eq!(output, "outer");
    assert_eq!(container.resolve::<Dependency>().await?.0, "root");
    Ok(())
}
