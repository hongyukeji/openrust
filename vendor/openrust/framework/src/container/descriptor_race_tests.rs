//! 通过实际 Resolver 交错描述符构造与来源/资源变化。

use super::{
    class_descriptor::{ClassDescriptor, ConstructorParam},
    *,
};
use std::sync::atomic::AtomicUsize;
use std::time::Duration;
use tokio::time::Instant;

const LIMIT: Duration = Duration::from_secs(3);
struct Dependency;
struct Product(usize);

struct PendingBuilder {
    builder: ContainerBuilder,
    entered: Arc<Notify>,
    proceed: Arc<Notify>,
    builds: Arc<AtomicUsize>,
}

fn pending_builder() -> Result<PendingBuilder> {
    let entered = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());
    let builds = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete(
        "App/Product",
        vec![ConstructorParam::class::<Dependency>("dependency")],
        {
            let builds = Arc::clone(&builds);
            move |args| {
                let _dependency = args.service::<Dependency>(0)?;
                builds.fetch_add(1, Ordering::AcqRel);
                Ok(Arc::new(Product(1)))
            }
        },
    )])?;
    builder.bind::<Dependency, _>({
        let entered = Arc::clone(&entered);
        let proceed = Arc::clone(&proceed);
        move |_| {
            let entered = Arc::clone(&entered);
            let proceed = Arc::clone(&proceed);
            Box::pin(async move {
                entered.notify_one();
                proceed.notified().await;
                Ok(Arc::new(Dependency))
            })
        }
    });
    Ok(PendingBuilder {
        builder,
        entered,
        proceed,
        builds,
    })
}

async fn pending_product(
    call: &mut std::pin::Pin<Box<impl Future<Output = Result<Arc<Product>>>>>,
    entered: &Notify,
) -> Result<()> {
    tokio::time::timeout(LIMIT, async {
        tokio::select! {
            () = entered.notified() => Ok(()),
            _ = call => Err(container_error("test_descriptor_early", "描述符提前完成")),
        }
    })
    .await
    .map_err(Error::internal)?
}

#[tokio::test]
async fn alias_aba_after_dependency_started_does_not_rerun_constructor() -> Result<()> {
    let PendingBuilder {
        builder,
        entered,
        proceed,
        builds,
    } = pending_builder()?;
    let container = builder.build();
    let scope = container.scope()?;
    let resolver = container.resolver(
        scope.id,
        Arc::clone(&scope.closed),
        scope.resources.registrar(),
    );
    let alias = ServiceKey::named("app.product");
    let key = ServiceKey::typed::<Product>();
    container.alias_key(key.clone(), alias.clone())?;
    let mut call = Box::pin(resolver.resolve_key::<Product>(alias.clone()));
    pending_product(&mut call, &entered).await?;
    container.alias_key(ServiceKey::typed::<String>(), alias.clone())?;
    container.alias_key(key.clone(), alias)?;
    proceed.notify_one();
    assert!(
        matches!(tokio::time::timeout(LIMIT, call).await.map_err(Error::internal)?,
        Err(error) if error.code() == "binding_replaced")
    );
    assert_eq!(builds.load(Ordering::Acquire), 0);
    let state = lock(&container.inner.state);
    assert!(!state.resolved_key(&key)?);
    assert_eq!(state.extensions.descriptor_completions().count(), 0);
    Ok(())
}

#[tokio::test]
async fn extension_registered_while_descriptor_is_constructing_is_used_once() -> Result<()> {
    let PendingBuilder {
        builder,
        entered,
        proceed,
        builds,
    } = pending_builder()?;
    let container = builder.build();
    let scope = container.scope()?;
    let mut call = Box::pin(scope.resolve::<Product>());
    pending_product(&mut call, &entered).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    tokio::time::timeout(
        LIMIT,
        container.extend::<Product, _>({
            let calls = Arc::clone(&calls);
            move |value, _| {
                calls.fetch_add(1, Ordering::AcqRel);
                Box::pin(async move { Ok(Arc::new(Product(value.0 + 3))) })
            }
        }),
    )
    .await
    .map_err(Error::internal)??;
    proceed.notify_one();
    let value = tokio::time::timeout(LIMIT, call)
        .await
        .map_err(Error::internal)??;
    assert_eq!(value.0, 4);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(builds.load(Ordering::Acquire), 1);
    Ok(())
}

