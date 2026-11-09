use super::*;
use crate::container::{
    AttributeParameter, AttributeValue, ClassAttribute, ClassDescriptor, ContainerBuilder,
    ContextualAttribute, ServiceKey,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[derive(Debug, PartialEq)]
struct Dep(u32);

struct Pick(Arc<AtomicUsize>);

impl ContextualAttribute for Pick {
    fn resolve(
        self: Arc<Self>,
        parameter: AttributeParameter,
        _: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            assert_eq!(parameter.name(), "dep");
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(AttributeValue::One(ParameterValue::service(Arc::new(Dep(
                11,
            )))))
        })
    }

    fn after(self: Arc<Self>, _: AttributeValue, _: Resolver) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            self.0.fetch_add(10, Ordering::Relaxed);
            Ok(())
        })
    }
}

#[tokio::test]
async fn explicit_names_precede_attributes_and_unused_values_keep_order() -> Result<()> {
    let container = ContainerBuilder::new().build();
    let trace = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&trace);
    let signature = [ConstructorParam::class::<Dep>("dep")
        .nullable()
        .with_attribute(ClassAttribute::contextual(move || {
            Ok(Pick(Arc::clone(&observed)))
        }))];
    let mut inputs = ParameterOverrides::new();
    let _ = inputs.insert(ParameterKey::Index(7), ParameterValue::new(5u32));
    let _ = inputs.insert(
        ParameterKey::name(ServiceKey::typed::<Dep>().name()),
        ParameterValue::service(Arc::new(Dep(17))),
    );
    let _ = inputs.insert(ParameterKey::name("dep"), ParameterValue::null());
    let _ = inputs.insert(ParameterKey::name("extra"), ParameterValue::new(23u32));
    let consumed = container
        .call(&signature, inputs.clone(), |args| {
            let values = (|| {
                assert!(args.resolved.value(0)?.is_null());
                assert_eq!(args.remaining.len(), 3);
                assert_eq!(*args.remaining[0].downcast::<u32>()?, 5);
                assert_eq!(**args.remaining[1].downcast::<Arc<Dep>>()?, Dep(17));
                Ok(*args.remaining[2].downcast::<u32>()?)
            })();
            Box::pin(async move { values })
        })
        .await?;
    assert_eq!(consumed, 23);
    assert_eq!(trace.load(Ordering::Relaxed), 10);

    let mut attribute_inputs = ParameterOverrides::new();
    let _ = attribute_inputs.insert(
        ParameterKey::name(ServiceKey::typed::<Dep>().name()),
        ParameterValue::service(Arc::new(Dep(17))),
    );
    let dep = container
        .call(&signature, attribute_inputs, |args| {
            let value = args.resolved.service::<Dep>(0);
            assert_eq!(args.remaining.len(), 1);
            Box::pin(async move { value })
        })
        .await?;
    assert_eq!(*dep, Dep(11));
    assert_eq!(trace.load(Ordering::Relaxed), 21);
    Ok(())
}

#[tokio::test]
async fn class_override_is_consumed_once_and_default_precedes_unbound_construction() -> Result<()> {
    let built = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&built);
    let container =
        ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<Dep>(
            "tests::Dep",
            vec![],
            move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(Arc::new(Dep(29)))
            },
        )])?
        .build();
    let signature = [
        ConstructorParam::class::<Dep>("first"),
        ConstructorParam::class::<Dep>("second")
            .with_default(|| Ok(ParameterValue::service(Arc::new(Dep(13))))),
    ];
    let mut inputs = ParameterOverrides::new();
    let _ = inputs.insert(
        ParameterKey::name(ServiceKey::typed::<Dep>().name()),
        ParameterValue::service(Arc::new(Dep(19))),
    );
    let result = container
        .call(&signature, inputs, |args| {
            let values = (|| {
                assert!(args.remaining.is_empty());
                Ok((
                    args.resolved.service::<Dep>(0)?.0,
                    args.resolved.service::<Dep>(1)?.0,
                ))
            })();
            Box::pin(async move { values })
        })
        .await?;
    assert_eq!(result, (19, 13));
    assert_eq!(built.load(Ordering::Relaxed), 0);
    let values = container
        .call(&signature, ParameterOverrides::new(), |args| {
            let values = (|| {
                Ok((
                    args.resolved.service::<Dep>(0)?.0,
                    args.resolved.service::<Dep>(1)?.0,
                ))
            })();
            Box::pin(async move { values })
        })
        .await?;
    assert_eq!(values, (29, 13));
    assert_eq!(built.load(Ordering::Relaxed), 1);
    container.instance(Arc::new(Dep(31))).await?;
    let values = container
        .call(&signature, ParameterOverrides::new(), |args| {
            let values = (|| {
                Ok((
                    args.resolved.service::<Dep>(0)?.0,
                    args.resolved.service::<Dep>(1)?.0,
                ))
            })();
            Box::pin(async move { values })
        })
        .await?;
    assert_eq!(values, (31, 31));
    Ok(())
}

