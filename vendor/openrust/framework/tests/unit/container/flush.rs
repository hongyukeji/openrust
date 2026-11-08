use super::*;
use std::sync::atomic::AtomicUsize;

#[tokio::test]
async fn flush_clears_all_cached_instances_but_keeps_registration_rules() -> Result<()> {
    let service = ServiceKey::typed::<usize>();
    let alias = ServiceKey::named("app.counter");
    let mut builder = ContainerBuilder::new();
    builder.scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    builder.instance(Arc::new(String::from("旧实例")));
    builder.extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 10)) }))?;
    let resolutions = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&resolutions);
    builder.resolving(Some(service.clone()), move |_, _| {
        seen.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Ok(()) })
    })?;
    let container = builder.build();
    container.alias(service.clone(), alias.clone())?;
    let root = container.resolve_key::<usize>(alias.clone()).await?;
    let first = container.scope()?;
    let second = container.scope()?;
    first
        .extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 100)) }))
        .await?;
    let old_first = first.resolve::<usize>().await?;
    let old_second = second.resolve::<usize>().await?;
    assert_eq!((*root, *old_first, *old_second), (11, 111, 11));
    assert!(container.resolved_key(&service)?);
    assert_eq!(resolutions.load(Ordering::Relaxed), 3);

    let rebound = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&rebound);
    container
        .rebinding::<usize, _>(move |_, _| {
            called.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(()) })
        })
        .await?;

    container.flush()?;
    container.flush()?;
    assert!(container.get_bindings()?.is_empty());
    assert!(!container.contains::<String>());
    assert!(!container.contains_key(&service));
    assert!(!container.is_alias(&alias));
    assert_eq!(container.get_alias(&alias)?, alias);
    assert!(!container.is_shared_key(&service));
    assert!(!container.resolved_key(&service)?);
    assert_eq!(rebound.load(Ordering::Relaxed), 0);
    assert!(matches!(first.resolve::<usize>().await, Err(e) if e.code() == "service_not_bound"));
    assert!(matches!(second.resolve::<usize>().await, Err(e) if e.code() == "service_not_bound"));

    container
        .scoped::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
        .await?;
    assert_eq!(*container.resolve::<usize>().await?, 12);
    assert_eq!(*first.resolve::<usize>().await?, 112);
    assert_eq!(*second.resolve::<usize>().await?, 12);
    assert_eq!((*root, *old_first, *old_second), (11, 111, 11));
    assert_eq!(resolutions.load(Ordering::Relaxed), 6);
    assert_eq!(rebound.load(Ordering::Relaxed), 0);
    container
        .bind::<usize, _>(|_| Box::pin(async { Ok(Arc::new(3)) }))
        .await?;
    assert_eq!(rebound.load(Ordering::Relaxed), 1);
    Ok(())
}

#[tokio::test]
async fn flush_does_not_republish_old_singleton_or_invalidate_raw_snapshot() -> Result<()> {
    let release = Arc::new(Notify::new());
    let mut builder = ContainerBuilder::new();
    let blocked = Arc::clone(&release);
    builder.singleton::<usize, _>(move |_| {
        let blocked = Arc::clone(&blocked);
        Box::pin(async move {
            blocked.notified().await;
            Ok(Arc::new(1))
        })
    });
    builder.bind::<String, _>(|resolver| {
        Box::pin(async move { Ok(Arc::new(resolver.resolve::<usize>().await?.to_string())) })
    });
    let container = builder.build();
    let snapshot = container
        .get_bindings()?
        .into_iter()
        .find(|binding| binding.key == ServiceKey::typed::<String>())
        .ok_or_else(|| container_error("test_factory_missing", "缺少原工厂"))?;
    let mut owner = Box::pin(container.resolve::<usize>());
    let mut waiter = Box::pin(container.resolve::<usize>());
    assert!(futures_util::poll!(owner.as_mut()).is_pending());
    assert!(futures_util::poll!(waiter.as_mut()).is_pending());

    container.flush()?;
    assert!(!container.resolved_key(&ServiceKey::typed::<usize>())?);
    container
        .singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
        .await?;
    let new = container.resolve::<usize>().await?;
    release.notify_one();
    assert_eq!(*owner.await?, 1);
    assert_eq!(*waiter.await?, 1);
    assert!(Arc::ptr_eq(&new, &container.resolve::<usize>().await?));
    let raw = snapshot
        .concrete
        .invoke::<String>(container.factory_resolver()?)
        .await?;
    assert_eq!(&*raw, "2");
    assert!(!container.contains::<String>());
    Ok(())
}

#[tokio::test]
async fn flush_rejects_old_extension_publication() -> Result<()> {
    let mut builder = ContainerBuilder::new();
    builder.singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(1)) }));
    builder.extend::<usize, _>(|value, _| Box::pin(async move { Ok(Arc::new(*value + 10)) }))?;
    let container = builder.build();
    assert_eq!(*container.resolve::<usize>().await?, 11);
    let release = Arc::new(Notify::new());
    let blocked = Arc::clone(&release);
    let mut old = Box::pin(container.extend::<usize, _>(move |value, _| {
        let blocked = Arc::clone(&blocked);
        Box::pin(async move {
            blocked.notified().await;
            Ok(Arc::new(*value + 100))
        })
    }));
    assert!(futures_util::poll!(old.as_mut()).is_pending());

    container.flush()?;
    container
        .singleton::<usize, _>(|_| Box::pin(async { Ok(Arc::new(2)) }))
        .await?;
    let new = container.resolve::<usize>().await?;
    assert_eq!(*new, 12);
    release.notify_one();
    assert!(matches!(old.await, Err(e) if e.code() == "binding_replaced"));
    assert!(Arc::ptr_eq(&new, &container.resolve::<usize>().await?));
    Ok(())
}

#[tokio::test]
async fn flush_releases_user_objects_outside_the_lock_and_preserves_closed_state() -> Result<()> {
    struct InspectOnDrop(Weak<Inner>, Arc<AtomicUsize>);
    impl Drop for InspectOnDrop {
        fn drop(&mut self) {
            if let Some(inner) = self.0.upgrade()
                && lock(&inner.state).bindings.is_empty()
            {
                self.1.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    let container = ContainerBuilder::new().build();
    let drops = Arc::new(AtomicUsize::new(0));
    container
        .instance(Arc::new(InspectOnDrop(
            Arc::downgrade(&container.inner),
            Arc::clone(&drops),
        )))
        .await?;
    container.flush()?;
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    container
        .shutdown_root(tokio::time::Instant::now() + std::time::Duration::from_secs(2))
        .await?;
    assert!(matches!(container.flush(), Err(e) if e.code() == "container_closed"));
    Ok(())
}