struct BadArgument(Arc<AtomicUsize>);
impl Drop for BadArgument {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("descriptor test parameter"));
    }
}

#[tokio::test]
async fn argument_drop_after_construction_rejects_delivery_and_leaves_history_empty() -> Result<()>
{
    let built = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete(
        "App/Product",
        vec![ConstructorParam::class::<BadArgument>("dependency")],
        {
            let built = Arc::clone(&built);
            move |args| {
                let _dependency = args.service::<BadArgument>(0)?;
                built.fetch_add(1, Ordering::AcqRel);
                Ok(Arc::new(Product(1)))
            }
        },
    )])?;
    builder.bind::<BadArgument, _>({
        let dropped = Arc::clone(&dropped);
        move |_| {
            let dropped = Arc::clone(&dropped);
            Box::pin(async move { Ok(Arc::new(BadArgument(dropped))) })
        }
    });
    let container = builder.build();
    let scope = container.scope()?;
    let result = scope.resolve::<Product>().await;
    assert!(matches!(result, Err(error) if error.code() == "resource_task_interrupted"));
    assert_eq!(built.load(Ordering::Acquire), 1);
    assert_eq!(dropped.load(Ordering::Acquire), 1);
    {
        let state = lock(&container.inner.state);
        assert!(!state.resolved_key(&ServiceKey::typed::<Product>())?);
        assert_eq!(state.extensions.descriptor_completions().count(), 0);
    }
    assert!(matches!(scope.shutdown(Instant::now() + LIMIT).await,
        Err(error) if error.code() == "resource_shutdown_failed"));
    Ok(())
}

#[tokio::test]
async fn scope_close_retires_pending_descriptor_without_executing_new() -> Result<()> {
    let PendingBuilder {
        builder,
        entered,
        proceed: _proceed,
        builds,
    } = pending_builder()?;
    let container = builder.build();
    let scope = container.scope()?;
    let mut call = Box::pin(scope.resolve::<Product>());
    pending_product(&mut call, &entered).await?;
    assert_eq!(
        lock(&container.inner.state)
            .extensions
            .descriptor_completions()
            .count(),
        1
    );
    scope.close();
    let result = tokio::time::timeout(LIMIT, call)
        .await
        .map_err(Error::internal)?;
    assert!(matches!(result, Err(error) if error.code() == "scope_closed"));
    assert_eq!(builds.load(Ordering::Acquire), 0);
    assert_eq!(
        lock(&container.inner.state)
            .extensions
            .descriptor_completions()
            .count(),
        0
    );
    scope.shutdown(Instant::now() + LIMIT).await?;
    Ok(())
}

#[tokio::test]
async fn independent_owner_seal_waits_for_the_actual_pending_caller() -> Result<()> {
    let PendingBuilder {
        builder,
        entered,
        proceed: _proceed,
        builds,
    } = pending_builder()?;
    let container = builder.build();
    let scope = container.scope()?;
    let mut call = Box::pin(scope.resolve::<Product>());
    pending_product(&mut call, &entered).await?;
    let completion = lock(&container.inner.state)
        .extensions
        .descriptor_completions()
        .next()
        .ok_or_else(|| container_error("test_descriptor_missing", "缺少内联观察"))?;
    let close = scope
        .resources
        .shutdown(Instant::now() + Duration::from_millis(10))
        .await;
    assert!(matches!(close, Err(error) if error.code() == "resource_shutdown_timeout"));
    assert!(completion.finished_interrupted().is_none());
    let result = tokio::time::timeout(LIMIT, call)
        .await
        .map_err(Error::internal)?;
    assert!(matches!(result, Err(error) if error.code() == "resources_closed"));
    assert_eq!(builds.load(Ordering::Acquire), 0);
    completion.wait().await?;
    assert_eq!(
        lock(&container.inner.state)
            .extensions
            .descriptor_completions()
            .count(),
        0
    );
    Ok(())
}