#[tokio::test]
async fn variadic_arguments_append_unused_values_without_spreading_explicit_vectors() -> Result<()>
{
    let container = ContainerBuilder::new().build();
    let signature = [ConstructorParam::scalar::<u32>("numbers").variadic()];
    let mut inputs = ParameterOverrides::new();
    let _ = inputs.insert(ParameterKey::Index(0), ParameterValue::new(7u32));
    let _ = inputs.insert(ParameterKey::name("numbers"), ParameterValue::new(3u32));
    let _ = inputs.insert(ParameterKey::Index(1), ParameterValue::new(13u32));
    let values = container
        .call(&signature, inputs, |args| {
            assert!(args.remaining.is_empty());
            let values = args.resolved.values(0).and_then(|values| {
                values
                    .iter()
                    .map(|value| value.downcast::<u32>().map(|value| *value))
                    .collect::<Result<Vec<_>>>()
            });
            Box::pin(async move { values })
        })
        .await?;
    assert_eq!(values, [3, 7, 13]);

    let mut inputs = ParameterOverrides::new();
    let _ = inputs.insert(
        ParameterKey::name("numbers"),
        ParameterValue::new(vec![3u32, 7]),
    );
    let called = AtomicUsize::new(0);
    let rejected = container
        .call(&signature, inputs, |_| {
            called.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(()) })
        })
        .await;
    assert!(matches!(rejected, Err(error) if error.code() == "container_parameter_type_mismatch"));
    assert_eq!(called.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test]
async fn scoped_callable_keeps_factory_inputs_local_and_scope_close_cancels_it() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.scoped::<Dep, _>(|resolver| {
        Box::pin(async move {
            assert!(resolver.parameters().is_empty());
            Ok(Arc::new(Dep(37)))
        })
    });
    let container = builder.build();
    let scope = container.scope()?;
    let signature = [ConstructorParam::class::<Dep>("dep")];
    let mut inputs = ParameterOverrides::new();
    let _ = inputs.insert(ParameterKey::name("other"), ParameterValue::new(true));
    let through_call = scope
        .call(&signature, inputs, |args| {
            let dep = args.resolved.service::<Dep>(0);
            Box::pin(async move { dep })
        })
        .await?;
    assert!(Arc::ptr_eq(&through_call, &scope.resolve::<Dep>().await?));
    assert!(!Arc::ptr_eq(
        &through_call,
        &container.resolve::<Dep>().await?
    ));

    let entered = tokio::sync::Notify::new();
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            scope.call(&signature, ParameterOverrides::new(), |_| Box::pin(async {
                entered.notify_one();
                std::future::pending::<Result<()>>().await
            })),
            async {
                entered.notified().await;
                scope.close();
            },
        )
    })
    .await
    .map_err(crate::Error::internal)?;
    assert!(matches!(result, Err(error) if error.code() == "scope_closed"));
    Ok(())
}

struct HeldParameter {
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl Drop for HeldParameter {
    fn drop(&mut self) {
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
        }
        let _ = self
            .release
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recv_timeout(Duration::from_secs(5));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn shutdown_waits_for_explicit_parameter_disposal() -> Result<()> {
    let container = ContainerBuilder::new().build();
    let scope = Arc::new(container.scope()?);
    let (entered, disposed) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let mut inputs = ParameterOverrides::new();
    let _ = inputs.insert(
        ParameterKey::Index(0),
        ParameterValue::new(HeldParameter {
            entered: Some(entered),
            release: std::sync::Mutex::new(released),
        }),
    );
    let call_scope = Arc::clone(&scope);
    let call = tokio::spawn(async move {
        call_scope
            .call(&[], inputs, |_| Box::pin(async { Ok(()) }))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), disposed)
        .await
        .map_err(crate::Error::internal)?
        .map_err(crate::Error::internal)?;
    let mut shutdown = tokio::spawn(async move {
        scope
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(2))
            .await
    });
    let early = tokio::time::timeout(Duration::from_millis(40), &mut shutdown).await;
    release.send(()).map_err(crate::Error::internal)?;
    assert!(early.is_err(), "输入析构完成前关闭栅栏提前返回");
    call.await.map_err(crate::Error::internal)??;
    shutdown.await.map_err(crate::Error::internal)??;
    Ok(())
}