type PrepareHook = Box<dyn FnOnce() -> Result<()>>;
std::thread_local! {
    static AFTER_PREPARE: std::cell::RefCell<Option<PrepareHook>> = const {
        std::cell::RefCell::new(None)
    };
}

pub(super) fn after_prepare() -> Result<()> {
    match AFTER_PREPARE.take() {
        Some(hook) => hook(),
        None => Ok(()),
    }
}

#[tokio::test]
async fn stale_preflight_reselects_original_lookup_before_user_code() -> Result<()> {
    let PendingBuilder {
        builder,
        entered: _entered,
        proceed: _proceed,
        builds,
    } = pending_builder()?;
    let container = builder.build();
    let key = ServiceKey::typed::<Product>();
    let alias = ServiceKey::named("app.product");
    let replacement = ServiceKey::named("app.replacement");
    container.alias_key(key.clone(), alias.clone())?;
    container
        .register_key::<Product, _>(
            replacement.clone(),
            Lifetime::Bind,
            |_| Box::pin(async { Ok(Arc::new(Product(9))) }),
            false,
        )
        .await?;
    let target = container.clone();
    let request = alias.clone();
    AFTER_PREPARE.replace(Some(Box::new(move || {
        target.alias_key(replacement, request)
    })));
    let scope = container.scope()?;
    let resolver = container.resolver(
        scope.id,
        Arc::clone(&scope.closed),
        scope.resources.registrar(),
    );
    let value = tokio::time::timeout(LIMIT, resolver.resolve_key::<Product>(alias))
        .await
        .map_err(Error::internal)??;
    assert_eq!(value.0, 9);
    assert_eq!(builds.load(Ordering::Acquire), 0);
    let state = lock(&container.inner.state);
    assert!(!state.resolved_history.contains(&key));
    assert_eq!(state.extensions.descriptor_completions().count(), 0);
    Ok(())
}

#[tokio::test]
async fn removed_extender_captures_are_retired_before_history_publication() -> Result<()> {
    let container = ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete(
        "App/Product",
        Vec::new(),
        |_| Ok(Arc::new(Product(1))),
    )])?
    .build();
    let entered = Arc::new(Notify::new());
    let proceed = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    let capture = BadArgument(Arc::clone(&dropped));
    container
        .extend::<Product, _>({
            let entered = Arc::clone(&entered);
            let proceed = Arc::clone(&proceed);
            move |value, _| {
                let _capture = &capture;
                let entered = Arc::clone(&entered);
                let proceed = Arc::clone(&proceed);
                Box::pin(async move {
                    entered.notify_one();
                    proceed.notified().await;
                    Ok(value)
                })
            }
        })
        .await?;
    let scope = container.scope()?;
    let mut call = Box::pin(scope.resolve::<Product>());
    pending_product(&mut call, &entered).await?;
    tokio::time::timeout(LIMIT, container.forget_extenders::<Product>())
        .await
        .map_err(Error::internal)??;
    assert_eq!(dropped.load(Ordering::Acquire), 0);
    proceed.notify_one();
    assert!(
        matches!(tokio::time::timeout(LIMIT, call).await.map_err(Error::internal)?,
        Err(error) if error.code() == "resource_task_interrupted")
    );
    assert_eq!(dropped.load(Ordering::Acquire), 1);
    {
        let state = lock(&container.inner.state);
        assert!(!state.resolved_key(&ServiceKey::typed::<Product>())?);
        assert_eq!(state.extensions.descriptor_completions().count(), 0);
    }
    assert!(matches!(scope.shutdown(Instant::now() + LIMIT).await,
        Err(error) if error.code() == "resource_shutdown_failed"));
    Ok(())
}
